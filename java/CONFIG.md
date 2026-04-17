# OKX x402 Java SDK — Configuration Reference

All knobs the SDK exposes, split into three parts by **who owns the
value**:

- **Part A — Must configure** — values the external team **must**
  provide for the SDK to run. No defaults, no fallback.
- **Part B — Optional tuning** — sane defaults ship with the SDK; only
  override when you have a reason.
- **Part C — SDK built-ins (read-only reference)** — values the SDK
  hard-codes; listed here so integrators know what to expect, but not
  something to configure.

Paired with `INTEGRATION.md` (which explains *where* to wire each value).

---

# Part A — Must configure (external team owns)

Everything in this section **must** be supplied by the integrating team.
If any of it is missing, the SDK fails at construction time or first
call.

## A1. Secrets (from Vault / KMS at startup)

The SDK does not read `System.getenv` directly — the host app loads
these and hands them to the SDK constructors. The env-var names below
are the convention used by `DemoServer` / `DemoClient`; you may name
yours differently.

| Variable | Side | Purpose |
|---|---|---|
| `OKX_API_KEY` | server | Facilitator header `OK-ACCESS-KEY` |
| `OKX_SECRET_KEY` | server | HMAC-SHA256 signing key |
| `OKX_PASSPHRASE` | server | Facilitator header `OK-ACCESS-PASSPHRASE` |
| `PAY_TO_ADDRESS` | server | Seller EOA that receives USDT |
| `PRIVATE_KEY` | client (`exact`) | 0x-prefixed hex, 64 chars — used by `OKXEvmSigner` |
| `SESSION_PRIVATE_KEY` + `SESSION_CERT` | client (`aggr_deferred`) | Session-key signing + TEE-issued cert |

Never bake these into app config, image, or Git.

## A2. Facilitator base URL (pick one per environment)

| Environment | URL | How to configure |
|---|---|---|
| Production | `https://www.okx.com` | Use the 3-arg constructor (default) |
| Swimlane / test | `https://forked-okx-test2-dataasset.okx.testokg.com` | Use the 4-arg constructor |

```java
// prod
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
        apiKey, secretKey, passphrase);

// swimlane
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
        apiKey, secretKey, passphrase,
        "https://forked-okx-test2-dataasset.okx.testokg.com");
```

## A3. RouteConfig — required per endpoint

For each HTTP route you want to charge, populate a
`PaymentProcessor.RouteConfig`. **These four fields are mandatory:**

| Field | Type | Example | Notes |
|---|---|---|---|
| `scheme` | String | `"exact"` | `"exact"` or `"aggr_deferred"` |
| `network` | String | `"eip155:196"` | CAIP-2 identifier |
| `payTo` | String | `0x…` | Receiver on the chain identified by `network` |
| `price` **or** `priceFunction` | String / lambda | `"$0.01"` | USD string auto-resolved to atomic units, or a lambda computing price per request |

Route-key format when registering the map:

```java
Map.of(
    "GET /api/data", route,   // method + path (preferred)
    "/api/data",     route    // method-agnostic fallback
);
```

Lookup order: `"METHOD /path"` → `"/path"`.

## A4. Thread pool for async settle (conditional)

Required **only** if any route has `settleInBackground = true`. Without
it, `PaymentProcessor.postHandle` throws `IllegalStateException`.

```java
ExecutorService pool = Executors.newFixedThreadPool(16, r -> {
    Thread t = new Thread(r, "x402-settle");
    t.setDaemon(true);
    return t;
});
processor.settleExecutor(pool);
```

Pool sizing is the caller's responsibility — a blocked settle call waits
up to 30 s on the facilitator.

## A5. Signer (client side)

### A5.1 `exact` scheme

```java
OKXEvmSigner signer = new OKXEvmSigner(privateKeyHex);
```

Key requirements: 64 hex chars, `0x`-prefixed or raw; EOA must hold
enough token balance on the chosen network.

### A5.2 `aggr_deferred` scheme

No one-liner ships in the SDK. External team must provide, at
construction time:

- AA wallet address (becomes `authorization.from`)
- Session private key (signs the EIP-3009 authorization)
- Session certificate issued by OKX Wallet TEE (placed in
  `accepted.extra.sessionCert` on the payload)

Coordinate with the OKX Wallet team for a session-signer that
implements `EvmSigner`.

## A6. Custom assets (conditional)

Required **only** if you charge in a token that is not pre-registered
(see `Part C §C3` for what ships built-in). Call **before** constructing
any `PaymentFilter` / `PaymentInterceptor`:

```java
AssetRegistry.register("eip155:196", AssetConfig.builder()
        .symbol("MYTOKEN")
        .contractAddress("0x…")
        .decimals(18)
        .eip712Name("MYTOKEN")
        .eip712Version("1")
        .transferMethod("eip3009")
        .build());
```

The token **must** support EIP-3009 `transferWithAuthorization` — a
plain ERC-20 will not work for the `exact` scheme.

---

# Part B — Optional tuning

Ships with sensible defaults. Override only when you have a reason.

## B1. RouteConfig — optional fields

