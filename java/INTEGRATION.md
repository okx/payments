# OKX x402 Java SDK — External Team Integration Guide

Audience: product / backend teams that want to charge crypto payments on
their HTTP endpoints, or call x402-protected endpoints from a Java client,
by embedding the `x402-java` SDK.

This document lists **everything an external team has to obtain and
implement** before the SDK can run end-to-end in their service.

- API reference → `README.md`
- Full config reference (env vars, `RouteConfig`, `AssetRegistry`,
  timeouts, error codes) → `CONFIG.md`
- This doc → **prerequisites, wiring, responsibilities, checklist**

---

## 1. Prerequisites (what to get before writing code)

| Item | Why it is needed | Where to get it |
|---|---|---|
| Java 17+ runtime & JDK | SDK compile target is 17 | internal image / SDKMAN |
| Maven 3.8+ with access to `https://nexus.okg.com/repository/maven-releases/` | SDK artifact `com.okx:x402-java` is published here | OKG Nexus; add the repository to `settings.xml` or `pom.xml` |
| OKX API credentials: `API Key` + `Secret Key` + `Passphrase` | Required on the **server / facilitator-caller** side to call `/api/v6/pay/x402/*` (HMAC-SHA256 auth) | Apply through the OKX API console; IP-allowlist the calling service |
| A `payTo` wallet address on X Layer | Receives settled funds | Treasury / wallet-ops-owned EOA on `eip155:196` |
| (Client, `exact` scheme) Private key of a funded EOA on X Layer | Signs EIP-3009 `TransferWithAuthorization` | Treasury; kept in KMS / Vault, **never** in source |
| (Client, `aggr_deferred` scheme) Registered x402 AA account + session certificate + session private key | Session-key signing for AI-agent batching | OKX Wallet TEE: AA registration flow issues the `sessionCert` |
| Network egress to `https://www.okx.com` (prod) and `https://forked-okx-test2-dataasset.okx.testokg.com` (swimlane) | Facilitator endpoint | Open firewall / proxy rules |

If any of the above is missing the SDK will fail at first call — close
the checklist before wiring code.

---

## 2. Add the SDK

### 2.1 Nexus repository

```xml
<repositories>
  <repository>
    <id>okg-releases</id>
    <url>https://nexus.okg.com/repository/maven-releases/</url>
  </repository>
  <repository>
    <id>okg-snapshots</id>
    <url>https://nexus.okg.com/repository/maven-snapshots/</url>
    <snapshots><enabled>true</enabled></snapshots>
  </repository>
</repositories>
```

### 2.2 Dependency

```xml
<dependency>
  <groupId>com.okx</groupId>
  <artifactId>x402-java</artifactId>
  <version>1.0.0-SNAPSHOT</version>
</dependency>
```

Transitive deps the SDK pulls in:

- `com.fasterxml.jackson.core:jackson-databind:2.17.0`
- `org.web3j:core:4.12.3`
- `org.bouncycastle:bcprov-jdk18on:1.78.1`

