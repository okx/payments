# OKX x402 Java SDK

[![Java Version](https://img.shields.io/badge/java-17%2B-orange)](https://github.com/okx/payments-sdk/java)

Java implementation of the [x402](https://www.x402.org/) payment protocol with OKX X Layer support.

Supports two payment schemes:
- **`exact`** — Standard EIP-3009 per-request payment, immediate on-chain settlement
- **`aggr_deferred`** — Session key signing, OKX Facilitator batches N payments into 1 on-chain transaction via TEE

## Quick Start

```bash
mvn clean install    # Build and test
mvn test             # Run 113 tests (unit + integration)
```

## Installation

```xml
<dependency>
    <groupId>com.okx</groupId>
    <artifactId>x402-java</artifactId>
    <version>1.0.0-SNAPSHOT</version>
</dependency>
```

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
PaymentFilter.RouteConfig route = new PaymentFilter.RouteConfig();
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
PaymentFilter.RouteConfig exactRoute = new PaymentFilter.RouteConfig();
exactRoute.scheme = "exact";
exactRoute.network = "eip155:196";
exactRoute.payTo = payTo;
exactRoute.price = "$0.001";

PaymentFilter.RouteConfig deferredRoute = new PaymentFilter.RouteConfig();
deferredRoute.scheme = "aggr_deferred";
deferredRoute.network = "eip155:196";
deferredRoute.payTo = payTo;
deferredRoute.price = "$0.001";

PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /api/standard", exactRoute,
        "GET /api/agent",    deferredRoute
));
```

### FacilitatorRouter (multi-network)

```java
// Route X Layer to OKX, others to default
FacilitatorClient facilitator = FacilitatorRouter.builder()
        .okx(apiKey, secretKey, passphrase)    // auto-routes eip155:196 + eip155:195
        .defaultFacilitator(cdpClient)         // fallback for other networks
        .build();
```

### Skip Payment (Billing Interceptor Integration)

When an upstream billing interceptor determines a request is free, it sets a request attribute:

```java
// Approach 1: Default attribute convention (zero coupling)
// Your billing interceptor sets this before PaymentFilter/PaymentInterceptor runs:
request.setAttribute(PaymentProcessor.PAYMENT_EXEMPT_ATTRIBUTE, true);  // "paymentExempt"

// Approach 2: Custom predicate
PaymentFilter filter = PaymentFilter.create(facilitator, Map.of("GET /api/data", route));
filter.processor().paymentExemptWhen(req ->
    Boolean.TRUE.equals(req.getAttribute("billing.free")));
```

See [INTEGRATION.md](INTEGRATION.md) for the complete billing interceptor integration guide.

### Async Settlement

Settle in a background thread — return 200 immediately after verify, without waiting for on-chain settlement:

```java
PaymentFilter.RouteConfig route = new PaymentFilter.RouteConfig();
route.network = "eip155:196";
route.payTo = "0xYourWalletAddress";
route.price = "$0.01";
route.settleInBackground = true;  // enable background settle

PaymentFilter filter = PaymentFilter.create(facilitator, Map.of("GET /api/data", route))
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

        PaymentFilter.RouteConfig route = new PaymentFilter.RouteConfig();
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

// Or with custom base URL (e.g. for test environments)
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
        apiKey, secretKey, passphrase,
        "https://forked-okx-test2-dataasset.okx.testokg.com");
```

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
├── server/
│   ├── PaymentFilter          — V2 servlet filter (skip / verify / settle / async settle)
│   └── PaymentHooks           — Lifecycle hooks (before/after verify & settle)
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

### Unit Tests (106 tests, WireMock-based)

```bash
mvn test
```

### Integration Tests Against Real Facilitator

```bash
# Against swimlane test env (no auth required)
mvn test -Dtest=RealFacilitatorIT

# Full suite including integration
mvn test -Dtest="RealFacilitatorIT,E2EPaymentFlowTest,OKXFacilitatorClientTest,FacilitatorRouterTest,OKXEvmSignerTest,OKXSignerFactoryTest,CryptoSignExceptionTest,OKXAuthTest,AssetRegistryTest,PaymentFilterV2Test,V1ModelTest,PaymentPayloadV2Test,SharedModelTest,HttpFacilitatorClientTest,X402HttpClientTest,KindTest,OKXHttpClientTest"
```

### Test Coverage

| Suite | Tests | Environment |
|-------|-------|-------------|
| Unit (WireMock) | 106 | Local mocks |
| Integration — exact | 7 | swimlane test env |
| Integration — aggr_deferred | 5 | prod (web3.okx.com) |
| **Total** | **118** | |

### Integration Test Results

**exact scheme** (swimlane: `forked-okx-test2-dataasset.okx.testokg.com`):

| Test | Result |
|------|--------|
| supported → 2 kinds | PASS |
| verify (valid signature) | PASS — isValid=true |
| verify (bad signature) | PASS — isValid=false |
| settle (syncSettle=true) | PASS — success, txHash returned |
| settle (syncSettle=false) | PASS — success, status=pending |
| settleStatus (real tx) | PASS — status=success |
| settleStatus (not found) | PASS — reason=not_found |

**aggr_deferred scheme** (prod: `web3.okx.com`):

| Test | Result |
|------|--------|
| supported → includes aggr_deferred | PASS |
| verify (AA account found) | PASS — payer=AA address |
| settle (single tx) | PASS — reaches TEE layer |
| settle (multi-tx ×3) | PASS — consistent behavior |
| settleStatus | PASS |

## Examples

See [`examples/`](examples/) for complete demo code:
- `DemoServer.java` — Spring Boot server with payment middleware
- `DemoClient.java` — Auto-402 client with private key signing

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