| Field | Type | Default | When to override |
|---|---|---|---|
| `asset` | String | registry default (USDT on X Layer) | Charge in a non-default token on the same network |
| `maxTimeoutSeconds` | int | `86400` (24 h) | Shorter window for anti-replay; or longer for slow batch flows |
| `syncSettle` | boolean | `false` | Set `true` to make facilitator wait for on-chain confirmation before returning |
| `settleInBackground` | boolean | `false` | Set `true` to return 200 to the caller immediately; pair with `settleExecutor` (Part A §A4) |
| `priceFunction` | `DynamicPrice` lambda | null | Per-request price (e.g. based on query params) instead of a flat `price` |

## B2. Settle-timeout polling

| Setting | Default | Meaning |
|---|---|---|
| `pollInterval(Duration)` | 1 s | Gap between status polls after a `"timeout"` settle result |
| `pollDeadline(Duration)` | 5 s | Total budget for recovery polling |
| `onSettlementTimeout(hook)` | none | Custom decision if polling still did not confirm — return `true` to treat the tx as success |

## B3. Async-settle completion callback

```java
processor.onAsyncSettleComplete((payload, req, result, err) -> {
    if (err != null) metrics.settleFailed();
    else             metrics.settleOk(result.transaction);
});
```

Strongly recommended when `settleInBackground=true`, since the caller
has already been 200'd and has no other way to learn settle outcome.

## B4. Payment-exempt coordination

Default convention (zero-coupling):

```java
// upstream billing tier marks the request free
request.setAttribute("paymentExempt", Boolean.TRUE);
```

Custom predicate:

```java
processor.paymentExemptWhen(req ->
    Boolean.TRUE.equals(req.getAttribute("billing.free")));
```

Exempt requests skip verify + settle entirely.

## B5. Lifecycle hooks

| Hook | Phase | Can abort? | Can recover? |
|---|---|---|---|
| `onBeforeVerify` | before `facilitator.verify` | yes (`AbortResult.abort(reason)` → 402) | — |
| `onAfterVerify` | after verify success | — | — |
| `onVerifyFailure` | verify threw | — | yes (`RecoverResult.recovered(resp)`) |
| `onBeforeSettle` | before `facilitator.settle` | yes | — |
| `onAfterSettle` | after settle success | — | — |
| `onSettleFailure` | settle failed or threw | — | yes |

## B6. Multi-network facilitator routing

Use when you want X Layer routed to OKX and other chains routed
elsewhere:

```java
FacilitatorClient facilitator = FacilitatorRouter.builder()
        .okx(apiKey, secretKey, passphrase)   // auto-routes eip155:196 + eip155:195
        .route("eip155:1", customMainnetClient)
        .defaultFacilitator(cdpClient)
        .build();
```

## B7. `OKXHttpClient` network selection (client side)

| Constructor | Default network |
|---|---|
| `new OKXHttpClient(signer)` | `"eip155:196"` (X Layer mainnet) |
| `new OKXHttpClient(signer, network)` | explicit |

When a server returns multiple `accepts` entries, the client picks the
first one whose `network` matches the configured value; otherwise falls
back to the first entry.

---

# Part C — SDK built-ins (read-only reference)

Values the SDK hard-codes. Listed so integrators know what to expect —
**not** configurable.

## C1. HTTP behavior

| Property | Value |
|---|---|
| Connect timeout | 10 s |
| Request timeout | 30 s |
| Retry on HTTP 429 | yes |
| Retry on OKX envelope `code=50011` (rate-limit) | yes |
| Max retries | 3 |
| Back-off | exponential: 1 s → 2 s → 4 s |

## C2. RouteConfig defaults

| Field | Default |
|---|---|
| `scheme` | `"exact"` |
| `maxTimeoutSeconds` | `86400` |
| `syncSettle` | `false` |
| `settleInBackground` | `false` |

## C3. Pre-registered assets

| Chain | Network | Symbol | Contract | Decimals | EIP-712 Name | Version |
|---|---|---|---|---|---|---|
| X Layer mainnet | `eip155:196` | USDT | `0x779ded0c9e1022225f8e0630b35a9b54be713736` | 6 | `USD₮0` (U+20AE) | `1` |
| X Layer mainnet | `eip155:196` | USDG | `0x4ae46a509f6b1d9056937ba4500cb143933d2dc8` | 6 | `USDG` | `2` |
| X Layer testnet | `eip155:195` | USDT | TBD | 6 | `USD₮0` | `1` |

The first asset registered per network is the "default" picked when
`RouteConfig.asset` is unset.

## C4. Price resolution

- `"$0.01"` → `AssetRegistry.resolvePrice` → atomic units using the
  default asset's `decimals` (`"10000"` for 6 decimals).
- `$` prefix is stripped; stablecoins are treated 1:1 with USD.
- Non-stablecoin assets require the caller to pass atomic units
  directly, or compute with `priceFunction`.

## C5. Facilitator error-code mapping

Wrapped by the SDK as
`IOException("OKX API error on /verify (code=X): …")`.

| Code | Mapped message |
|---|---|
| `50103` | Invalid API key |
| `50104` | Invalid API key or IP |
| `50113` | Invalid passphrase |
| `50001` | Service temporarily unavailable |
| `50011` | Too many requests (rate limit) — **retried automatically** |
| `8000` | TEE operation failed |
| `10002` | x402 AA account not found |

Other codes pass through with the raw OKX `msg` string.

## C6. Settle-timeout polling defaults

| Setting | Default |
|---|---|
| `pollInterval` | 1 s |
| `pollDeadline` | 5 s |

(Override via Part B §B2.)