Provided (expected on the host app's classpath):

- `jakarta.servlet:jakarta.servlet-api:6.x` — required for `PaymentFilter` / `PaymentInterceptor`
- `org.springframework:spring-webmvc:6.x` — required **only** if you use `PaymentInterceptor`

Java HTTP is the JDK built-in `java.net.http.HttpClient`; no extra HTTP
library needed.

---

## 3. Server-side integration

See `CONFIG.md` for env vars, facilitator construction and the full
`RouteConfig` schema. This section is about **where to wire it**.

Pick **one** adapter; do not wire both.

### 3.1 Servlet filter (Jetty / Tomcat / Spring Boot without MVC interceptors)

```java
PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
route.scheme  = "exact";
route.network = "eip155:196";
route.payTo   = payTo;
route.price   = "$0.01";

PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /api/data", route));

// Register with your container (example: Spring Boot ServletContextInitializer)
ctx.addFilter("x402", filter).addMappingForUrlPatterns(null, false, "/api/*");
```

### 3.2 Spring MVC `HandlerInterceptor` (preferred if you already have interceptors)

```java
@Configuration
class X402Config implements WebMvcConfigurer {
    @Override
    public void addInterceptors(InterceptorRegistry r) {
        r.addInterceptor(billingInterceptor).order(10);                  // first
        r.addInterceptor(PaymentInterceptor.create(facilitator, routes)) // after billing
                .order(20)
                .addPathPatterns("/api/**");
    }
}
```

Use this when your billing layer already runs as an interceptor — order
via `InterceptorRegistry.order()` is simpler than mixing filters and
interceptors.

### 3.3 Billing / free-request coordination

Any upstream filter or interceptor that classifies a request as "free"
sets a request attribute before the payment layer runs:

```java
// Zero-coupling convention — no SDK import required upstream
request.setAttribute("paymentExempt", Boolean.TRUE);
```

Or wire a predicate (see `CONFIG.md §5.3`).

### 3.4 Async settlement (must-configure if enabled)

```java
ExecutorService settlePool = Executors.newFixedThreadPool(16, r -> {
    Thread t = new Thread(r, "x402-settle");
    t.setDaemon(true);
    return t;
});

PaymentFilter.create(facilitator, Map.of("GET /api/data", route))
    .processor()
    .settleExecutor(settlePool)                           // required when route.settleInBackground=true
    .onAsyncSettleComplete((payload, req, result, err) -> {
        if (err != null) log.error("settle failed", err);
        else log.info("settle tx={}", result.transaction);
    });
```

Missing `settleExecutor(...)` while `settleInBackground=true` throws
`IllegalStateException` at runtime.

### 3.5 Lifecycle hooks (optional, for metrics / audit / abort)

```java
processor
    .onBeforeVerify((p, r) -> AbortResult.proceed())
    .onAfterVerify((p, r, resp) -> metrics.verifyOk())
    .onVerifyFailure((p, r, e) -> RecoverResult.notRecovered())
    .onBeforeSettle((p, r) -> AbortResult.proceed())
    .onAfterSettle((p, r, resp) -> auditLog.write(resp))
    .onSettleFailure((p, r, e) -> RecoverResult.notRecovered());
```

Full hook table in `CONFIG.md §5.4`.

---

## 4. Client-side integration

### 4.1 `exact` scheme (EOA private key)

```java
OKXEvmSigner signer  = new OKXEvmSigner(System.getenv("PRIVATE_KEY"));
OKXHttpClient client = new OKXHttpClient(signer, "eip155:196");

HttpResponse<String> resp = client.get(URI.create("https://seller/api/data"));
// SDK auto-handles 402 → sign → retry → 200
String proof = resp.headers().firstValue("PAYMENT-RESPONSE").orElse(null);
```

Key requirements in `CONFIG.md §6.1`.

### 4.2 `aggr_deferred` scheme (AI agent / session key)

External teams using this scheme must:

1. Register an x402 AA account via OKX Wallet.
2. Obtain a session certificate from the Wallet TEE binding the session
   key to the AA account.
3. Sign with the **session private key** — the authorization's `from` is
   the **AA wallet address**, not the session-key address.

Signer wiring for this scheme is not a one-liner like `exact`; consult
the OKX Wallet team for a session-signer that implements `EvmSigner`.
See `CONFIG.md §6.2`.

---

## 5. What the host service is responsible for

The SDK does **not** own the following; the integrating team must
implement or wire them:

- **Secrets management** — loading OKX keys, private keys and session
  certs from Vault / KMS (the SDK only reads values handed to it).
- **Thread pool for async settle** — SDK throws if you enable background
  settle without injecting an `Executor` (§3.4).
- **Observability** — the SDK logs nothing by default; use lifecycle
  hooks (§3.5) to emit metrics, traces, audit rows.
- **Rate-limiting / DDoS** — the facilitator has its own rate limits
  (error code `50011`), but the seller endpoint itself still needs
  normal protection because 402 generation is free.
- **Nonce / replay storage** — EIP-3009 nonces are stored by the
  on-chain token contract; the SDK does not keep its own nonce DB.
- **Route inventory** — keeping the `Map<String, RouteConfig>` in sync
  with deployed endpoints.

---

## 6. Error handling contract

| Source | Throws | Meaning |
|---|---|---|
| `OKXFacilitatorClient.verify/settle/...` | `IOException` | Network error, HTTP ≥400, or OKX envelope `code != 0` (e.g. `OKX API error on /verify (code=50103): Invalid API key`) |
| `OKXFacilitatorClient.*` | `InterruptedException` | Request interrupted |
| `OKXEvmSigner.signPaymentRequirements` | `CryptoSignException` | Missing `extra.name` / `extra.version`, invalid key, etc. |
| `PaymentProcessor.postHandle` | `IllegalStateException` | `settleInBackground=true` without `settleExecutor` |

Rate-limit (`50011`) and HTTP 429 responses are **retried automatically**
by the SDK (3 attempts, 1-2-4 s back-off). Callers do not need to retry.

Full error-code table in `CONFIG.md §9`.

---

## 7. Integration test plan

Before going to prod, external teams should verify:

1. `supported()` returns the expected kinds for your network.
2. Happy-path 402 → sign → 200 with a single route.
3. Invalid signature is rejected with 402.
4. `syncSettle=true` returns a real tx hash; `syncSettle=false` returns
   `status="pending"`.
5. Async settle (if enabled) delivers the callback on success **and**
   failure, on the injected thread pool.
6. Exempt request passes through without calling facilitator (observe
   zero `/verify` traffic).
7. Facilitator rate-limit path is exercised by the SDK's retry.

Steps 1–5 can run against the swimlane URL without real funds
(`CONFIG.md §2.1`).

---

## 8. Go-live checklist

- [ ] Nexus credentials configured in build
- [ ] `OKX_*` secrets in Vault / KMS, injected at startup
- [ ] `payTo` wallet allowlisted by Treasury
- [ ] Firewall open to `www.okx.com` (and swimlane if needed)
- [ ] Routes declared with correct `scheme` / `network` / `price`
- [ ] Custom assets (if any) registered **before** filter creation
- [ ] `settleExecutor` injected if any route uses `settleInBackground`
- [ ] Lifecycle hooks wired to metrics / audit
- [ ] Billing layer sets `paymentExempt` attribute on free requests
- [ ] Error handling for `IOException` / `CryptoSignException` at
      the caller sites
- [ ] Integration tests (§7) passing against swimlane
- [ ] Runbook entry for OKX error codes (`CONFIG.md §9`) and retry
      semantics

---

## 9. Support & references

- API reference: `java/README.md`
- Config reference: `java/CONFIG.md`
- Architecture: `java/ARCHITECTURE.md`
- x402 spec: https://www.x402.org/
- OKX facilitator endpoints: `POST /api/v6/pay/x402/{verify,settle,supported}`,
  `GET /api/v6/pay/x402/settle/status`
- Questions / onboarding: #x402-sdk (internal)
