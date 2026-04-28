# OKX x402 Java SDK -- Architecture

This document describes the internal architecture of the OKX x402 Java SDK. It is written for team developers who need to understand how the SDK works, extend it, or debug it. It assumes familiarity with the x402 protocol specification and EIP-3009 (TransferWithAuthorization).

---

## Table of Contents

1. [Overview](#overview)
2. [Package Structure](#package-structure)
3. [Payment Schemes](#payment-schemes)
4. [Request Lifecycle (Server Side)](#request-lifecycle-server-side)
5. [Auto-402 Client Flow (Buyer Side)](#auto-402-client-flow-buyer-side)
6. [OKX Facilitator Client](#okx-facilitator-client)
7. [EIP-712 Signing](#eip-712-signing)
8. [Asset Registry and Price Resolution](#asset-registry-and-price-resolution)
9. [Lifecycle Hooks](#lifecycle-hooks)
10. [Header Protocol](#header-protocol)
11. [Network Routing](#network-routing)
12. [V1 Compatibility Layer](#v1-compatibility-layer)
13. [Error Handling and Retry](#error-handling-and-retry)
14. [Dependencies](#dependencies)
15. [Testing Strategy](#testing-strategy)
16. [Design Decisions and Trade-offs](#design-decisions-and-trade-offs)

---

## Overview

The SDK implements both sides of the x402 payment protocol:

- **Server side**: A `PaymentFilter` (servlet `Filter`) that enforces payment on configured routes. It returns HTTP 402 with machine-readable payment requirements, verifies incoming payment signatures via a facilitator, executes the protected business logic, and settles the payment on-chain. Shipped as two adapter modules — `x402-java-jakarta` (Jakarta EE 9+ / Spring Boot 3) and `x402-java-javax` (Java EE 8 / Spring Boot 2) — over a shared, servlet-agnostic `PaymentProcessor` in `x402-java-core`.

- **Client side**: `OKXHttpClient` transparently intercepts 402 responses, signs a payment authorization using EIP-3009, and retries the request with the `PAYMENT-SIGNATURE` header attached.

- **Facilitator layer**: `OKXFacilitatorClient` talks to OKX's `/api/v6/pay/x402` endpoints with HMAC-SHA256 authentication. `FacilitatorRouter` dispatches to different facilitator backends based on CAIP-2 network identifiers.

The SDK targets Java 17+ and uses the built-in `java.net.http.HttpClient` (no Apache HttpClient, no OkHttp). Cryptographic signing uses web3j and BouncyCastle.

---

## Package Structure

```
com.okx.x402
|
+-- client/              Buyer-side HTTP clients
|   +-- OKXHttpClient          V2 auto-402 client (intercept -> sign -> retry)
|   +-- HttpFacilitatorClient  V1 compat facilitator client (Coinbase CDP)
|   +-- X402HttpClient         V1 compat HTTP client (X-PAYMENT header)
|   +-- Kind                   V1 scheme+network pair
|   +-- VerificationResponse   V1 verify response
|   +-- SettlementResponse     V1 settle response
|
+-- facilitator/         Facilitator client implementations
|   +-- FacilitatorClient      V2 interface: verify, settle, settleStatus, supported
|   +-- OKXFacilitatorClient   OKX /api/v6 with HMAC auth + envelope unwrapping
|   +-- OKXFacilitatorConfig   Options object — timeouts, baseUrl, HTTP stack injection
|   +-- HttpExecutor           SPI for plugging in OkHttp / Apache HC / etc. (raw HTTP only)
|   +-- JdkHttpExecutor        Default HttpExecutor, wraps java.net.http.HttpClient
|   +-- FacilitatorRouter      Network-based routing, aggregates supported()
|
+-- crypto/              Signing
|   +-- CryptoSigner           V1 signer interface (sign a payload map)
|   +-- EvmSigner              V2 signer interface (extends CryptoSigner)
|   +-- OKXEvmSigner           EIP-3009 + EIP-712 signing via web3j
|   +-- OKXSignerFactory       Factory with config builder
|   +-- CryptoSignException    Checked exception for signing failures
|
+-- model/
|   +-- v2/                    V2 protocol types
|   |   +-- PaymentPayload         Sent in PAYMENT-SIGNATURE header (toHeader/fromHeader)
|   |   +-- PaymentRequired        HTTP 402 response body
|   |   +-- PaymentRequirements    One acceptable payment method
|   |   +-- ResourceInfo           URL + description + mimeType
|   |   +-- VerifyRequest          POST /verify body
|   |   +-- VerifyResponse         Verify result (isValid, payer, invalidReason)
|   |   +-- SettleRequest          POST /settle body (with syncSettle extension)
|   |   +-- SettleResponse         Settle result (txHash, status, payer)
|   |   +-- SupportedResponse      GET /supported result
|   |   +-- SupportedKind          Scheme+network+extra tuple
|   |
|   +-- v1/                    V1 compat types
|   |   +-- PaymentPayload         V1 header format (scheme, network, payload map)
|   |   +-- PaymentRequirements    V1 requirements (maxAmountRequired, resource, etc.)
|   |   +-- PaymentRequiredResponse
|   |
|   +-- Authorization          ERC-3009 auth struct (from, to, value, validAfter, validBefore, nonce)
|   +-- ExactSchemePayload     Exact scheme wrapper (signature + authorization)
|   +-- SettlementResponseHeader   PAYMENT-RESPONSE header body
|
+-- server/                    (core module — no servlet deps)
|   +-- PaymentProcessor       Servlet-agnostic verify / settle / hook / async-settle logic
|   +-- PaymentHooks           Hook interfaces and result types
|   +-- X402Request            Adapter SPI — servlet-agnostic request view
|   +-- X402Response           Adapter SPI — servlet-agnostic response view
|
|   (jakarta adapter module — x402-java-jakarta)
|   +-- PaymentFilter          jakarta.servlet.Filter adapter
|   +-- PaymentInterceptor     Spring 6 HandlerInterceptor adapter
|   +-- internal/Jakarta{Request,Response}Adapter  X402Request/Response wrappers
|
|   (javax adapter module — x402-java-javax)
|   +-- PaymentFilter          javax.servlet.Filter adapter
|   +-- PaymentInterceptor     Spring 5 HandlerInterceptor adapter
|   +-- internal/Javax{Request,Response}Adapter    X402Request/Response wrappers
|
+-- config/
|   +-- AssetRegistry          Pre-registered assets, thread-safe, extensible
|   +-- AssetConfig            Token metadata (address, decimals, EIP-712 domain)
|   +-- ResolvedPrice          Record: amount + asset + extra
|
+-- util/
    +-- Json                   Shared Jackson ObjectMapper (NON_NULL, ignore unknowns)
    +-- OKXAuth                HMAC-SHA256 header generation
```

---

## Payment Schemes

The SDK supports two payment schemes. The scheme determines how the payment is authorized, signed, and settled.

### exact

Standard EOA (Externally Owned Account) flow. The buyer holds a private key and signs an EIP-3009 `TransferWithAuthorization` message. The facilitator submits this authorization to the token contract on-chain as a single transaction.

```
Buyer                    Server (PaymentFilter)        OKX Facilitator        X Layer
  |                              |                           |                    |
  |--- GET /resource ---------->|                           |                    |
  |<-- 402 + PaymentRequired ---|                           |                    |
  |                              |                           |                    |
  | [sign EIP-3009]              |                           |                    |
  |                              |                           |                    |
  |--- GET /resource ---------->|                           |                    |
  |    + PAYMENT-SIGNATURE      |--- POST /verify --------->|                    |
  |                              |<-- isValid=true ---------|                    |
  |                              |                           |                    |
  |                              | [execute business logic]  |                    |
  |                              |                           |                    |
  |                              |--- POST /settle --------->|                    |
  |                              |                           |--- submitTx ----->|
  |                              |<-- txHash + status -------|<-- receipt -------|
  |<-- 200 + PAYMENT-RESPONSE --|                           |                    |
```

Settlement produces a real `txHash`. The `SettleResponse.status` progresses through `pending` -> `success` (or `timeout` if on-chain confirmation is slow).

### aggr_deferred

Aggregated deferred settlement. The buyer uses a session key with an associated `sessionCert` (carried in `accepted.extra`). Instead of submitting one on-chain transaction per payment, the OKX facilitator's TEE (Trusted Execution Environment) verifies the session certificate, converts the signature, and batches N payments into a single on-chain transaction.

```
Buyer (session key)      Server (PaymentFilter)        OKX Facilitator (TEE)    X Layer
  |                              |                           |                      |
  |--- GET /resource ---------->|                           |                      |
  |<-- 402 + PaymentRequired ---|                           |                      |
  |                              |                           |                      |
  | [sign with session key]      |                           |                      |
  | [attach sessionCert in       |                           |                      |
  |  accepted.extra]             |                           |                      |
  |                              |                           |                      |
  |--- GET /resource ---------->|                           |                      |
  |    + PAYMENT-SIGNATURE      |--- POST /verify --------->|                      |
  |                              |                           | [TEE verifies cert]  |
  |                              |<-- isValid=true ---------|                      |
  |                              |                           |                      |
  |                              | [execute business logic]  |                      |
  |                              |                           |                      |
  |                              |--- POST /settle --------->|                      |
  |                              |                           | [queue into batch]   |
  |                              |<-- success (no txHash) ---|                      |
  |<-- 200 + PAYMENT-RESPONSE --|                           |                      |
  |                              |                           |  [later, batch N->1] |
  |                              |                           |--- batchTx --------->|
```

The key difference: `SettleResponse` comes back with `success=true` and `status=success` immediately, but with an empty `transaction` field. The payment has been accepted into the batch queue, not yet settled on-chain. This is by design -- it reduces gas costs and latency for high-throughput use cases.

---

## Request Lifecycle (Server Side)

`PaymentFilter` is a standard Jakarta `Filter`. It intercepts HTTP requests and enforces payment on configured routes. Here is the complete lifecycle, step by step:

### 1. Route Matching

The filter checks the incoming request against its route map. Routes are keyed as either `"METHOD /path"` (e.g., `"GET /api/data"`) or `"/path"` (method-agnostic). The method-specific key is checked first, then the path-only key. If neither matches, the request passes through to business logic with no payment enforcement.

### 2. Header Detection

The filter reads the `PAYMENT-SIGNATURE` header (V2 protocol). If absent, it falls back to the `X-PAYMENT` header (V1 compatibility). If neither header is present, the filter responds with HTTP 402.

### 3. Payload Decode

The header value is base64-decoded and deserialized into a `PaymentPayload`. If decoding fails (malformed base64 or invalid JSON), the filter responds with 402 and a `"malformed payment header"` error.

### 4. Resource URL Validation

If the payload contains a `resource.url` field, the filter compares it to the actual request URL. A mismatch triggers 402 with `"resource mismatch"`. This prevents cross-route replay attacks where a payment signed for `/api/cheap` is replayed against `/api/expensive`.

### 5. Price Resolution

`buildRequirements()` resolves the price for this route. If the route has a `priceFunction` (dynamic pricing), it is called with the current `X402Request` (the servlet-agnostic view; call `request.unwrap()` to get the native `HttpServletRequest` if you need container-specific access). Otherwise, the static `price` string is used. The price string (e.g., `"$0.01"`) is converted to atomic units via `AssetRegistry.resolvePrice()`.

### 6. Before-Verify Hooks

All registered `BeforeVerifyHook` instances are called in order. Any hook can return `AbortResult.abort(reason)` to short-circuit the flow and return 402.

### 7. Verification

`facilitator.verify(payload, requirements)` is called. On success, the response contains `isValid=true` and the `payer` address. On failure, the filter returns 402 with the `invalidReason`.

If verification throws an exception, `OnVerifyFailureHook` instances are consulted. A hook can return `RecoverResult.recovered(overrideResponse)` to salvage the flow.

### 8. After-Verify Hooks

Fire-and-forget. All `AfterVerifyHook` instances are called. Their return values are ignored. Use these for logging, metrics, or audit trails.

### 9. Business Logic

`chain.doFilter()` executes the downstream servlet. The response status is checked afterward -- if >= 400, settlement is skipped entirely (you do not charge the buyer for a failed request).

### 10. Before-Settle Hooks

Same pattern as before-verify. Can abort settlement if needed (e.g., if the business logic produced a partial result).

### 11. Settlement

`facilitator.settle(payload, requirements, syncSettle)` is called. If the response has `success=false`, `OnSettleFailureHook` instances are consulted for recovery.

### 12. Timeout Recovery

If `SettleResponse.status` is `"timeout"` and a `transaction` hash is present, the filter enters recovery mode:

1. **Poll**: Call `facilitator.settleStatus(txHash)` at `pollInterval` (default 1 second) until `pollDeadline` (default 5 seconds) expires. If status becomes `"success"` or `"failed"`, use that result.
2. **Hook**: If polling is inconclusive, call the `OnSettlementTimeoutHook` (if configured). The hook receives `(txHash, network)` and returns a `SettlementTimeoutResult` -- `confirmed()` grants access, `notConfirmed()` denies. Exceptions thrown by the hook are caught, logged at `WARNING`, and treated as `notConfirmed()`.
3. **Deny**: If no hook is configured, the hook returns `notConfirmed()`, or the hook throws, respond with 402.

### 13. After-Settle Hooks and Response

After successful settlement, the filter sets the `PAYMENT-RESPONSE` header (base64-encoded `SettlementResponseHeader` with `success`, `transaction`, `network`, `payer`) and `Access-Control-Expose-Headers` for browser CORS.

---

## Auto-402 Client Flow (Buyer Side)

`OKXHttpClient` wraps `java.net.http.HttpClient` and adds transparent payment handling:

1. Send the original HTTP request (any method, any headers, any body).
2. If the response is not 402, return it as-is.
3. On 402: parse the response body as `PaymentRequired`.
4. Select a matching `PaymentRequirements` entry from the `accepts` list. The client prefers entries matching its configured CAIP-2 network (e.g., `"eip155:196"`). If none match, it falls back to the first entry.
5. Call `signer.signPaymentRequirements(selected)` to produce the EIP-3009 signature and authorization fields.
6. Build a `PaymentPayload` with `x402Version=2`, the selected requirements as `accepted`, the resource info echoed back, and the signed payload.
7. Base64-encode the payload and retry the original request with the `PAYMENT-SIGNATURE` header added. The original method, URI, headers, and body are preserved.
8. Return the retried response (expected 200).

The client does not retry more than once. If the retried request also returns 402, it is returned to the caller.

---

## OKX Facilitator Client

`OKXFacilitatorClient` implements the `FacilitatorClient` interface against OKX's proprietary API endpoints.

### Endpoints

| Operation     | Method | Path                                |
|---------------|--------|-------------------------------------|
| verify        | POST   | `/api/v6/pay/x402/verify`          |
| settle        | POST   | `/api/v6/pay/x402/settle`          |
| settleStatus  | GET    | `/api/v6/pay/x402/settle/status?txHash=` |
| supported     | GET    | `/api/v6/pay/x402/supported`       |

### Authentication

Every request carries four headers generated by `OKXAuth`:

| Header               | Value                                              |
|----------------------|----------------------------------------------------|
| `OK-ACCESS-KEY`      | API key (plaintext)                                |
| `OK-ACCESS-SIGN`     | Base64(HMAC-SHA256(secretKey, prehash))             |
| `OK-ACCESS-TIMESTAMP`| ISO 8601 with milliseconds, e.g. `2025-01-15T10:30:45.123Z` |
| `OK-ACCESS-PASSPHRASE`| Passphrase (plaintext)                            |

The prehash string is: `timestamp + method + path + body`. For GET requests, the body component is empty string. Auth headers are regenerated on every attempt (including retries) because the timestamp must be current.

### Response Envelope Unwrapping

OKX APIs wrap responses in an envelope: `{"code": 0, "data": {...}, "msg": ""}`. The client auto-unwraps this:

- `code == 0`: extract `data` and deserialize into the expected type.
- `code != 0`: throw `IOException` with the mapped error message, even though the HTTP status was 200.
- No envelope (no `code`/`data` fields): treat the body as the direct payload. This handles test mocks and future API changes gracefully.

### Error Code Mapping

Known OKX error codes are mapped to human-readable messages:

| Code  | Meaning                        |
|-------|--------------------------------|
| 50103 | Invalid API key                |
| 50104 | Invalid API key or IP          |
| 50113 | Invalid passphrase             |
| 50001 | Service temporarily unavailable|
| 50011 | Too many requests (rate limit) |
| 8000  | TEE operation failed           |
| 10002 | x402 AA account not found      |

### Sync Settlement Extension

The `settle()` method accepts a `syncSettle` boolean. When `true`, the `SettleRequest` includes `"syncSettle": true`, telling the OKX facilitator to wait for on-chain confirmation before responding. This is an OKX-specific extension not in the base x402 spec. When `false` (default), the facilitator may return `status="pending"` or `status="timeout"`, requiring the caller to poll `settleStatus`.

---

## EIP-712 Signing

`OKXEvmSigner` produces EIP-3009 `TransferWithAuthorization` signatures using EIP-712 typed data hashing.

### Domain Separator

```
EIP712Domain(
    string name,              // from requirements.extra["name"], e.g. "USD₮0"
    string version,           // from requirements.extra["version"], e.g. "1"
    uint256 chainId,          // extracted from CAIP-2 network, e.g. 196
    address verifyingContract  // the token contract address (requirements.asset)
)
```

The `name` field uses Unicode. For X Layer USDT, it is `"USD\u20AE0"` (the U+20AE togrog sign, not a plain T). Getting this wrong produces a valid-looking signature that the contract will reject.

### Struct Hash

```
TransferWithAuthorization(
    address from,        // signer's checksummed address
    address to,          // requirements.payTo
    uint256 value,       // requirements.amount (atomic units)
    uint256 validAfter,  // now - 5 seconds
    uint256 validBefore, // now + maxTimeoutSeconds
    bytes32 nonce        // 32 bytes from SecureRandom
)
```

### Time Window

- `validAfter` = current epoch seconds minus 5. The 5-second grace period is per OKX specification and is intentionally much shorter than Coinbase's 600-second window. It limits the replay window.
- `validBefore` = current epoch seconds plus `maxTimeoutSeconds` (default 86400 = 24 hours per spec).

### Final Hash

```
keccak256("\x19\x01" || domainSeparator || structHash)
```

The signature is produced by `web3j`'s `Sign.signMessage(digest, keyPair, false)`. The `false` parameter means "do not prefix with Ethereum signed message" -- EIP-712 uses raw signing. The 65-byte (r + s + v) result is hex-encoded with a `0x` prefix.

### Input Validation

`signPaymentRequirements` validates eagerly before any cryptographic operations:
- `requirements` must not be null.
- `requirements.extra` must contain `"name"` and `"version"` keys.
- `payTo`, `amount`, and `network` must be non-null.

This fail-fast approach surfaces configuration errors immediately rather than producing a cryptic signature failure downstream.

---

## Asset Registry and Price Resolution

`AssetRegistry` maintains a thread-safe (`ConcurrentHashMap`) registry of token assets per CAIP-2 network. It ships with pre-configured entries:

| Network       | Symbol | Contract Address                           | Decimals | EIP-712 Name | Version |
|---------------|--------|--------------------------------------------|----------|--------------|---------|
| eip155:196    | USDT   | `0x779ded0c9e1022225f8e0630b35a9b54be713736` | 6        | `USD₮0`      | `1`     |
| eip155:196    | USDG   | `0x4ae46a509f6b1d9056937ba4500cb143933d2dc8` | 6        | `USDG`       | `2`     |
| eip155:195    | USDT   | TBD (testnet)                              | 6        | `USD₮0`      | `1`     |

### Price Resolution Flow

When `PaymentFilter` builds requirements for a route, it calls `AssetRegistry.resolvePrice(price, network)`:

1. Look up the default asset for the network (first registered -- typically USDT).
2. Parse the price string. If it starts with `$`, strip the prefix and treat as USD amount.
3. Convert to atomic units: `amount * 10^decimals`, rounded down. For example, `"$0.01"` with 6 decimals becomes `"10000"`.
4. Return a `ResolvedPrice` record containing:
   - `amount`: the atomic units string
   - `asset`: the token contract address
   - `extra`: map with `name`, `version`, `transferMethod` -- these flow into `PaymentRequirements.extra` and are required by the signer

### Extending the Registry

To add a custom token:

```java
AssetRegistry.register("eip155:42161", AssetConfig.builder()
        .symbol("USDC")
        .contractAddress("0xaf88d065e77c8cC2239327C5EDb3A432268e5831")
        .decimals(6)
        .eip712Name("USD Coin")
        .eip712Version("2")
        .transferMethod("eip3009")
        .build());
```

Registration is thread-safe. The first asset registered for a network becomes the default.

---

## Lifecycle Hooks

The hook system follows the same pattern as the Go and TypeScript SDKs.

### Hook Categories

| Type                  | When                           | Can it change the flow? |
|-----------------------|--------------------------------|-------------------------|
| `BeforeVerifyHook`    | Before `facilitator.verify()`  | Yes -- abort with reason |
| `AfterVerifyHook`     | After verify succeeds          | No -- fire-and-forget   |
| `OnVerifyFailureHook` | When verify throws exception   | Yes -- recover with override |
| `BeforeSettleHook`    | Before `facilitator.settle()`  | Yes -- abort with reason |
| `AfterSettleHook`     | After settle succeeds          | No -- fire-and-forget   |
| `OnSettleFailureHook` | When settle fails or throws    | Yes -- recover with override |
| `OnProtectedRequestHook` | After route match, before payment header is read | Yes -- grant (skip payment), abort (HTTP 403), or proceed |
| `OnSettlementTimeoutHook` | When settle status polling times out | Yes -- `confirmed()` grants access, `notConfirmed()` denies |

### Result Types

- **`AbortResult`**: Used by before-hooks. Static factories `proceed()` and `abort(reason)`. When aborted, the reason is included in the 402 error response.
- **`RecoverResult<T>`**: Used by failure-hooks. Static factories `notRecovered()` and `recovered(result)`. When recovered, the provided result replaces the failed operation's output.
- **`ProtectedRequestResult`**: Used by `OnProtectedRequestHook`. Static factories `proceed()`, `grantAccess()`, `abort(reason)`. `abort` responds with HTTP 403 (not 402) and body `{"error":"<reason>"}`.
- **`SettlementTimeoutResult`**: Used by `OnSettlementTimeoutHook`. Static factories `confirmed()` and `notConfirmed()`.

### Registration

Hooks are registered via fluent API on `PaymentProcessor`, not on `PaymentFilter`. `PaymentFilter` is a thin adapter whose only configuration methods are the two `create(...)` factories; hook wiring goes through `filter.processor()`. Multiple hooks of the same type may be registered; they execute in registration order.

```java
PaymentFilter filter = PaymentFilter.create(facilitator, routes);
filter.processor()
    .onBeforeVerify((payload, reqs) -> {
        if (isBlacklisted(payload)) return AbortResult.abort("blocked");
        return AbortResult.proceed();
    })
    .onAfterSettle((payload, reqs, result) -> {
        auditLog.record(result.transaction);
    })
    .onSettlementTimeout((txHash, network) ->
        chainMonitor.isConfirmed(txHash)
            ? SettlementTimeoutResult.confirmed()
            : SettlementTimeoutResult.notConfirmed())
    .onProtectedRequest((req, route) -> {
        if ("internal".equals(req.getHeader("x-api-key"))) {
            return ProtectedRequestResult.grantAccess();   // skip payment
        }
        return ProtectedRequestResult.proceed();
    });
```

All hook interfaces are `@FunctionalInterface`, so lambdas work naturally.

---

## Header Protocol

### Request Headers

| Header              | Protocol | Format                                    |
|---------------------|----------|-------------------------------------------|
| `PAYMENT-SIGNATURE` | V2       | Base64-encoded JSON `PaymentPayload`      |
| `X-PAYMENT`         | V1       | Base64-encoded JSON `v1.PaymentPayload`   |

`PaymentFilter` checks `PAYMENT-SIGNATURE` first, falls back to `X-PAYMENT`.

### 402 Response

- HTTP status: 402
- Body: JSON `PaymentRequired` with `x402Version`, `error`, `resource`, `accepts[]`
- Header `PAYMENT-REQUIRED`: Base64-encoded copy of the JSON body (for cross-SDK compat -- Go and TS clients expect this header)
- Header `Access-Control-Expose-Headers: PAYMENT-REQUIRED`

### 200 Response (after settlement)

- Header `PAYMENT-RESPONSE`: Base64-encoded JSON `SettlementResponseHeader` with `success`, `transaction`, `network`, `payer`
- Header `Access-Control-Expose-Headers: PAYMENT-RESPONSE`

---

## Network Routing

`FacilitatorRouter` implements `FacilitatorClient` and dispatches calls to the appropriate backend based on the `network` field in `PaymentRequirements`.

### Routing Logic

1. Look up the network in the route map (e.g., `"eip155:196"` -> `OKXFacilitatorClient`).
2. If no match, fall back to the configured `defaultClient`.
3. If no default, throw `IllegalStateException`.

### Builder

```java
FacilitatorClient facilitator = FacilitatorRouter.builder()
    .okx(apiKey, secretKey, passphrase)    // auto-registers eip155:196 and eip155:195
    .route("base-sepolia", coinbaseClient) // custom route
    .defaultFacilitator(fallbackClient)    // optional fallback
    .build();
```

The `.okx()` convenience method creates a single `OKXFacilitatorClient` instance and registers it for both X Layer mainnet (196) and testnet (195).

### Aggregated `supported()`

`FacilitatorRouter.supported()` merges the `SupportedResponse` from every registered client (deduplicating by object identity). It unions `kinds`, `extensions`, and `signers` so the server can advertise all capabilities from all backends in a single response.

---

## V1 Compatibility Layer

The SDK preserves V1 protocol support for backward compatibility with Coinbase CDP facilitators:

| V2 Component          | V1 Equivalent              | Notes |
|-----------------------|----------------------------|-------|
| `OKXHttpClient`       | `X402HttpClient`           | V1 sends `X-PAYMENT` header |
| `FacilitatorClient`   | `HttpFacilitatorClient`    | V1 talks to `/verify`, `/settle`, `/supported` without `/api/v6` prefix |
| `v2.PaymentPayload`   | `v1.PaymentPayload`       | V1 has flat `scheme`/`network`/`payload` vs V2's `accepted` structure |
| `v2.PaymentRequirements` | `v1.PaymentRequirements` | V1 uses `maxAmountRequired` + `resource` string; V2 uses `amount` + `ResourceInfo` |
| `EvmSigner`           | `CryptoSigner`             | `EvmSigner` extends `CryptoSigner`; the legacy `sign(Map)` method throws if called directly |

V1 types live in `com.okx.x402.model.v1`. They are not used by the server-side `PaymentFilter` (which is V2-only) but remain available for clients that need to talk to V1 servers.

---

## Error Handling and Retry

### OKX Facilitator Retries

`OKXFacilitatorClient` retries requests up to 3 times with exponential backoff on:

- **HTTP 429** (Too Many Requests): Standard rate-limit response.
- **OKX error code 50011** inside a 200 response: OKX's envelope-level rate limit signal.

Backoff delays: 1s, 2s, 4s (formula: `BASE_RETRY_DELAY_MS * 2^attempt`). Auth headers are regenerated on each retry because the timestamp must be fresh.

### Settlement Timeout Recovery

When `settle()` returns `status="timeout"`, the `PaymentFilter` does not immediately fail. Instead:

1. Polls `settleStatus(txHash)` every `pollInterval` (default 1s) for up to `pollDeadline` (default 5s).
2. If polling succeeds or deterministically fails, uses that result.
3. If polling is inconclusive and a timeout hook is registered, delegates the decision to the developer.
4. If no hook or hook denies, returns 402.

This handles the common case where X Layer block confirmation takes a few extra seconds.

### Exception Propagation

- `IOException`: Network-level failures from `java.net.http.HttpClient` or JSON parsing errors. Propagated to callers or handled by failure hooks.
- `InterruptedException`: Propagated; the SDK does not swallow interrupts.
- `CryptoSignException`: Checked exception from signing. `OKXHttpClient` wraps it in `IOException`. `PaymentFilter` does not catch it directly since signing happens on the client side.
- `IllegalStateException`/`IllegalArgumentException`: Configuration errors (missing network, missing private key, etc.). Thrown eagerly at construction time.

---

## Dependencies

| Dependency          | Version  | Purpose                                        | Scope     |
|---------------------|----------|------------------------------------------------|-----------|
| Jackson Databind    | 2.17.0   | JSON serialization for all model types         | compile   |
| web3j Core          | 4.12.3   | EIP-712 hashing, EC key operations, keccak256  | compile   |
| BouncyCastle        | 1.78.1   | Cryptographic provider for web3j               | compile   |
| Jakarta Servlet API | 6.1.0    | `x402-java-jakarta` `PaymentFilter` / `PaymentInterceptor` | provided  |
| javax.servlet API   | 4.0.1    | `x402-java-javax` `PaymentFilter` / `PaymentInterceptor`   | provided  |
| Spring Web MVC (6.x or 5.x) | 6.1.14 / 5.3.39 | `PaymentInterceptor` (matched to chosen adapter) | provided  |
| JUnit 5             | 5.10.2   | Unit and integration testing                   | test      |
| Mockito             | 5.11.0   | Mocking for servlet and facilitator tests      | test      |
| WireMock            | 3.13.1   | HTTP mock server for facilitator tests         | test      |
| Jetty               | (transitive via WireMock) | Embedded server for E2E tests | test      |

The SDK deliberately avoids heavy frameworks. The core module has **no** servlet or Spring dependencies at all; those live only in the jakarta / javax adapter modules, both scoped `provided` so the host container supplies the actual implementation.

---

## Testing Strategy

Tests are organized in three tiers:

### Unit Tests

Tests follow the module split. Pure logic tests live in `core/src/test/java` (no servlet dependency). Filter/interceptor tests live in `jakarta/src/test/java` because they exercise real servlet types. Use Mockito for servlet mocks and WireMock for HTTP mocks. Cover:

- `OKXEvmSigner`: Signature generation, EIP-712 hash construction, input validation, chain ID extraction.
- `OKXFacilitatorClient`: Envelope unwrapping, error code mapping, retry on 429/50011.
- `PaymentFilter` / `PaymentProcessor`: Route matching, header detection, V1 fallback, 402 response format, `onProtectedRequest` (grant / abort / proceed), lifecycle hooks, settlement timeout polling.
- `AssetRegistry`: Price resolution, registration, thread safety.
- `OKXAuth`: HMAC prehash format, header structure.
- Model types: `toHeader()`/`fromHeader()` round-trip, Jackson serialization.

### End-to-End Tests (`E2EPaymentFlowTest`)

Spin up an embedded Jetty server with `PaymentFilter` and a WireMock facilitator. Exercise the full flow:

1. Client sends GET without payment header -> receives 402.
2. Client signs and resends with `PAYMENT-SIGNATURE` -> WireMock returns verify success.
3. Business logic executes -> WireMock returns settle success.
4. Client receives 200 with `PAYMENT-RESPONSE` header.

Also tests error paths: malformed headers, verify failure, settle failure, settle timeout + polling recovery, and the complete OKXHttpClient auto-402 flow.

### Integration Tests

Two integration test suites test against real OKX infrastructure:

- **`RealFacilitatorIT`**: Tests the `exact` scheme against an OKX-hosted non-production facilitator (URL supplied via the `OKX_FACILITATOR_BASE_URL` environment variable). Exercises verify, settle, settleStatus, and supported with real HTTP calls. The non-production environment may not enforce HMAC auth, so dummy credentials may suffice.
- **`AggrDeferredTestServer`**: A standalone server for manual `aggr_deferred` testing against the production OKX facilitator (`web3.okx.com`). Requires real OKX API credentials.

Integration tests are not run by default (`mvn test` excludes them). Run explicitly with `mvn test -Dtest=RealFacilitatorIT`.

### Quality Tools

`checkstyle.xml`, `checkstyle-suppressions.xml`, and `spotbugs-exclude.xml` are checked into `java/` as configuration templates, but as of the current pom (`1.0.0`) no Maven plugin wires them up — only `maven-enforcer-plugin` (Java 17, Maven 3.6.3, no duplicate dependency versions) and `maven-surefire-plugin` are active. JaCoCo is not configured. Integrators who want style/bug gates or coverage should add the corresponding plugin invocations themselves.

---

## Design Decisions and Trade-offs

### Why both a servlet Filter and a Spring MVC HandlerInterceptor?

The SDK targets the widest possible Java ecosystem. A servlet `Filter` works with any servlet container -- Jetty, Tomcat, Undertow, and Spring Boot (which is built on servlets). Teams already using Spring MVC interceptors for billing or auth prefer `PaymentInterceptor` because ordering across concerns is controlled uniformly via `InterceptorRegistry.order()`. Both adapters delegate to the same servlet-agnostic `PaymentProcessor` in `x402-java-core`, so behavior stays identical regardless of which entry point is wired.

### Why split the SDK into core / jakarta / javax modules?

A single artifact that hard-codes `jakarta.servlet.*` is unusable by Spring Boot 2 / Java EE 8 apps because the javax and jakarta namespaces are different types at the bytecode level, not aliases. Rather than forcing every consumer to upgrade, the core logic is kept servlet-agnostic (`X402Request` / `X402Response` interfaces) and namespace-specific bindings are published as separate adapter artifacts. Non-servlet runtimes (Vert.x, Play, Micronaut Netty) can skip both adapters, depend on `x402-java-core` directly, and implement the adapter SPI themselves in ~50 lines.

### Why public fields on model classes?

V2 model types (`PaymentPayload`, `PaymentRequirements`, etc.) use public fields instead of getters/setters. This is a deliberate choice to match the Go and TypeScript SDKs, which use plain structs. It reduces boilerplate and makes the code easier to read and write. Jackson handles public fields natively.

### Why `java.net.http.HttpClient` instead of OkHttp?

Zero additional dependencies for HTTP in the default build. The JDK's built-in client already supports connection pooling (HTTP/2 multiplexing, HTTP/1.1 keep-alive), custom `Executor`, `SSLContext`, proxy, `Authenticator`, and custom protocol version selection — which covers what most callers actually want.

For teams that standardize on a different HTTP stack (OkHttp, Apache HttpClient, Reactor Netty), the SDK exposes a small `HttpExecutor` SPI in `com.okx.x402.facilitator`. All OKX-specific protocol logic (HMAC auth, envelope unwrapping, error-code mapping, retry on 429 / `50011`) stays inside `OKXFacilitatorClient`; an executor is responsible only for raw HTTP execution. An OkHttp adapter is ~25 lines — see `CONFIG.md §A2.2` for the full recipe. The default implementation, `JdkHttpExecutor`, wraps `java.net.http.HttpClient` and ships in core.

HTTP execution precedence on `OKXFacilitatorClient`: `config.httpExecutor` > `config.httpClient` (wrapped in `JdkHttpExecutor`) > default JDK client built from `config.connectTimeout`.

### Why thread-safe `AssetRegistry` with `ConcurrentHashMap`?

The registry is a singleton with static methods. In a servlet container, multiple request threads may call `resolvePrice()` concurrently. `ConcurrentHashMap` + `computeIfAbsent` ensures safe concurrent reads and writes without explicit locking.

### Why regenerate HMAC headers on retry?

OKX's server validates that the timestamp in the `OK-ACCESS-TIMESTAMP` header is within a few seconds of the current time. Reusing a stale timestamp from the original attempt would cause authentication failures on retries, especially after the 2s or 4s backoff delays.

### Why 5 seconds for `validAfter` (not 600)?

The OKX spec requires a tight time window. Coinbase's 600-second window is designed for networks with slow clocks or high latency. X Layer operates its own facilitator and can enforce a tighter window, reducing the risk of authorization replay.

---

## On-Chain Verification (Integration Test Results)

The exact scheme integration tests produce real on-chain transactions on X Layer mainnet (eip155:196). The shape below documents what an integrator should expect on a successful run; concrete tx hashes, payer / payTo / facilitator-signer addresses are deployment-specific and intentionally not pinned in this document.

To reproduce, run `mvn test -Dtest=RealFacilitatorIT` against your own X Layer credentials and inspect the resulting transactions on the [X Layer block explorer](https://www.oklink.com/xlayer).

### Transaction 1: syncSettle=true

| Field | Expected shape |
|-------|----------------|
| **TxHash** | 32-byte hex string (`0x…`) — real tx hash returned synchronously by the facilitator |
| **Block** | block height assigned by X Layer |
| **Status** | `success` |
| **Contract** | `0x779ded0c9e1022225f8e0630b35a9b54be713736` (X Layer USDT, registry default) |
| **From (payer)** | the EOA address derived from `OKX_PRIVATE_KEY` |
| **To (payTo)** | the address you set via `OKX_PAY_TO` / route config |
| **Amount** | the resolved atomic-units amount (e.g. `1` for `$0.000001` at 6 decimals) |
| **Submitted by** | the OKX facilitator's submitter address (visible on the explorer; deployment-specific) |

### Transaction 2: syncSettle=false (async)

Same shape as above, but the facilitator returns `status="pending"` immediately and the on-chain inclusion happens out-of-band. After confirmation the block + tx layout is identical to Transaction 1.

### On-Chain Event Analysis

A successful settlement emits two events on the USDT contract:
1. **`AuthorizationUsed`** -- EIP-3009 nonce consumed, preventing replay
2. **`Transfer`** -- USDT transferred from payer to payTo

This confirms the full x402 flow: SDK signs EIP-3009 authorization off-chain, OKX facilitator submits the `transferWithAuthorization` call on-chain, USDT moves from payer to seller, nonce is consumed.
