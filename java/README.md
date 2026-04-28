<!-- Reviewed: AI-assisted drafting + human review completed. -->
> **Status:** Reviewed — passed AI & human review.

# OKX x402 Java SDK

[![Java Version](https://img.shields.io/badge/java-17%2B-orange)](https://github.com/okx/payments-sdk/java)

Java implementation of the [x402](https://www.x402.org/) payment protocol with OKX X Layer support.

Supports two payment schemes:
- **`exact`** — Standard EIP-3009 per-request payment, immediate on-chain settlement
- **`aggr_deferred`** — Session key signing, OKX Facilitator batches N payments into 1 on-chain transaction via TEE

## Quick Start

```bash
mvn clean install    # Build and test
mvn test             # Run the default unit + E2E suite (RealFacilitatorIT excluded)
```

## Installation

Pick the adapter that matches your servlet namespace:

**Jakarta EE 9+ / Spring Boot 3:**

```xml
<dependency>
    <groupId>com.okx</groupId>
    <artifactId>x402-java-jakarta</artifactId>
    <version>1.0.0</version>
</dependency>
```

**Java EE 8 / Spring Boot 2:**

```xml
<dependency>
    <groupId>com.okx</groupId>
    <artifactId>x402-java-javax</artifactId>
    <version>1.0.0</version>
</dependency>
```

Non-servlet frameworks can depend on `com.okx:x402-java-core` and
implement `X402Request` / `X402Response` against their own types — see
[INTEGRATION.md](INTEGRATION.md) §2.2.

## Server Side — Requiring Payments

### Basic Setup (exact scheme)

```java
import com.okx.x402.facilitator.FacilitatorRouter;
import com.okx.x402.facilitator.OKXFacilitatorClient;
import com.okx.x402.server.PaymentFilter;

// 1. Create facilitator client (HMAC-SHA256 auth is automatic)
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
        System.getenv("OKX_API_KEY"),
        System.getenv("OKX_SECRET_KEY"),
        System.getenv("OKX_PASSPHRASE"));

// 2. Configure route pricing
PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
route.scheme = "exact";
route.network = "eip155:196";           // X Layer
route.payTo = "0xYourWalletAddress";
route.price = "$0.01";                  // Auto-resolves to USDT atomic units

// 3. One-line middleware
PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /api/data", route
));

// 4. Register as servlet filter (Spring Boot, Jetty, Tomcat, etc.)
```

### Both Schemes (exact + aggr_deferred)

```java
// Accept both exact and aggr_deferred on different endpoints
PaymentProcessor.RouteConfig exactRoute = new PaymentProcessor.RouteConfig();
exactRoute.scheme = "exact";
exactRoute.network = "eip155:196";
exactRoute.payTo = payTo;
exactRoute.price = "$0.001";

PaymentProcessor.RouteConfig deferredRoute = new PaymentProcessor.RouteConfig();
deferredRoute.scheme = "aggr_deferred";
deferredRoute.network = "eip155:196";
deferredRoute.payTo = payTo;
deferredRoute.price = "$0.001";

PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /api/standard", exactRoute,
        "GET /api/agent",    deferredRoute
));
```

### Multi-currency on a single endpoint (USDT + USDG)

Populate `RouteConfig.accepts` with a list of `AcceptOption` — every
option becomes one entry in the 402 `accepts` envelope and the client
picks which one to pay. Mirrors the `PaymentOptions` model of the
Go / TypeScript SDKs. See [CONFIG.md §B1.1](CONFIG.md) for full semantics.

```java
import com.okx.x402.server.AcceptOption;

PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
route.network = "eip155:196";           // shared defaults
route.payTo   = payTo;
route.accepts = List.of(
    AcceptOption.builder()
        .scheme("exact").price("$0.01")           // default asset (USDT)
        .build(),
    AcceptOption.builder()
        .scheme("exact").price("$0.01")
        .asset("0x4ae46a509f6b1d9056937ba4500cb143933d2dc8")   // USDG
        .build()
);

PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /api/data", route
));
```

On the client, inject a custom selector to prefer a specific token:

```java
OKXHttpClientConfig cfg = new OKXHttpClientConfig(signer);
cfg.paymentRequirementsSelector = (v, accepts) -> accepts.stream()
        .filter(r -> "0x4ae46a509f6b1d9056937ba4500cb143933d2dc8".equalsIgnoreCase(r.asset))
        .findFirst()
        .orElse(accepts.get(0));
OKXHttpClient client = new OKXHttpClient(cfg);
```

### FacilitatorRouter (multi-network)

```java
// Route X Layer to OKX, others to default
FacilitatorClient facilitator = FacilitatorRouter.builder()
        .okx(apiKey, secretKey, passphrase)    // auto-routes eip155:196 + eip155:195
        .defaultFacilitator(cdpClient)         // fallback for other networks
        .build();
```

### Skip Payment / Early Reject (`onProtectedRequest`)

The HTTP-layer hook that runs after route match, before the payment header is read. Lets the application grant access (e.g. for an API-key tier) or reject a request outright with a reason:

```java
PaymentFilter filter = PaymentFilter.create(facilitator, Map.of("GET /api/data", route));
filter.processor().onProtectedRequest((request, routeConfig) -> {
    if ("internal".equals(request.getHeader("x-api-key"))) {
        return PaymentHooks.ProtectedRequestResult.grantAccess();  // skip payment
    }
    if (rateLimiter.isThrottled(request)) {
        return PaymentHooks.ProtectedRequestResult.abort("rate_limited");  // HTTP 403
    }
    return PaymentHooks.ProtectedRequestResult.proceed();  // normal payment flow
});
```

Multiple hooks are supported; they run in registration order and the first hook returning `grantAccess()` or `abort(reason)` wins. `abort` responds with HTTP 403 and a `{"error":"<reason>"}` body — it does not use the 402 `PAYMENT-REQUIRED` envelope.

See [INTEGRATION.md](INTEGRATION.md) for the complete billing interceptor integration guide.

### Async Settlement

Settle in a background thread — return 200 immediately after verify, without waiting for on-chain settlement:

```java
PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
route.network = "eip155:196";
route.payTo = "0xYourWalletAddress";
route.price = "$0.01";
route.asyncSettle = true;  // enable async settle

PaymentFilter filter = PaymentFilter.create(facilitator, Map.of("GET /api/data", route));
filter.processor()
    .settleExecutor(bizThreadPool)  // required — must inject your own thread pool
    .onAsyncSettleComplete((payload, requirements, result, error) -> {
        if (error != null) {
            log.error("Settlement failed", error);
        } else {
            log.info("Settlement tx={}", result.transaction);
        }
    });
```

### Spring Boot Integration

```java
@SpringBootApplication
public class App implements ServletContextInitializer {
    @Override
    public void onStartup(ServletContext ctx) {
        OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
                System.getenv("OKX_API_KEY"),
                System.getenv("OKX_SECRET_KEY"),
                System.getenv("OKX_PASSPHRASE"));

        PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
        route.network = "eip155:196";
        route.payTo = System.getenv("PAY_TO_ADDRESS");
        route.price = "$0.01";

        FilterRegistration.Dynamic filter = ctx.addFilter(
                "x402", PaymentFilter.create(facilitator, Map.of(
                        "GET /api/data", route)));
        filter.addMappingForUrlPatterns(null, false, "/api/*");
    }
}
```

### Server-Side Limitations (read before going to prod)

`PaymentFilter` wraps the response in an in-memory `BufferedHttpServletResponse`
so settlement can attach the `PAYMENT-RESPONSE` header **after** your handler
returns. Three integration constraints follow from that wrapping; pick a
handler style that respects them.

1. **Do not use `response.sendError(...)` on a paid route.**
   `HttpServletResponseWrapper.sendError(...)` forwards directly to the
   underlying response and commits it before settlement can run. The flow
   stays safe — `postHandle` short-circuits on status ≥ 400 and no settle is
   attempted — but the buyer client will not see a settlement-proof header
   even if the path was already paid for. Surface business errors via
   `setStatus(...) + getWriter().write(...)` (or your framework's exception
   handler that does the same), not `sendError`.

2. **Async / non-blocking I/O is not supported on paid routes.**
   The buffered `ServletOutputStream` is synchronous; a handler that opts
   into Servlet 3 non-blocking I/O via `setWriteListener(...)` will not
   receive callbacks. The wrapper is designed for the synchronous
   filter → handler → postHandle flow only. Streaming responses
   (`StreamingResponseBody`, server-sent events, chunked transfers larger
   than fits in memory) should not be marked as paid routes.

3. **`@RestController` with `PaymentInterceptor` drops the proof header.**
   For the Spring `HandlerInterceptor` adapter (`PaymentInterceptor`),
   Spring's message converter writes the response body during
   `RequestMappingHandlerAdapter.handle(...)`, which commits the response
   before `postHandle` runs. Settlement still happens, but `PAYMENT-RESPONSE`
   is silently dropped (servlet spec: `setHeader` on a committed response is
   a no-op). If you need the proof header on a `@ResponseBody` /
   `@RestController` flow, use `PaymentFilter` instead — it can be
   registered via `FilterRegistrationBean` alongside Spring's interceptor
   stack without losing ordering control.

## Client Side — Making Paid Requests

### Auto-402 Client (exact scheme)

```java
import com.okx.x402.client.OKXHttpClient;
import com.okx.x402.crypto.OKXEvmSigner;

// 1. Create signer from private key
OKXEvmSigner signer = new OKXEvmSigner(System.getenv("PRIVATE_KEY"));

// 2. Create client with auto-402 handling
OKXHttpClient client = new OKXHttpClient(signer, "eip155:196");

// 3. GET — auto-handles 402 -> sign -> retry -> 200
HttpResponse<String> resp = client.get(
        URI.create("https://api.example.com/api/data"));

System.out.println(resp.statusCode());  // 200
System.out.println(resp.body());        // business data

// 4. Settlement proof in PAYMENT-RESPONSE header
String proof = resp.headers().firstValue("PAYMENT-RESPONSE").orElse(null);
```

## Facilitator Client API

The `FacilitatorClient` interface wraps the OKX `/api/v6/pay/x402` endpoints:

```java
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
        apiKey, secretKey, passphrase);

// Or with a custom base URL (e.g. a non-production facilitator endpoint
// supplied by your OKX contact for integration testing)
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
        apiKey, secretKey, passphrase,
        System.getenv("OKX_FACILITATOR_BASE_URL"));

// Or with full config — tune timeouts, inject a shared HttpClient, or plug
// in OkHttp / Apache HttpClient / Reactor Netty via the HttpExecutor SPI
OKXFacilitatorConfig cfg = new OKXFacilitatorConfig(apiKey, secretKey, passphrase);
cfg.requestTimeout = Duration.ofSeconds(60);
cfg.httpClient   = mySharedHttpClient;             // JDK client shortcut, optional
cfg.httpExecutor = new OkHttpExecutor(myOkHttp);   // any HTTP stack, optional
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(cfg);
```

See `CONFIG.md §A2.1` for the full `OKXFacilitatorConfig` field reference and
`§A2.2` for the OkHttp adapter recipe. The buyer client has a matching
`OKXHttpClientConfig` — see `CONFIG.md §B7.1`.

### verify

```java
VerifyResponse vr = facilitator.verify(payload, requirements);
// vr.isValid, vr.payer, vr.invalidReason, vr.invalidMessage
```

### settle

```java
// Async (default) — returns immediately with status="pending"
SettleResponse sr = facilitator.settle(payload, requirements);

// Sync — waits for on-chain confirmation
SettleResponse sr = facilitator.settle(payload, requirements, true);
// sr.success, sr.transaction, sr.status ("success"/"pending"/"timeout")
```

### settleStatus

```java
// Poll settlement status by transaction hash
SettleResponse sr = facilitator.settleStatus("0xTxHash...");
// sr.success, sr.status ("pending"/"success"/"failed")
```

### supported

```java
SupportedResponse sr = facilitator.supported();
// sr.kinds = [{scheme:"exact", network:"eip155:196"}, {scheme:"aggr_deferred", ...}]
```

## Payment Schemes

### exact

Standard x402 per-request payment. Buyer signs EIP-3009 `TransferWithAuthorization` with their EOA private key. Facilitator submits on-chain immediately.

```
Buyer signs with EOA key → Seller receives EOA signature → Facilitator submits on-chain
```

| Field | Value |
|-------|-------|
| `scheme` | `"exact"` |
| `authorization.from` | Buyer's EOA address |
| `authorization.validBefore` | `now + maxTimeoutSeconds` |
| Settle response `transaction` | Real tx hash |
| Settle response `status` | `"success"` / `"pending"` / `"timeout"` |

### aggr_deferred

Session key based payment for AI Agent high-frequency scenarios. Buyer signs with a session key (not EOA). OKX Facilitator TEE converts to EOA signature and batches N payments into 1 on-chain transaction.

```
Buyer signs with session key → Seller passes to Facilitator → TEE converts → Batch on-chain
```

| Field | Value |
|-------|-------|
| `scheme` | `"aggr_deferred"` |
| `authorization.from` | AA wallet address (not session key address) |
| `authorization.validBefore` | `uint256.max` (no expiry) |
| `accepted.extra.sessionCert` | Base64-encoded session certificate |
| Settle response `transaction` | `""` (empty — TEE batches later) |
| Settle response `status` | `"success"` (means accepted into batch) |

**Requirements:**
- Buyer must have a registered x402 AA (Account Abstraction) account
- Session certificate issued by OKX Wallet TEE binding session key to AA account
- Session private key for signing (not EOA key)

## Architecture

```
com.okx.x402
├── client/                    # HTTP clients (buyer side)
│   ├── OKXHttpClient          — V2 auto-402 client
│   ├── HttpFacilitatorClient  — V1 compat (Coinbase CDP)
│   └── X402HttpClient         — V1 compat
├── facilitator/               # Facilitator client implementations
│   ├── FacilitatorClient      — V2 interface (verify, settle, settleStatus, supported)
│   ├── OKXFacilitatorClient   — OKX /api/v6 with HMAC auth + envelope unwrapping
│   └── FacilitatorRouter      — Network-based routing
├── crypto/                    # Signing
│   ├── EvmSigner              — V2 EVM signer interface
│   ├── OKXEvmSigner           — EIP-3009 + EIP-712 signing (web3j)
│   ├── OKXSignerFactory       — Factory with config builder
│   └── CryptoSigner           — V1 compat interface
├── model/
│   ├── v2/                    — V2 protocol types (PaymentPayload, SettleRequest, etc.)
│   └── v1/                    — V1 compat types
├── server/                   (core)
│   ├── PaymentProcessor       — servlet-agnostic verify / settle / hook / async-settle logic
│   ├── PaymentHooks           — Lifecycle hook interfaces (before/after verify & settle)
│   ├── X402Request            — servlet-agnostic request view (adapter SPI)
│   └── X402Response           — servlet-agnostic response view (adapter SPI)
│
│   (jakarta module — x402-java-jakarta)
│   ├── PaymentFilter          — jakarta.servlet.Filter adapter
│   └── PaymentInterceptor     — Spring 6 HandlerInterceptor adapter
│
│   (javax module — x402-java-javax)
│   ├── PaymentFilter          — javax.servlet.Filter adapter
│   └── PaymentInterceptor     — Spring 5 HandlerInterceptor adapter
├── config/
│   ├── AssetRegistry          — X Layer USDT pre-registered, extensible
│   └── AssetConfig            — Token metadata (address, decimals, EIP-712 domain)
└── util/
    ├── Json                   — Shared Jackson ObjectMapper
    └── OKXAuth                — HMAC-SHA256 header generation
```

## Supported Networks & Assets

| Chain | Network ID | Token | Contract | Decimals | EIP-712 Name |
|-------|-----------|-------|----------|----------|-------------|
| X Layer | `eip155:196` | USDT | `0x779ded0c9e1022225f8e0630b35a9b54be713736` | 6 | `USD₮0` (U+20AE) |
| X Layer Testnet | `eip155:195` | USDT | TBD | 6 | `USD₮0` |

Register custom assets:

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

## Payment Flow

### exact scheme

```
Client                     Server                   OKX Facilitator
  │── GET /api/data ──────>│                           │
  │<── 402 + accepts ──────│                           │
  │                        │                           │
  │  (EIP-3009 sign)       │                           │
  │                        │                           │
  │── GET + PAYMENT-SIG ──>│                           │
  │                        │── POST /verify ──────────>│
  │                        │<── isValid: true ─────────│
  │                        │                           │
  │                        │  (run business handler)   │
  │                        │                           │
  │                        │── POST /settle ──────────>│
  │                        │<── success + txHash ──────│
  │<── 200 + data ─────────│                           │
  │  + PAYMENT-RESPONSE    │                           │
```

### aggr_deferred scheme

```
AI Agent (AA wallet)       Server                   OKX Facilitator + TEE
  │── GET /api/agent ─────>│                           │
  │<── 402 + accepts ──────│                           │
  │                        │                           │
  │  (session key sign     │                           │
  │   + sessionCert)       │                           │
  │                        │                           │
  │── GET + PAYMENT-SIG ──>│                           │
  │                        │── POST /verify ──────────>│
  │                        │  TEE: getPubKey(cert) ───>│
  │                        │  ecrecover == pubKey? ───>│
  │                        │<── isValid: true ─────────│
  │                        │                           │
  │                        │  (run business handler)   │
  │                        │                           │
  │                        │── POST /settle ──────────>│
  │                        │  TEE: convert session sig >│
  │                        │  → EOA sig → store batch  │
  │                        │<── success, tx="" ────────│
  │<── 200 + data ─────────│                           │
  │                        │                           │
  │                        │  (later: batch job)       │
  │                        │  TEE: compress N→1 ──────>│ Chain
```

## OKX API Response Envelope

The OKX facilitator wraps responses in `{"code":0,"data":{...}}`. The SDK auto-unwraps this — you always get the inner `data` object. Non-zero `code` is treated as an error.

## Error Handling

| Error Code | Meaning |
|-----------|---------|
| `50103` | Invalid API key |
| `50104` | Invalid API key or IP |
| `50113` | Invalid passphrase |
| `50001` | Service temporarily unavailable |
| `50011` | Too many requests (rate limit) |
| `8000` | TEE operation failed |
| `10002` | x402 AA account not found |

```java
try {
    VerifyResponse vr = facilitator.verify(payload, requirements);
} catch (IOException e) {
    // e.getMessage() contains mapped error: "OKX API error on /verify (code=50103): Invalid API key"
}
```

## Testing

### Default suite (WireMock-based, no network)

```bash
mvn test
```

Runs all classes matching surefire defaults (`**/*Test.java`). `RealFacilitatorIT.java` does not match the default include and is skipped.

### Integration Tests Against a Real Facilitator

```bash
# Force-include the IT class (hits the configured OKX endpoint)
# RealFacilitatorIT reads OKX_API_KEY / OKX_SECRET_KEY / OKX_PASSPHRASE
# and OKX_FACILITATOR_BASE_URL from the environment.
mvn test -Dtest=RealFacilitatorIT
```

### Test Coverage

| Suite | Test class(es) | @Test methods | Environment |
|-------|-----------------|---------------|-------------|
| Core unit | 14 classes in `core/src/test/java` (excl. `integration/`) | 82 | Local / WireMock |
| Jakarta unit | `PaymentFilterV2Test` | 23 | Mockito |
| Jakarta E2E | `E2EPaymentFlowTest` | 27 | Embedded Jetty + WireMock |
| **Default `mvn test`** | | **132** | |
| Real facilitator IT | `RealFacilitatorIT` | 13 | OKX-hosted facilitator |
| **Grand total** | | **145** | |

### Integration Test Results

**exact scheme** (run against an OKX-hosted non-production facilitator):

| Test | Result |
|------|--------|
| supported → 2 kinds | PASS |
| verify (valid signature) | PASS — isValid=true |
| verify (bad signature) | PASS — isValid=false |
| settle (syncSettle=true) | PASS — success, txHash returned |
| settle (syncSettle=false) | PASS — success, status=pending |
| settleStatus (real tx) | PASS — status=success |
| settleStatus (not found) | PASS — reason=not_found |

**aggr_deferred scheme** (run against `web3.okx.com`):

| Test | Result |
|------|--------|
| supported → includes aggr_deferred | PASS |
| verify (AA account found) | PASS — payer=AA address |
| settle (single tx) | PASS — reaches TEE layer |
| settle (multi-tx ×3) | PASS — consistent behavior |
| settleStatus | PASS |

## Examples

See [`examples/`](examples/) for complete demo code:
- `DemoServer.java` — standalone embedded Jetty server with `PaymentFilter` wired in
- `DemoClient.java` — auto-402 client, signs via `OKXSignerFactory.createOKXSigner(...)`

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `OKX_API_KEY` | Server | OKX API key for facilitator auth |
| `OKX_SECRET_KEY` | Server | OKX secret key (HMAC-SHA256) |
| `OKX_PASSPHRASE` | Server | OKX API passphrase |
| `PAY_TO_ADDRESS` | Server | Seller wallet address on X Layer |
| `PRIVATE_KEY` | Client | 0x-prefixed hex private key (exact scheme) |

## Protocol Reference

- [x402 Specification V2](https://www.x402.org/)
- OKX Facilitator API: `POST /api/v6/pay/x402/{verify,settle,supported}`, `GET /api/v6/pay/x402/settle/status`
- EIP-3009: Transfer With Authorization
- EIP-712: Typed Structured Data Signing
- CAIP-2: Chain Agnostic Network IDs (`eip155:196`)
