# OKX x402 Java SDK — External Team Integration Guide

Audience: product / backend teams that want to charge crypto payments on
their HTTP endpoints, or call x402-protected endpoints from a Java client,
by embedding the x402 Java SDK.

The SDK is published as three Maven artifacts under the group `com.okx`:
`x402-java-core` (servlet-agnostic logic), `x402-java-jakarta` (Jakarta EE 9+
/ Spring Boot 3 bindings), and `x402-java-javax` (Java EE 8 / Spring Boot 2
bindings). Pick **one** of the adapter modules; the core is pulled in
transitively.

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
| Java 17+ runtime & JDK | SDK compile target is 17 | SDKMAN, Adoptium, Amazon Corretto, etc. |
| Maven 3.8+ | Build tool used by the SDK | `https://maven.apache.org/` |
| OKX API credentials: `API Key` + `Secret Key` + `Passphrase` | Required on the **server / facilitator-caller** side to call `/api/v6/pay/x402/*` (HMAC-SHA256 auth) | Apply through the OKX API console; IP-allowlist the calling service |
| A `payTo` wallet address on X Layer | Receives settled funds | A treasury / wallet-ops-owned EOA on `eip155:196` |
| (Client, `exact` scheme) Private key of a funded EOA on X Layer | Signs EIP-3009 `TransferWithAuthorization` | Treasury; kept in KMS / Vault, **never** in source |
| (Client, `aggr_deferred` scheme) Registered x402 AA account + session certificate + session private key | Session-key signing for AI-agent batching | OKX Wallet TEE: AA registration flow issues the `sessionCert` |
| Network egress to `https://www.okx.com` | Facilitator endpoint | Open firewall / proxy rules |

If any of the above is missing the SDK will fail at first call — close
the checklist before wiring code.

---

## 2. Add the SDK

### 2.1 Repository

The public release of `com.okx:x402-java-*` is published to **Maven Central**
— no extra `<repositories>` entry is required if your build already resolves
against Central (the default for Maven and most Gradle setups).

If you are integrating against an in-development snapshot or an internal
mirror, add the corresponding `<repository>` entry to your `settings.xml`
or build's `pom.xml` per your organisation's conventions.

### 2.2 Dependency

Pick the adapter that matches your servlet namespace. Do not depend on
both; they expose the same package names and would clash on the classpath.

**Jakarta EE 9+ / Spring Boot 3 (jakarta.servlet.*):**

```xml
<dependency>
  <groupId>com.okx</groupId>
  <artifactId>x402-java-jakarta</artifactId>
  <version>1.0.0</version>
</dependency>
```

**Java EE 8 / Spring Boot 2 (javax.servlet.*):**

```xml
<dependency>
  <groupId>com.okx</groupId>
  <artifactId>x402-java-javax</artifactId>
  <version>1.0.0</version>
</dependency>
```

**Other frameworks (Vert.x, Play, Micronaut Netty, plain Undertow, …):**
depend on `com.okx:x402-java-core` directly and implement the
`com.okx.x402.server.X402Request` / `X402Response` interfaces against your
native request/response types. The jakarta adapter is ~50 lines and serves
as a reference.

Transitive deps the SDK pulls in (via `x402-java-core`):

- `com.fasterxml.jackson.core:jackson-databind:2.17.0`
- `org.web3j:core:4.12.3`
- `org.bouncycastle:bcprov-jdk18on:1.78.1`

