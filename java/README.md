<!-- Reviewed: AI-assisted drafting + human review completed. -->
> **Status:** Reviewed — passed AI & human review.

# OKX Payments Java SDK

[![Java Version](https://img.shields.io/badge/java-17%2B-orange)](https://github.com/okx/payments-sdk/java)

Java implementation of the OKX on-chain payment stack — covering both the
[x402](https://www.x402.org/) protocol (per-request HTTP 402 on X Layer)
and the **Machine Payment Protocol (MPP)**.

This README is the single integration manual for **sellers**. Buyer-side
integration is out of scope for this SDK release; the reference buyer
implementation is the OKX `onchainos` CLI.

## Modules

| Module | Artifact | Role | Servlet API |
|---|---|---|---|
| `core` | `com.okx:okxweb3-app-x402-core` | Servlet-agnostic x402 verify/settle logic + signers. | — |
| `jakarta` | `com.okx:okxweb3-app-x402-jakarta` | x402 Filter / Interceptor adapter for **Jakarta** (Servlet 5 / Spring Boot 3). | `jakarta.servlet` |
| `javax` | `com.okx:okxweb3-app-x402-javax` | x402 Filter / Interceptor adapter for **javax** (Servlet 4 / Spring Boot 2). | `javax.servlet` |
| `mpp` | `com.okx:okxweb3-app-mpp` | MPP seller SDK — charge (one-shot) + session (channel). | `javax.servlet` |
| `payment-router` | `com.okx:okxweb3-app-payment-router` | Cross-protocol 402 dispatcher (x402 + MPP on the same URL). Optional. | `javax.servlet` |

Namespace constraint: `okxweb3-app-mpp` and `okxweb3-app-payment-router` are `javax`. Use
`okxweb3-app-x402-javax` if you want both protocols served from one runtime;
pure-x402 deployments may use `okxweb3-app-x402-jakarta`.

## Supported payment paths

| Protocol | Scheme | Transfer method | Use case |
|---|---|---|---|
| x402 | **`exact`** | **`eip3009`** (default) — `transferWithAuthorization` | EOA pays a fixed amount per request on an EIP-3009 token (USDT0 / USDG). Immediate on-chain settlement. |
| x402 | **`exact`** | **`permit2`** | Same as above but routed through OKX `x402ExactPermit2Proxy` (`0x402085…0001`). Works for any ERC-20 once the buyer has approved the canonical Permit2 contract. |
| x402 | **`upto`** | **`permit2`** (required) | Cap-style metered billing: buyer signs an upper-bound; facilitator settles ≤ cap. Routed through `x402UptoPermit2Proxy` (`0x4020e7…0002`); witness binds to the settlement facilitator. |
| x402 | **`aggr_deferred`** | n/a | Session-key signing — OKX facilitator TEE batches N payments into 1 on-chain tx. |
| x402 | **`period`** | **`permit2`** (AllowanceTransfer) | Recurring subscriptions — buyer signs `SubscriptionTerms` + a Permit2 allowance once; the facilitator charges each period via `A2APaySubscription`. Later requests authenticate with a signed `APP-Access` proof instead of paying. |
| MPP | **`charge`** intent | EIP-3009 (transaction or hash mode) | One-shot pay-per-call. SA settles a single tx; SDK exposes a `verifyCharge` façade. |
| MPP | **`session`** intent | EIP-712 voucher | Open one escrowed channel; sign incrementing vouchers off-chain; bill many URLs against the same channel; one on-chain settle at the end. |

For x402, the transfer method is signalled to the buyer via
`paymentRequirements.extra.assetTransferMethod`; seller picks per route.
For MPP, the intent (`charge` vs `session`) is encoded in the
`WWW-Authenticate: Payment` challenge.

---

## Prerequisites

| Item | Why it is needed | Where to get it |
|---|---|---|
| Java 17+ runtime & JDK | SDK compile target is 17 | Adoptium / Corretto / SDKMAN |
| Maven 3.8+ | Build tool | `https://maven.apache.org/` |
| OKX API key + secret + passphrase | Server-side HMAC auth to `/api/v6/pay/*` | OKX API console; IP-allowlist the calling service |
| `payTo` / `MPP_PAYEE_ADDRESS` wallet on X Layer | Receives settled funds | Treasury-owned EOA on `eip155:196` |
| `MPP_HMAC_SECRET` (32+ random bytes) | HMAC-signs the MPP 402 `Challenge` body | Generate at deploy time; rotate via secrets manager |
| `MPP_PAYEE_PRIVATE_KEY` (hex 32B) | Merchant EOA used by `PrivateKeyPayeeAuthSigner` for MPP settle/close | Treasury; KMS/Vault only |
| (Buyer) Private key of a funded EOA on X Layer | Signs payments (x402 `exact`, MPP `charge`) | KMS/Vault, never in source |
| (Buyer, `permit2` paths) One-time on-chain `approve(Permit2, …)` | Permit2 needs allowance to move the token | Buyer EOA calls `approve(0x000000000022D473030F116dDEE9F6B43aC78BA3, smallAmount)` once per (EOA × token) |
| (Buyer, `aggr_deferred`) Registered x402 AA account + sessionCert + session private key | Session-key flow | OKX Wallet TEE issues the cert |
| Network egress to `https://www.okx.com` | Facilitator + SA endpoint | Firewall / proxy rules |

---

## Install

Pick **one** x402 adapter for your servlet namespace; add `mpp` if you
need MPP:

```xml
<!-- x402 — Jakarta EE 9+ / Spring Boot 3 -->
<dependency>
    <groupId>com.okx</groupId>
    <artifactId>okxweb3-app-x402-jakarta</artifactId>
    <version>0.2.0</version>
</dependency>

<!-- x402 — Java EE 8 / Spring Boot 2 -->
<dependency>
    <groupId>com.okx</groupId>
    <artifactId>okxweb3-app-x402-javax</artifactId>
    <version>0.2.0</version>
</dependency>

<!-- MPP — javax only -->
<dependency>
    <groupId>com.okx</groupId>
    <artifactId>okxweb3-app-mpp</artifactId>
    <version>0.2.0</version>
</dependency>

<!-- Cross-protocol dispatcher — only needed if one URL accepts both -->
<dependency>
    <groupId>com.okx</groupId>
    <artifactId>okxweb3-app-payment-router</artifactId>
    <version>0.2.0</version>
</dependency>
```

Non-servlet frameworks (Vert.x, Netty, Micronaut, …): depend on
`okxweb3-app-x402-core` and implement `X402Request` / `X402Response` against your
native types. The jakarta adapter is ~50 lines and serves as a reference.

Transitive deps pulled in by `okxweb3-app-x402-core`:

- `com.fasterxml.jackson.core:jackson-databind:2.17.0`
- `org.web3j:core:4.12.3`
- `org.bouncycastle:bcprov-jdk18on:1.78.1`

## Environment variables

| Variable | Side | Purpose |
|---|---|---|
| `OKX_API_KEY` | server (x402, MPP) | Facilitator / SA `OK-ACCESS-KEY` |
| `OKX_SECRET_KEY` | server | HMAC-SHA256 key |
| `OKX_PASSPHRASE` | server | Facilitator / SA passphrase |
| `PAY_TO_ADDRESS` | x402 server | Seller wallet on X Layer |
| `MPP_HMAC_SECRET` | MPP server | 32+ random bytes; signs MPP `Challenge`s |
| `MPP_PAYEE_PRIVATE_KEY` | MPP server | Merchant EOA hex key for settle/close auth |
| `MPP_PAYEE_ADDRESS` | MPP server | Address derived from `MPP_PAYEE_PRIVATE_KEY` |
| `MPP_CHAIN_ID` / `MPP_ESCROW_ADDRESS` | MPP server (optional) | Override defaults (X Layer mainnet) |
| `MPP_DOMAIN_NAME` / `MPP_DOMAIN_VERSION` | MPP server (optional) | Override EIP-712 domain |
| `PRIVATE_KEY` | x402 client | 0x-prefixed hex (exact / permit2 schemes) |
| `SESSION_PRIVATE_KEY` + `SESSION_CERT` | x402 client (aggr_deferred) | Session key + TEE-issued cert |

Never bake these into app config, container images, or Git.

---

# x402 — Seller integration

## 1. Build the facilitator client once

```java
import com.okx.x402.facilitator.OKXFacilitatorClient;

OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
        System.getenv("OKX_API_KEY"),
        System.getenv("OKX_SECRET_KEY"),
        System.getenv("OKX_PASSPHRASE"));
```

Override the base URL for integration testing:

```java
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
        apiKey, secretKey, passphrase,
        System.getenv("OKX_FACILITATOR_BASE_URL"));
```

For timeouts, a shared `HttpClient`, or routing through OkHttp / Apache /
Netty, use `OKXFacilitatorConfig`:

```java
OKXFacilitatorConfig cfg = new OKXFacilitatorConfig(apiKey, secretKey, passphrase);
cfg.requestTimeout = Duration.ofSeconds(60);
cfg.httpClient     = mySharedHttpClient;          // JDK client
cfg.httpExecutor   = new OkHttpExecutor(myOkHttp); // SPI for any stack
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(cfg);
```

Precedence: `httpExecutor > httpClient > default JDK client`. Defaults
are 10 s connect / 30 s request.

## 2. `exact + eip3009` (default route)

```java
import com.okx.x402.server.PaymentFilter;
import com.okx.x402.server.PaymentProcessor;

PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
route.scheme  = "exact";
route.network = "eip155:196";        // X Layer
route.payTo   = System.getenv("PAY_TO_ADDRESS");
route.price   = "$0.01";              // auto-resolved to atomic USDT

PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /api/data", route));

// Register on any servlet container
ctx.addFilter("x402", filter).addMappingForUrlPatterns(null, false, "/api/*");
```

Route-key lookup order: `"METHOD /path"` → `"/path"`.

## 3. `exact + permit2`

Use `AcceptOption` so the `extra.assetTransferMethod` flows in
`paymentRequirements`:

```java
import com.okx.x402.server.AcceptOption;

PaymentProcessor.RouteConfig permit2Exact = new PaymentProcessor.RouteConfig();
permit2Exact.network = "eip155:196";
permit2Exact.payTo   = payTo;
permit2Exact.accepts = List.of(
    AcceptOption.builder()
        .scheme("exact")
        .price("$0.000001")
        .extra(Map.of("assetTransferMethod", "permit2"))
        .build());

PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /api/data", permit2Exact));
```

Buyer must have already called
`approve(0x000000000022D473030F116dDEE9F6B43aC78BA3, smallAmount)` on
the token contract.

## 4. `upto + permit2`

`facilitatorAddress` must be advertised in `extra` — discover it once at
startup:

```java
import com.okx.x402.model.v2.SupportedKind;

String uptoFacilitator = lookupUptoFacilitatorAddress(facilitator, "eip155:196");

PaymentProcessor.RouteConfig permit2Upto = new PaymentProcessor.RouteConfig();
permit2Upto.network = "eip155:196";
permit2Upto.payTo   = payTo;
permit2Upto.accepts = List.of(
    AcceptOption.builder()
        .scheme("upto")
        .price("$0.000001")           // upper cap; gateway rewrites amount at settle
        .extra(Map.of(
            "assetTransferMethod", "permit2",
            "facilitatorAddress",  uptoFacilitator))
        .build());

PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /api/metered", permit2Upto));

static String lookupUptoFacilitatorAddress(OKXFacilitatorClient f, String network) throws Exception {
    for (SupportedKind k : f.supported().kinds) {
        if ("upto".equalsIgnoreCase(k.scheme) && network.equalsIgnoreCase(k.network)
                && k.extra != null && k.extra.get("facilitatorAddress") instanceof String s) {
            return s;
        }
    }
    throw new IllegalStateException("no upto facilitatorAddress on " + network);
}
```

## 5. `aggr_deferred` — session key for AI agents

The buyer side requires a session-key signer (AA address as `from`,
session private key as the signing key, sessionCert in
`accepted.extra.sessionCert`); coordinate with the OKX Wallet team.
The seller route is the same shape as `exact`, just with a different
scheme:

```java
PaymentProcessor.RouteConfig agentRoute = new PaymentProcessor.RouteConfig();
agentRoute.scheme  = "aggr_deferred";
agentRoute.network = "eip155:196";
agentRoute.payTo   = payTo;
agentRoute.price   = "$0.001";

PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /api/standard", exactRoute,
        "GET /api/agent",    agentRoute));
```

Settle response carries `transaction=""` and `status="success"` —
real on-chain tx happens later when the TEE batches.

## 6. Multi-currency on one endpoint

```java
PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
route.network = "eip155:196";
route.payTo   = payTo;
route.accepts = List.of(
    AcceptOption.builder()
        .scheme("exact").price("$0.01")                   // USDT (default)
        .build(),
    AcceptOption.builder()
        .scheme("exact").price("$0.01")
        .asset("0x4ae46a509f6b1d9056937ba4500cb143933d2dc8") // USDG
        .build());

PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /api/data", route));
```

## 7. Free-tier / early-reject hook

`onProtectedRequest` runs after route match, before the payment header
is read:

```java
filter.processor().onProtectedRequest((req, route) -> {
    if (Boolean.TRUE.equals(req.unwrap() instanceof HttpServletRequest hsr
            ? hsr.getAttribute("billing.free") : null)) {
        return PaymentHooks.ProtectedRequestResult.grantAccess();   // skip payment
    }
    if ("internal".equals(req.getHeader("x-api-key"))) {
        return PaymentHooks.ProtectedRequestResult.grantAccess();
    }
    if (rateLimiter.isThrottled(req)) {
        return PaymentHooks.ProtectedRequestResult.abort("rate_limited"); // HTTP 403
    }
    return PaymentHooks.ProtectedRequestResult.proceed();
});
```

Multiple hooks run in registration order; first non-`proceed()` wins.
`abort` returns HTTP 403 (not 402).

## 8. Async settlement

```java
ExecutorService settlePool = Executors.newFixedThreadPool(16, r -> {
    Thread t = new Thread(r, "x402-settle");
    t.setDaemon(true);
    return t;
});

route.asyncSettle = true;
PaymentFilter filter = PaymentFilter.create(facilitator, Map.of("GET /api/data", route));
filter.processor()
    .settleExecutor(settlePool)                       // required when asyncSettle=true
    .onAsyncSettleComplete((payload, req, result, err) -> {
        if (err != null) log.error("settle failed", err);
        else             log.info("settle tx={}", result.transaction);
    });
```

Missing `settleExecutor(...)` while `asyncSettle=true` throws
`IllegalStateException` at runtime.

## 9. Lifecycle hooks

```java
filter.processor()
    .onBeforeVerify  ((p, r)        -> AbortResult.proceed())
    .onAfterVerify   ((p, r, resp)  -> metrics.verifyOk())
    .onVerifyFailure ((p, r, e)     -> RecoverResult.notRecovered())
    .onBeforeSettle  ((p, r)        -> AbortResult.proceed())
    .onAfterSettle   ((p, r, resp)  -> auditLog.write(resp))
    .onSettleFailure ((p, r, e)     -> RecoverResult.notRecovered());
```

| Hook | Can abort? | Can recover? |
|---|---|---|
| `onBeforeVerify` | yes (`abort(reason)` → 402) | — |
| `onAfterVerify` | — | — |
| `onVerifyFailure` | — | yes (`RecoverResult.recovered(resp)`) |
| `onBeforeSettle` | yes | — |
| `onAfterSettle` | — | — |
| `onSettleFailure` | — | yes |

## 10. Spring Boot wiring

```java
@SpringBootApplication
public class App implements ServletContextInitializer {
    @Override
    public void onStartup(ServletContext ctx) {
        OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
                env("OKX_API_KEY"), env("OKX_SECRET_KEY"), env("OKX_PASSPHRASE"));

        PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
        route.network = "eip155:196";
        route.payTo   = env("PAY_TO_ADDRESS");
        route.price   = "$0.01";

        FilterRegistration.Dynamic reg = ctx.addFilter(
                "x402", PaymentFilter.create(facilitator, Map.of("GET /api/data", route)));
        reg.addMappingForUrlPatterns(null, false, "/api/*");
    }
}
```

Or as a `FilterRegistrationBean` (when you need ordering against other
filters):

```java
@Bean
FilterRegistrationBean<PaymentFilter> x402Filter(FacilitatorClient facilitator,
                                                 Map<String, PaymentProcessor.RouteConfig> routes) {
    FilterRegistrationBean<PaymentFilter> reg = new FilterRegistrationBean<>(
            PaymentFilter.create(facilitator, routes));
    reg.addUrlPatterns("/api/*");
    reg.setOrder(20);     // runs after billing filter at order 10
    return reg;
}
```

For Spring MVC `HandlerInterceptor` users:

```java
@Configuration
class X402Config implements WebMvcConfigurer {
    @Override
    public void addInterceptors(InterceptorRegistry r) {
        r.addInterceptor(billingInterceptor).order(10);
        r.addInterceptor(PaymentInterceptor.create(facilitator, routes))
                .order(20)
                .addPathPatterns("/api/**");
    }
}
```

## 11. Custom assets

Register **before** building any `PaymentFilter` / `PaymentInterceptor`:

```java
AssetRegistry.register("eip155:196", AssetConfig.builder()
        .symbol("USDG")
        .contractAddress("0x4ae46a509f6b1d9056937ba4500cb143933d2dc8")
        .decimals(6)
        .eip712Name("USDG")
        .eip712Version("1")
        .transferMethod("eip3009")
        .build());
```

The token must support EIP-3009 for the `exact` scheme.

## 12. Multi-network routing

```java
FacilitatorClient facilitator = FacilitatorRouter.builder()
        .okx(apiKey, secretKey, passphrase)   // routes eip155:196 to OKX
        .route("eip155:1", customMainnetClient)
        .defaultFacilitator(cdpClient)
        .build();
```

## 13. Resource-URL validation behind a proxy (`acceptedDomains`)

The buyer signs the target URL (`payload.resource.url`); the server checks
it against the incoming request. By default that check is strict full-URL
equality. Behind a reverse proxy / CDN / internal gateway the `Host` is
rewritten before the request reaches the backend, so the strict check
fails with `resource mismatch` even on a legitimate payment.

Set `RouteConfig.acceptedDomains` to the seller's public host names. The
check then requires the **payload URL's host ∈ `acceptedDomains`** and the
**paths to match** — the request URL's (proxy-rewritten) host is no longer
compared:

```java
PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
route.payTo = "0x...";
route.price = "$0.01";
route.acceptedDomains = List.of("web3.okx.com", "web3.okx.io");

Map<String, PaymentProcessor.RouteConfig> routes = Map.of(
        "GET /api/v6/dex/market/signal/list", route);
PaymentProcessor processor = new PaymentProcessor(facilitator, routes);
```

Leaving `acceptedDomains` null or empty preserves the legacy strict
full-URL equality — fully backward compatible. The host check is still an
intent guard: declaring the legal public domains stops a buyer being
tricked into signing a payment bound to a third-party host (path is
compared explicitly; `payTo` is still enforced later by the facilitator).

## 14. Server-side limitations

`PaymentFilter` wraps the response in an in-memory buffer so settlement
can attach the `PAYMENT-RESPONSE` header **after** your handler returns.
Three constraints follow:

1. **Do not use `response.sendError(...)` on a paid route.** It commits
   the underlying response before settlement runs. Use
   `setStatus(...) + getWriter().write(...)` for business errors.
2. **Async / non-blocking I/O is not supported on paid routes.** The
   buffered `ServletOutputStream` is synchronous; `setWriteListener(...)`
   callbacks are not delivered. Don't mark streaming endpoints
   (`StreamingResponseBody`, SSE, chunked > buffer size) as paid.
3. **`@RestController` with `PaymentInterceptor` drops the proof header.**
   Spring's message converter writes the body before `postHandle` runs,
   committing the response. Use `PaymentFilter` for `@ResponseBody`
   flows that must return the `PAYMENT-RESPONSE` header.

---

# Facilitator client API

```java
SupportedResponse sr = facilitator.supported();
// sr.kinds = [{scheme:"exact", network:"eip155:196"}, ...]

VerifyResponse  vr = facilitator.verify (payload, requirements);
SettleResponse  st = facilitator.settle (payload, requirements);        // async
SettleResponse  st = facilitator.settle (payload, requirements, true);  // sync — waits for confirmation
SettleResponse  st = facilitator.settleStatus("0xTxHash...");           // poll by tx hash
```

`OKXFacilitatorClient` auto-unwraps the `{"code":0,"data":{...}}` OKX
envelope. Non-zero `code` becomes `IOException("OKX API error on /verify
(code=50103): Invalid API key")`. HTTP 429 and OKX code `50011` are
retried automatically (3 attempts; 1 s → 2 s → 4 s back-off).

---

# x402 subscriptions — `period` scheme (Seller)

Recurring billing over Permit2 AllowanceTransfer. The buyer double-signs
once (`SubscriptionTerms` EIP-712 + `PermitSingle`); the OKX facilitator
creates the subscription on `A2APaySubscription` and charges each
period. Later requests do not pay per call — they carry a signed
`APP-Access` proof, granted while the current period is charged
(`lastChargedPeriod >= elapsedPeriods && elapsedPeriods > 0`; no
`state == ACTIVE` check, so a paid period stays accessible after a
cancel until its boundary).

## 1. Bolt onto the same pipeline

Subscription routes share the exact/upto `PaymentFilter` pipeline; the
capability is two bolt-ons on the processor:

```java
import com.okx.x402.subscription.facilitator.OKXSubscriptionFacilitatorClient;
import com.okx.x402.subscription.server.SubscriptionSchemeHandler;
import com.okx.x402.subscription.server.access.AccessProofHook;
import com.okx.x402.subscription.server.store.InMemorySubscriptionStore;

OKXSubscriptionFacilitatorClient subFacilitator = new OKXSubscriptionFacilitatorClient(
        new OKXAuth(apiKey, secretKey, passphrase),
        new JdkHttpExecutor(HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(10)).build()),
        "https://www.okx.com", Duration.ofSeconds(30));

SubscriptionStore store = new InMemorySubscriptionStore();  // cache, not source of truth;
                                                            // implement SubscriptionStore for Redis/SQL
SubscriptionSchemeHandler schemeHandler =
        new SubscriptionSchemeHandler(subFacilitator, store, subscriptionContract);

PaymentFilter filter = PaymentFilter.create(facilitator, routes);
filter.processor()
      .onProtectedRequest(new AccessProofHook(store, subFacilitator))  // APP-Access gate
      .subscriptionHandler(schemeHandler);                             // subscribe / change
```

## 2. Declare plans

One `AcceptOption` per plan; `PlanCatalog` + `ChangePlanHandler.toAcceptOption`
build the `extra` block (per-period economics, nested
`initialCharge {periodCount, totalAmount, coversFirstPeriods}`, plaintext
`plan {id, tier, features}`). Plan tiers must be unique — they decide
the upgrade/downgrade direction.

```java
PlanCatalog catalog = new PlanCatalog()
        .register("basic_monthly", new PlanCatalog.PlanEntry(
                1, "10000", 2_592_000L, 0, 12, 1, "10000", "$0.01",
                List.of("api_basic"), tokenAddress, payTo))
        .register("pro_monthly", new PlanCatalog.PlanEntry(
                2, "50000", 2_592_000L, 0, 12, 1, "50000", "$0.05",
                List.of("api_basic", "api_pro"), tokenAddress, payTo));

ChangePlanHandler.OfferContext ctx = new ChangePlanHandler.OfferContext();
ctx.chainIndex = 196;
ctx.network = "eip155:196";
ctx.subscriptionContract = subscriptionContract;
ctx.permit2Contract = permit2Contract;
ctx.facilitatorAddress = facilitatorAddress;    // facilitator EOA from GET /supported

PaymentProcessor.RouteConfig premium = new PaymentProcessor.RouteConfig();
premium.scheme = PaymentProcessor.SCHEME_PERIOD;
premium.network = "eip155:196";
premium.payTo = payTo;
premium.accepts = new ArrayList<>();
catalog.all().forEach((id, plan) ->
        premium.accepts.add(ChangePlanHandler.toAcceptOption(id, plan, ctx)));
```

`contracts` / `facilitator` / `domain` left unset are auto-injected from
the facilitator's `/supported` broadcast at 402 time; seller-pinned
values always win. `periodMode` 0 = fixed seconds, 1 = calendar month
(`periodSec` must then be 0; boundaries are month-end truncated —
Jan 31 → Feb 28). The buyer-signed terms are bound against the
advertised plan on submission (merchant / economics / token / initial
charge), so a tampered cheaper signature is rejected before it reaches
the facilitator.

## 3. Lifecycle operation routes

```java
PaymentProcessor.RouteConfig changeRoute = new PaymentProcessor.RouteConfig();
changeRoute.scheme = PaymentProcessor.SCHEME_PERIOD;
changeRoute.network = "eip155:196";
changeRoute.payTo = payTo;
changeRoute.accepts = premium.accepts;   // change candidates = the same plan menu
changeRoute.subscriptionOperation = PaymentProcessor.SubscriptionOperation.CHANGE;

PaymentProcessor.RouteConfig cancelRoute = new PaymentProcessor.RouteConfig();
cancelRoute.scheme = PaymentProcessor.SCHEME_PERIOD;
cancelRoute.subscriptionOperation = PaymentProcessor.SubscriptionOperation.CANCEL;
// CANCEL_PENDING_CHANGE is analogous — reverts a scheduled downgrade

routes.put("GET /premium", premium);
routes.put("GET /subscription/change", changeRoute);
routes.put("POST /subscription/cancel", cancelRoute);
```

- **CHANGE** — an `APP-Access` proof answers 402 + change offers: the
  buyer's current plan (and its tier) is dropped, downgrade offers lose
  their `initialCharge`, and every offer carries `extra.changeFrom`
  (`fromSubId` / direction / `effectiveAt`). A `PAYMENT-SIGNATURE` on
  the same route executes the change — upgrades are immediate,
  downgrades activate at the period boundary.
- **CANCEL / CANCEL_PENDING_CHANGE** — the buyer POSTs
  `{"subId": "...", "cancelAuth": {...}}` (buyer-signed auth); the SDK
  relays it to the facilitator and returns `{subId, txHash, state}`.

## 4. Seller-driven renewal charging

```java
SubscriptionService service = new SubscriptionService(subFacilitator, store);
scheduler.scheduleAtFixedRate(() -> {
    for (StoredSubscription sub : service.dueSubscriptions(Instant.now().getEpochSecond())) {
        try {
            service.chargeAndRecord(sub.subId);
        } catch (Exception e) {
            // terminal business failure → dunning
        }
    }
}, 0, 60, TimeUnit.SECONDS);
```

`period_not_due` / `charge_in_flight` / `all_periods_charged` come back
as idempotent no-ops; `subscription_not_active` self-heals the store
from chain truth (following `changedToSubId` after a downgrade
activation). Transient codes (`*_in_flight`, `lock_acquire_interrupted`)
are safe to retry on the next tick.

## 5. Merchant access policy

```java
accessHook.onBeforeAccess((proof, sub) ->
        bannedSubs.contains(proof.subId)
                ? SubscriptionHooks.AccessDecision.deny("access denied by merchant")
                : SubscriptionHooks.AccessDecision.proceed());
schemeHandler.onBeforeAccess(/* same hook */);  // the CHANGE offer phase gates separately
```

Runs after signature verification, before the period gate; a deny
answers a bare 402 (no offers). Whether a seller-canceled subscription
keeps access until period end is the merchant's call — implement it
here.

## 6. Wire format

| Step | Header | Content |
|---|---|---|
| 402 offer | `PAYMENT-REQUIRED` | base64 `{x402Version, error?, resource, accepts[]}` |
| subscribe / change | `PAYMENT-SIGNATURE` | base64 x402-V2 wrapped `{permitSingle, permitSingleSignature, terms, termsSignature}` |
| result | `PAYMENT-RESPONSE` | base64 `{subId, txHash, state}` |
| subsequent access | `APP-Access` | base64 `{kind:"subscription-id", subId, payer, timestamp, signature}` — EIP-191, ±300 s replay window |

Legacy `X-APP-PAYMENT*` / `X-APP-Access` aliases from earlier releases
are still read inbound.

---

# MPP — Seller integration

> **At a glance** — pay-as-you-go HTTP 402 where buyer and seller hold an
> off-chain *session* against an on-chain escrow. Buyer signs
> incrementing EIP-712 vouchers; seller bills per request locally; one
> `settle` at the end commits a single tx to chain. One open session can
> pay for many distinct URL endpoints under the same merchant.
>
> `charge` is the simpler one-shot variant: a single EIP-3009 transfer
> per request, no channel state.

## 1. Bootstrap `MppServer`

```java
import com.okx.payments.mpp.seller.MppServer;
import com.okx.payments.mpp.seller.PrivateKeyPayeeAuthSigner;
import com.okx.payments.mpp.sa.SaApiConfig;

MppServer server = MppServer.builder()
    .saApiConfig(SaApiConfig.builder()
        .baseUrl(env("OKX_BASE_URL"))
        .apiKey(env("OKX_AK"))
        .secretKey(env("OKX_SK"))
        .passphrase(env("OKX_PASSPHRASE"))
        .build())
    .challengeSecretKey(env("MPP_HMAC_SECRET").getBytes(StandardCharsets.UTF_8))
    .payeeAuthSigner(PrivateKeyPayeeAuthSigner.fromEnvVar("MPP_PAYEE_PRIVATE_KEY"))
    // Defaults to X Layer mainnet (chainId=196, escrow=0x5E55…CE3b).
    // .domain(EvmPaymentChannelDomain.builder().chainId(196L).escrowAddress("0x...").build())
    // .sessionStore(new MysqlSessionStore(ds))   // see §6 below
    .build();
```

## 2. `charge` intent — one-shot pay-per-call

Façade-style: no Filter, no response buffering. The merchant handler
runs only after `verifyCharge` returns successfully.

```java
import com.okx.payments.mpp.protocol.Challenge;
import com.okx.payments.mpp.protocol.Credential;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.protocol.charge.ChargeRequest;
import com.okx.payments.mpp.protocol.charge.ChargeMethodDetails;
import com.okx.payments.mpp.protocol.encoding.Base64UrlJson;
import com.okx.payments.router.adapters.MppAdapter;

@RestController
class PayPerCallController {
    private final MppServer mpp;
    private final Base64UrlJson codec = new Base64UrlJson();

    PayPerCallController(MppServer mpp) { this.mpp = mpp; }

    @PostMapping("/pay-once")
    ResponseEntity<?> payOnce(
            @RequestHeader(value = "Authorization", required = false) String auth,
            @RequestBody MyBusinessRequest body) {

        if (auth == null || !auth.toLowerCase().startsWith("payment ")) {
            ChargeRequest req = new ChargeRequest(
                    "10000",                                            // 0.01 USDT (6 decimals)
                    "0x779ded0c9e1022225f8e0630b35a9b54be713736",       // currency
                    "0xYourTreasuryWallet",                             // recipient
                    "one-shot purchase",                                // description
                    body.orderId(),                                     // externalId — echoed in receipt
                    new ChargeMethodDetails(196L, false, null, null, null,
                            "/pay-once"));                              // resourceUrl — see below
            Challenge challenge = mpp.request("my-realm", Intent.CHARGE, req);
            return ResponseEntity.status(402)
                    .header("WWW-Authenticate", MppAdapter.serializeWwwAuth(challenge))
                    .build();
        }

        Credential cred = codec.decode(auth.substring("Payment ".length()).trim(), Credential.class);
        ChargeRequest req = codec.decode(cred.challenge().request(), ChargeRequest.class);
        Receipt.ChargeReceipt receipt = mpp.evmChargeMethod().verifyCharge(cred, req);

        MyBusinessResponse resp = doTheActualWork(body);
        return ResponseEntity.ok()
                .header(MppAdapter.PAYMENT_RECEIPT_HEADER, codec.encode(receipt))
                .body(resp);
    }
}
```

Two payload sub-modes are auto-dispatched by `verifyCharge` on
`payload.type`:

- **`transaction`** — buyer signs EIP-3009 off-chain; SA broadcasts the tx. Buyer needs no native gas.
- **`hash`** — buyer broadcasts the tx themselves and submits the tx hash; SA verifies the on-chain receipt.

`ChargeMethodDetails.resourceUrl` (charge-only, optional) tags the
charge with the endpoint it was raised against — full URL, path, or any
logical name. The SDK does not parse or validate the value; it is
base64url-embedded in `challenge.request` and forwarded verbatim to
`POST /charge/settle` and `POST /charge/verifyHash` so the SA backend
can attribute revenue / volume per endpoint. Pass `null` (or use the
legacy 5-arg constructor) to omit the field. Session intent has no
counterpart — one session spans multiple endpoints.

## 3. `session` intent — multi-URL channel

Wire `PaymentRouterFilter` once. Business `@GetMapping` handlers run
unchanged. The filter handles `/session/manage` (open / topUp / voucher /
close) as a terminal endpoint; on each resource call it runs the handler
then deducts the route's price from the channel.

```java
import com.okx.payments.mpp.server.MppRouteConfig;
import com.okx.payments.mpp.protocol.session.SessionMethodDetails;
import com.okx.payments.router.PaymentRouterConfig;
import com.okx.payments.router.PaymentRouterFilter;
import com.okx.payments.router.RouteConfig;
import com.okx.payments.router.adapters.MppAdapter;

@Configuration
class MppSessionConfig {

    @Bean MppRouteConfig mppRoutes() {
        SessionMethodDetails sd = new SessionMethodDetails(
                196L,
                "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
                null, "0", null);
        String payee = env("MPP_PAYEE_ADDRESS");

        // Same-chain, multi-token offering — buyer picks one when opening the channel.
        List<MppRouteConfig.Option> priceOpts = List.of(
            MppRouteConfig.Option.of(BigInteger.ONE,             USDC_XLAYER, payee, sd),
            MppRouteConfig.Option.of(BigInteger.valueOf(2),      USDT_XLAYER, payee, sd),
            MppRouteConfig.Option.of(BigInteger.valueOf(50_000), OKB_XLAYER,  payee, sd));

        // Single-token shortcut.
        List<MppRouteConfig.Option> usdcOnly = List.of(
            MppRouteConfig.Option.of(BigInteger.ONE, USDC_XLAYER, payee, sd));

        return MppRouteConfig.builder()
            .sessionManage("/session/manage",                    "dex.market", usdcOnly)
            .resource("/api/v5/dex/market/price",                "dex.market", priceOpts)
            .resource("/api/v5/dex/market/historical-price",     "dex.market", usdcOnly)
            .resource("/api/v5/dex/market/candles",              "dex.market", usdcOnly)
            .resource("/api/v5/dex/market/trades",               "dex.market", usdcOnly);
    }

    @Bean PaymentRouterFilter paymentRouterFilter(MppServer server, MppRouteConfig routes) {
        MppAdapter mpp = new MppAdapter(server);
        PaymentRouterConfig.Builder cfg = PaymentRouterConfig.builder().protocol(mpp);
        for (var e : routes.entries().entrySet()) {
            cfg.route(e.getKey(), RouteConfig.of().with("mpp", e.getValue()));
        }
        return new PaymentRouterFilter(cfg.build());
    }

    @Bean FilterRegistrationBean<PaymentRouterFilter> register(PaymentRouterFilter f) {
        FilterRegistrationBean<PaymentRouterFilter> r = new FilterRegistrationBean<>(f);
        r.addUrlPatterns("/*");
        r.setOrder(20);
        return r;
    }
}
```

What the filter does, by route kind:

| `MppRouteConfig.Kind` | Filter behaviour |
|---|---|
| `SESSION_MANAGE` | Decodes `Authorization: Payment`, calls `evmSessionMethod().verifySession` which auto-routes by `payload.action` (open / topUp / voucher / close), writes JSON body. Terminal — `chain.doFilter` NOT called. |
| `RESOURCE` | Resolves `channelId` from `X-Channel-Id`; runs `chain.doFilter`; if response `< 400`, calls `deductFromChannel(channelId, routePrice)` and injects `X-Spent` / `X-Units` / `Payment-Receipt`. On 70015 (insufficient), resets the response and emits 402 + `WWW-Authenticate`. |

> Voucher submission charges the route's `BigInteger.ONE` price. Set the
> `/session/manage` route price to `0` to make voucher upgrade free.

Business handlers stay payment-free:

```java
@RestController
class DexMarketController {
    @GetMapping("/api/v5/dex/market/price")
    public Map<String,Object> price(@RequestParam String token) {
        return Map.of("token", token, "price", lookupPrice(token));
    }
    // ... etc
}
```

## 4. `session` intent — Spring MVC `HandlerInterceptor` (alternative)

If your stack already wires interceptors instead of filters, swap
`PaymentRouterFilter` for `MppPaymentInterceptor`:

```java
@Bean MppPaymentInterceptor mppPaymentInterceptor(MppServer server, MppRouteConfig routes) {
    return MppPaymentInterceptor.create(server, routes)
        .onProtectedRequest((req, entry) -> {
            if (Boolean.TRUE.equals(req.getAttribute("paymentExempt"))) {
                return ProtectedRequestResult.grantAccess();
            }
            return ProtectedRequestResult.proceed();
        });
}

@Configuration
class MppMvc implements WebMvcConfigurer {
    @Autowired MppPaymentInterceptor mpp;
    @Override public void addInterceptors(InterceptorRegistry r) {
        r.addInterceptor(mpp).addPathPatterns("/api/v5/dex/market/**", "/session/manage");
    }
}
```

Spring MVC interceptors only run on URLs with a registered handler — add
a placeholder `@RequestMapping("/session/manage")` controller so the
interceptor's `preHandle` is reached.

## 5. Channel state model

Each MPP channel carries five amount fields:

| Field | What it means | When it changes |
|---|---|---|
| `deposit` | Amount escrowed on-chain — the hard cap | `/session/open` and `/session/topUp`; never shrinks |
| `lastAccepted` | Highest cumulative voucher amount the seller has verified and stored off-chain | `acceptVoucher()` after EIP-712 verify + monotonic + min-delta + CAS |
| `spent` | Amount the seller has billed (consumed) from accepted vouchers | each successful `deductFromChannel(amount)` |
| `settledOnChain` | Amount the seller has pushed on-chain via `/session/settle` | successful `settle(channelId, cum)` |
| `units` | Count of successful deducts — telemetry only | grows by 1 per successful deduct |

Invariants (SDK-enforced; custom `SessionStore` must preserve):

```
spent          ≤  lastAccepted   ≤  deposit
settledOnChain ≤  lastAccepted
available      =  lastAccepted - spent
```

When `available < amount` on `deductFromChannel`, SDK throws
`InsufficientBalanceError` (code 70015). Buyer recovers by signing a
higher cumulative voucher and re-submitting.

## 6. Persistence (production)

The default `InMemorySessionStore` is dev-only. For multi-instance prod,
implement `SessionStore` against a backend with atomic compare-and-set.
The MySQL pattern is the canonical reference:

```sql
CREATE TABLE mpp_channel (
    channel_id              VARCHAR(66)   NOT NULL PRIMARY KEY,
    payer                   VARCHAR(42)   NOT NULL,
    payee                   VARCHAR(42)   NOT NULL,
    token                   VARCHAR(42)   NOT NULL,
    escrow                  VARCHAR(42)   NOT NULL,
    chain_id                BIGINT        NOT NULL,
    authorized_signer       VARCHAR(42)   NOT NULL,
    deposit                 DECIMAL(78,0) NOT NULL DEFAULT 0,
    last_accepted           DECIMAL(78,0) NOT NULL DEFAULT 0,
    last_voucher_signature  VARBINARY(65) NULL,
    settled_on_chain        DECIMAL(78,0) NOT NULL DEFAULT 0,
    status                  ENUM('OPEN','CLOSING','CLOSED') NOT NULL DEFAULT 'OPEN',
    spent                   DECIMAL(78,0) NOT NULL DEFAULT 0,
    units                   BIGINT        NOT NULL DEFAULT 0,
    updated_at              TIMESTAMP     NOT NULL
                            DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_payer  (payer),
    INDEX idx_status (status),
    CONSTRAINT chk_spent_le_accepted   CHECK (spent <= last_accepted),
    CONSTRAINT chk_settled_le_accepted CHECK (settled_on_chain <= last_accepted)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;
```

Two atomic methods carry the invariants; the rest are last-write-wins
JDBC:

```java
public final class MysqlSessionStore implements SessionStore {

    private final DataSource ds;
    public MysqlSessionStore(DataSource ds) { this.ds = ds; }

    @Override
    public boolean casLastAccepted(String channelId, BigInteger expected,
                                   BigInteger newAccepted, byte[] newSig) {
        try (var c = ds.getConnection();
             var ps = c.prepareStatement(
                "UPDATE mpp_channel " +
                "   SET last_accepted = ?, last_voucher_signature = ? " +
                " WHERE channel_id = ? AND last_accepted = ?")) {
            ps.setBigDecimal(1, new BigDecimal(newAccepted));
            ps.setBytes     (2, newSig);
            ps.setString    (3, channelId);
            ps.setBigDecimal(4, new BigDecimal(expected));
            return ps.executeUpdate() == 1;
        } catch (SQLException e) { throw new RuntimeException(e); }
    }

    @Override
    public DeductResult deduct(String channelId, BigInteger amount) {
        if (amount == null || amount.signum() <= 0)
            throw new IllegalArgumentException("amount must be > 0");
        try (var c = ds.getConnection()) {
            int rows;
            try (var ps = c.prepareStatement(
                    "UPDATE mpp_channel " +
                    "   SET spent = spent + ?, units = units + 1 " +
                    " WHERE channel_id = ? AND (last_accepted - spent) >= ?")) {
                ps.setBigDecimal(1, new BigDecimal(amount));
                ps.setString    (2, channelId);
                ps.setBigDecimal(3, new BigDecimal(amount));
                rows = ps.executeUpdate();
            }
            if (rows == 0) {
                Channel ch = load(channelId).orElseThrow(() ->
                    new ChannelNotFoundError("channel not found: " + channelId));
                BigInteger available = ch.lastAccepted().subtract(ch.spent());
                throw new InsufficientBalanceError(
                    "insufficient: requested " + amount + " available " + available);
            }
            try (var ps = c.prepareStatement(
                    "SELECT spent, units FROM mpp_channel WHERE channel_id = ?")) {
                ps.setString(1, channelId);
                try (var rs = ps.executeQuery()) { rs.next();
                    return new DeductResult(rs.getBigDecimal(1).toBigInteger(), rs.getLong(2));
                }
            }
        } catch (SQLException e) { throw new RuntimeException(e); }
    }
    // load / put / updateDeposit / updateSettledOnChain / markStatus — JDBC
}
```

Wire it in:

```java
@Bean SessionStore sessionStore(DataSource ds) { return new MysqlSessionStore(ds); }

@Bean MppServer mppServer(SessionStore store, /* ... */) {
    return MppServer.builder()
        .sessionStore(store)
        .saApiConfig(/* ... */)
        .challengeSecretKey(/* ... */)
        .payeeAuthSigner(/* ... */)
        .build();
}
```

Redis Lua sketch (one alternative):

```lua
-- KEYS[1] = channel:{channelId}; ARGV[1] = amount
local data = redis.call('HMGET', KEYS[1], 'lastAccepted', 'spent', 'units')
if not data[1] then return {-1} end                                -- channel not found
local available = tonumber(data[1]) - tonumber(data[2])
if available < tonumber(ARGV[1]) then return {0, available} end    -- 70015
local newSpent = tonumber(data[2]) + tonumber(ARGV[1])
local newUnits = tonumber(data[3]) + 1
redis.call('HMSET', KEYS[1], 'spent', newSpent, 'units', newUnits)
return {1, newSpent, newUnits}
```

## 7. 70015 → 402 recovery (already wired)

The filter does this for you on every `RESOURCE` route; merchants do not
write recovery code. The buyer-visible exchange:

```
1. GET /api/.../trades                             (route price = 20)
2. (filter) deductFromChannel(channelId, 20) → InsufficientBalanceError
3. 402 + WWW-Authenticate: Payment …
   {"type":".../insufficient-balance","status":402,
    "detail":"requested 20, available 5"}

   Buyer signs new voucher cum = N+50 and submits:

4. POST /session/manage   action=voucher, cum=N+50
5. 200 {acceptedCum=N+50, spent, units}

6. GET /api/.../trades (retry) → 200 + body + X-Spent + Payment-Receipt
```

## 8. Channel ID resolution

Default: `X-Channel-Id` header (case-insensitive). Override per
`MppAdapter`:

```java
new MppAdapter(server, MppAdapter.DEFAULT_PRIORITY,
    request -> {
        Cookie[] cookies = request.getCookies();
        if (cookies == null) return null;
        for (Cookie c : cookies)
            if ("mpp-channel".equals(c.getName())) return c.getValue();
        return null;
    });
```

## 9. Closed-channel state

`SessionHandler.close()` is fire-and-mark: submits to SA, sets local
`status=CLOSING`, returns. To observe `settledOnChain` advancing
post-close, call `mppServer.status(channelId)` — that's a read-through
to SA. The local `SessionStore` does **not** auto-refresh post-close.

---

# Dual-protocol (x402 + MPP on one URL)

Wire one `PaymentRouterFilter` with **two** adapters — `MppAdapter` +
`X402Adapter`. Adapter priorities decide who runs first
(`mpp=10, x402=20`); the router emits a single 402 with **both**
`PAYMENT-REQUIRED` (x402) and `WWW-Authenticate: Payment` (MPP) headers
when neither auth header is present, and dispatches to whichever
protocol the buyer sends back.

```java
@Bean PaymentRouterFilter paymentRouter(MppServer mppServer,
                                        OKXFacilitatorClient facilitator,
                                        MppRouteConfig mppRoutes) {
    PaymentProcessor.RouteConfig x402Candles = newX402Route("$0.0001");

    Map<String, PaymentProcessor.RouteConfig> x402Map = Map.of(
        "GET /api/v5/dex/market/candles", x402Candles,
        "/api/v5/dex/market/candles",     x402Candles);

    MppAdapter  mpp  = new MppAdapter(mppServer);
    X402Adapter x402 = new X402Adapter(facilitator, x402Map);

    PaymentRouterConfig.Builder cfg = PaymentRouterConfig.builder()
        .protocol(mpp).protocol(x402);

    // Routes mapped to a single protocol
    for (var e : mppRoutes.entries().entrySet()) {
        if (e.getKey().equals("/api/v5/dex/market/candles")) continue;
        cfg.route(e.getKey(), RouteConfig.of().with("mpp", e.getValue()));
    }
    // Shared URL — both protocols
    cfg.route("/api/v5/dex/market/candles",
        RouteConfig.of()
            .with("mpp",  mppRoutes.match("/api/v5/dex/market/candles"))
            .with("x402", x402Candles));

    return new PaymentRouterFilter(cfg.build());
}
```

## Spring MVC `HandlerInterceptor` variant

If your stack wires payments via interceptors rather than a filter, use
`DualPaymentInterceptor` — it picks one delegate per request by header
(`PAYMENT-SIGNATURE`/`X-PAYMENT` → x402; `Authorization: Payment` or
`X-Channel-Id` → MPP; otherwise the URL's MPP route table decides, else
x402) and registers as a **single** interceptor on one path pattern:

```java
@Configuration
class DualPaymentConfig implements WebMvcConfigurer {

    @Bean OKXFacilitatorClient facilitator() {
        return new OKXFacilitatorClient(env("OKX_AK"), env("OKX_SK"), env("OKX_PASSPHRASE"));
    }

    @Bean MppServer mppServer() {
        return MppServer.builder()
            .saApiConfig(SaApiConfig.builder()
                .baseUrl(env("OKX_BASE_URL"))
                .apiKey(env("OKX_AK")).secretKey(env("OKX_SK")).passphrase(env("OKX_PASSPHRASE"))
                .build())
            .challengeSecretKey(env("MPP_HMAC_SECRET").getBytes(StandardCharsets.UTF_8))
            .payeeAuthSigner(PrivateKeyPayeeAuthSigner.fromEnvVar("MPP_PAYEE_PRIVATE_KEY"))
            .build();
    }

    // x402 delegate
    @Bean PaymentInterceptor x402Interceptor(OKXFacilitatorClient facilitator,
                                             Map<String, PaymentProcessor.RouteConfig> x402Routes) {
        return PaymentInterceptor.create(facilitator, x402Routes);
    }

    // MPP delegate (same MppServer + MppRouteConfig as the filter variant)
    @Bean MppPaymentInterceptor mppInterceptor(MppServer server, MppRouteConfig mppRoutes) {
        return MppPaymentInterceptor.create(server, mppRoutes);
    }

    // One interceptor multiplexes both — no interceptor-ordering puzzle
    @Bean DualPaymentInterceptor dualInterceptor(PaymentInterceptor x402,
                                                 MppPaymentInterceptor mpp) {
        return new DualPaymentInterceptor(x402, mpp);
    }

    @Autowired DualPaymentInterceptor dual;

    @Override public void addInterceptors(InterceptorRegistry registry) {
        registry.addInterceptor(dual).addPathPatterns("/api/v5/dex/market/**", "/session/manage");
    }
}
```

Notes:
- First-touch (no auth header) defaults to **x402** unless the URL is in
  the MPP route table — so existing x402 buyers see the unchanged 402.
- `/session/manage` needs a placeholder `@RequestMapping` controller so
  Spring dispatches the interceptor to it (`preHandle` terminates the
  request, so the controller body never runs).
- Free-tier exemption works on both delegates via
  `MppPaymentInterceptor.onProtectedRequest(...)` /
  `PaymentInterceptor`'s processor hook.

Constraint: a single JVM can only load **one** servlet API namespace.
With `okxweb3-app-mpp` + `okxweb3-app-payment-router` (javax), the x402
binding must also be `okxweb3-app-x402-javax`.

---

# Reference

## Networks & assets (pre-registered)

| Chain | Network | Symbol | Contract | Decimals | EIP-712 name | Version |
|---|---|---|---|---|---|---|
| X Layer mainnet | `eip155:196` | USDT | `0x779ded0c9e1022225f8e0630b35a9b54be713736` | 6 | `USD₮0` (U+20AE) | `1` |
| X Layer mainnet | `eip155:196` | USDG | `0x4ae46a509f6b1d9056937ba4500cb143933d2dc8` | 6 | `USDG` | `2` |


The first asset registered per network is the default picked when
`RouteConfig.asset` is unset.

## Permit2 constants (`com.okx.x402.crypto.Permit2Constants`)

| Constant | Value |
|---|---|
| `PERMIT2_ADDRESS` | `0x000000000022D473030F116dDEE9F6B43aC78BA3` |
| `EXACT_PERMIT2_PROXY_ADDRESS` | `0x402085c248EeA27D92E8b30b2C58ed07f9E20001` |
| `UPTO_PERMIT2_PROXY_ADDRESS` | `0x4020e7393B728A3939659E5732F87fdd8e680002` |
| `PERMIT2_DOMAIN_NAME` | `"Permit2"` (no `version`, no `salt`) |
| `PERMIT2_DEADLINE_BUFFER_SECONDS` | `6` |
| `EXACT_WITNESS_TYPE` | `Witness(address to,uint256 validAfter)` |
| `UPTO_WITNESS_TYPE` | `Witness(address to,address facilitator,uint256 validAfter)` |

## RouteConfig defaults (x402)

| Field | Default |
|---|---|
| `scheme` | `"exact"` |
| `maxTimeoutSeconds` | `86400` (24 h) |
| `syncSettle` | `false` |
| `asyncSettle` | `false` |

## HTTP behaviour

| Property | Value |
|---|---|
| Connect timeout | 10 s |
| Request timeout | 30 s |
| Retry on HTTP 429 / OKX `50011` | yes (3 attempts; 1-2-4 s back-off) |

## Error handling contract

| Source | Throws | Meaning |
|---|---|---|
| `OKXFacilitatorClient.verify/settle/...` | `IOException` | Network error, HTTP ≥ 400, or OKX envelope `code != 0` |
| `OKXFacilitatorClient.*` | `InterruptedException` | Request interrupted |
| `OKXEvmSigner.signPaymentRequirements` | `CryptoSignException` | Missing `extra.name` / `extra.version`, invalid key, etc. |
| `PaymentProcessor.postHandle` | `IllegalStateException` | `asyncSettle=true` without `settleExecutor` |
| `MppServer.deductFromChannel` | `InsufficientBalanceError` (70015) | `available < amount` — buyer signs higher voucher |
| `MppServer.acceptVoucher` | `AmountExceedsDepositError` (70012) | voucher cum > on-chain deposit |
| `MppServer.acceptVoucher` | `InvalidPayloadError` (70003) | cum < lastAccepted (monotonicity) |
| `MppServer.acceptVoucher` | `InvalidSignatureError` | EIP-712 verify failed |
| `MppServer.close` | `BadRequestError` | `voucherSignature` required but missing for `cum > settledOnChain` |

## OKX error codes

| Code | Mapped message |
|---|---|
| `50103` | Invalid API key |
| `50104` | Invalid API key or IP |
| `50113` | Invalid passphrase |
| `50001` | Service temporarily unavailable |
| `50011` | Too many requests — retried automatically |
| `8000` | TEE operation failed |
| `10002` | x402 AA account not found |

Other codes pass through with the raw OKX `msg` string.

## Settle-timeout polling (x402, default values)

| Setting | Default |
|---|---|
| `pollInterval` | 1 s |
| `pollDeadline` | 5 s |
| `onSettlementTimeout(hook)` | none — define one to return `confirmed()` / `notConfirmed()` |

## Architecture

```
com.okx.x402                       (okxweb3-app-x402-core)
├── client/                        # HTTP clients (buyer-side; ships in the jar,
│   └── OKXHttpClient              #   not covered by this seller manual)
├── facilitator/                   # Facilitator clients
│   ├── FacilitatorClient
│   ├── OKXFacilitatorClient       # HMAC auth + envelope unwrapping
│   └── FacilitatorRouter
├── crypto/
│   ├── EvmSigner                  # signer SPI
│   ├── OKXEvmSigner               # EIP-3009 + Permit2 (web3j)
│   └── Permit2Constants / Permit2Eip712
├── model/v2 ...                   # protocol types
├── server/                        # servlet-agnostic
│   ├── PaymentProcessor
│   ├── PaymentHooks
│   ├── X402Request / X402Response
│   └── PaymentFilter / PaymentInterceptor   ← in -jakarta / -javax module
└── config/                        # AssetRegistry, AssetConfig

com.okx.payments.mpp               (mpp)
├── seller/                        # MppServer + handlers
├── server/                        # EvmChargeMethod, EvmSessionMethod, MppRouteConfig
├── protocol/                      # Challenge, Credential, Receipt, charge.*, session.*, encoding.*
├── sa/                            # SaApiClient (HMAC auth to OKX SA gateway)
└── voucher/                       # EIP-712 sign / verify, domain

com.okx.payments.router            (payment-router)
├── PaymentRouterConfig / PaymentRouterFilter
├── RouteConfig
├── adapters/MppAdapter / adapters/X402Adapter
└── MppPaymentInterceptor / DualPaymentInterceptor   ← Spring MVC variant
```

---

# What the host service owns

| Responsibility | Notes |
|---|---|
| Secrets management | Load OKX keys, payee key, HMAC secret, client private keys from KMS / Vault. SDK does not call `System.getenv` itself. |
| Thread pool for `asyncSettle` | SDK throws `IllegalStateException` if missing. |
| Persistent `SessionStore` | Default is in-memory — production multi-instance deployments must plug in §6's MySQL or equivalent. |
| Observability | SDK logs INFO on happy paths and WARN on unmapped SA codes. Use lifecycle hooks + metrics for the rest. |
| Rate-limiting / DDoS | The facilitator has its own limits (`50011`), but 402 generation is free; protect endpoints. |
| Nonce / replay storage | EIP-3009 nonces live in the token contract; MPP nonces in the escrow contract — no SDK-owned DB. |
| Route inventory | Keep `Map<String, RouteConfig>` / `MppRouteConfig` synced with deployed endpoints. |
| Final-state polling for closed channels | Call `mppServer.status(channelId)` (queries SA). The local `SessionStore` does not refresh post-close. |

---

# Common pitfalls

| Pitfall | Symptom | Fix |
|---|---|---|
| `sendError(...)` on a paid x402 route | Settlement happens but `PAYMENT-RESPONSE` header missing | Use `setStatus + getWriter().write(...)` |
| Async / streaming I/O on a paid route | Wrapper drops writes | Don't mark streaming routes as paid |
| Forgot to register the route in `MppRouteConfig` | Buyer hits resource, no billing | Add the path to `resource(...)` |
| Resolved `channelId` from URL or query | Cross-tenant billing leak | Use `X-Channel-Id` header or signed cookie |
| `MPP_PAYEE_PRIVATE_KEY` matches buyer EOA | Settle reverts on-chain | Merchant payee must be distinct |
| Multi-instance with `InMemorySessionStore` | Vouchers lost on restart / split-brain | Use the MySQL store from §6 |
| `cum > deposit` voucher | 70012 returned | Buyer calls topUp first, or keeps cum ≤ deposit |
| `minVoucherDelta = 0` in production | Trivial spam | Pick a floor (e.g. 10× smallest route price) |
| Mixing javax-mpp with jakarta-x402 in one JVM | `NoClassDefFoundError: jakarta.servlet.Filter` (or javax) | Pick one servlet namespace |
| Polling local store after close | `status=CLOSING` forever, `settledOnChain=0` | Use `mppServer.status(channelId)` |
| Did not rotate `MPP_HMAC_SECRET` | Old leaked secrets still issue valid challenges | Rotate via secrets manager |

---

# Protocol reference

- x402 v2: <https://www.x402.org/>
- OKX facilitator: `POST /api/v6/pay/x402/{verify,settle,supported}`, `GET /api/v6/pay/x402/settle/status`
- OKX MPP / SA: `POST /api/v6/pay/mpp/session/{open,topUp,settle,close}`, `GET /api/v6/pay/mpp/session/status`, `POST /api/v6/pay/mpp/charge/{settle,verifyHash}`
- EIP-3009 — `transferWithAuthorization`
- EIP-712 — typed structured data signing
- Permit2 — `PermitWitnessTransferFrom`
- CAIP-2 — chain-agnostic network IDs (`eip155:196`)