Provided (expected on the host app's classpath):

- Jakarta variant: `jakarta.servlet:jakarta.servlet-api:6.x` and (only for `PaymentInterceptor`) `org.springframework:spring-webmvc:6.x`
- Javax variant: `javax.servlet:javax.servlet-api:4.x` and (only for `PaymentInterceptor`) `org.springframework:spring-webmvc:5.x`

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

For Spring Boot, a `FilterRegistrationBean` controls ordering relative
to other filters (billing, auth, etc.):

```java
@Bean
FilterRegistrationBean<PaymentFilter> x402Filter(
        FacilitatorClient facilitator,
        Map<String, PaymentProcessor.RouteConfig> routes) {
    FilterRegistrationBean<PaymentFilter> reg =
            new FilterRegistrationBean<>(PaymentFilter.create(facilitator, routes));
    reg.addUrlPatterns("/api/*");
    reg.setOrder(20);   // runs after billing filter at order 10
    return reg;
}
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

Register an `onProtectedRequest` hook on the processor. The hook runs
after route match and before the payment header is read, and can
inspect anything an upstream billing filter/interceptor stashed on the
request:

```java
// Upstream billing interceptor sets some request attribute when the
// request is on a free tier; the payment layer reads it:
filter.processor().onProtectedRequest((req, route) -> {
    if (Boolean.TRUE.equals(req.unwrap() instanceof HttpServletRequest hsr
            ? hsr.getAttribute("billing.free") : null)) {
        return PaymentHooks.ProtectedRequestResult.grantAccess();
    }
    return PaymentHooks.ProtectedRequestResult.proceed();
});
```

See `CONFIG.md §B4` for the full decision table (`proceed` /
`grantAccess` / `abort`).

### 3.4 Async settlement (must-configure if enabled)

```java
ExecutorService settlePool = Executors.newFixedThreadPool(16, r -> {
    Thread t = new Thread(r, "x402-settle");
    t.setDaemon(true);
    return t;
});

PaymentFilter.create(facilitator, Map.of("GET /api/data", route))
    .processor()
    .settleExecutor(settlePool)                           // required when route.asyncSettle=true
    .onAsyncSettleComplete((payload, req, result, err) -> {
        if (err != null) log.error("settle failed", err);
        else log.info("settle tx={}", result.transaction);
    });
```

Missing `settleExecutor(...)` while `asyncSettle=true` throws
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

Full hook table in `CONFIG.md §B5`.

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

Key requirements in `CONFIG.md §A5.1`.

### 4.2 `aggr_deferred` scheme (AI agent / session key)

External teams using this scheme must:

1. Register an x402 AA account via OKX Wallet.
2. Obtain a session certificate from the Wallet TEE binding the session
   key to the AA account.
3. Sign with the **session private key** — the authorization's `from` is
   the **AA wallet address**, not the session-key address.

Signer wiring for this scheme is not a one-liner like `exact`; consult
the OKX Wallet team for a session-signer that implements `EvmSigner`.
See `CONFIG.md §A5.2`.

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
| `PaymentProcessor.postHandle` | `IllegalStateException` | `asyncSettle=true` without `settleExecutor` |

Rate-limit (`50011`) and HTTP 429 responses are **retried automatically**
by the SDK (3 attempts, 1-2-4 s back-off). Callers do not need to retry.

Full error-code table in `CONFIG.md §C5`.

**Tuning HTTP timeouts / sharing a connection pool:** both
`OKXFacilitatorClient` and `OKXHttpClient` accept a config object
(`OKXFacilitatorConfig` / `OKXHttpClientConfig`) that exposes
`connectTimeout`, `requestTimeout`, and an optional caller-supplied
`java.net.http.HttpClient` for integrating with your org's HTTP stack
(metrics, tracing, proxy, shared `Executor`). See `CONFIG.md §A2.1` and
`§B7.1`.

**Using OkHttp / Apache HttpClient / Reactor Netty (facilitator only):**
`OKXFacilitatorConfig.httpExecutor` takes an `HttpExecutor` — a
two-method SPI — so you can route facilitator HTTP through any stack
while the SDK keeps owning HMAC auth, envelope unwrapping, error-code
mapping, and retry logic. OkHttp adapter is ~25 lines; full recipe in
`CONFIG.md §A2.2`. The SDK does not depend on OkHttp.

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

Steps 1–5 should be run against a non-production facilitator endpoint
(see `CONFIG.md §A2`) before ever pointing at `www.okx.com`.

---

## 8. Go-live checklist

- [ ] Maven Central (or your internal mirror) reachable from the build
- [ ] `OKX_*` secrets in Vault / KMS, injected at startup
- [ ] `payTo` wallet allowlisted by Treasury
- [ ] Firewall open to `www.okx.com` (and any non-production facilitator URL you use during integration testing)
- [ ] Routes declared with correct `scheme` / `network` / `price`
- [ ] Custom assets (if any) registered **before** filter creation
- [ ] `settleExecutor` injected if any route uses `asyncSettle`
- [ ] Lifecycle hooks wired to metrics / audit
- [ ] Billing layer sets the `billing.free` request attribute (`Boolean.TRUE`) on free-tier requests (matching the `onProtectedRequest` example in §3.2)
- [ ] Error handling for `IOException` / `CryptoSignException` at
      the caller sites
- [ ] Integration tests (§7) passing against a non-production facilitator
- [ ] Runbook entry for OKX error codes (`CONFIG.md §C5`) and retry
      semantics

---

## 9. Support & references

- API reference: `java/README.md`
- Config reference: `java/CONFIG.md`
- Architecture: `java/ARCHITECTURE.md`
- x402 spec: https://www.x402.org/
- OKX facilitator endpoints: `POST /api/v6/pay/x402/{verify,settle,supported}`,
  `GET /api/v6/pay/x402/settle/status`
- Issues / onboarding questions: open a GitHub issue on the SDK repository, or
  contact your OKX integration manager for credential / allowlist matters
- Security disclosures: see `SECURITY.md` at the repository root
