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
| Non-production / integration | URL supplied by your OKX contact | Use the 4-arg constructor or `OKXFacilitatorConfig` |

```java
// production
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
        apiKey, secretKey, passphrase);

// non-production (e.g. an OKX-hosted integration endpoint)
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
        apiKey, secretKey, passphrase,
        System.getenv("OKX_FACILITATOR_BASE_URL"));
```

### A2.1 Timeouts and custom `HttpClient` — `OKXFacilitatorConfig`

Defaults: 10s connect, 30s request. Tune via `OKXFacilitatorConfig`.

```java
OKXFacilitatorConfig cfg = new OKXFacilitatorConfig(apiKey, secretKey, passphrase);
cfg.baseUrl = System.getenv("OKX_FACILITATOR_BASE_URL");   // optional override
cfg.connectTimeout = Duration.ofSeconds(5);
cfg.requestTimeout = Duration.ofSeconds(60);
cfg.httpClient = mySharedHttpClient;   // optional — see below

OKXFacilitatorClient facilitator = new OKXFacilitatorClient(cfg);
```

Fields:

| Field | Default | Notes |
|---|---|---|
| `apiKey` / `secretKey` / `passphrase` | (required ctor args) | Validated by `OKXAuth` at client construction |
| `baseUrl` | `https://www.okx.com` | Trailing slashes stripped |
| `connectTimeout` | `10s` | Ignored when `httpClient` or `httpExecutor` is supplied |
| `requestTimeout` | `30s` | Always honoured — applied per call |
| `httpClient` | `null` | Caller-supplied `java.net.http.HttpClient`. Convenience for JDK-backed sharing (connection pool / `Executor` / `SSLContext` / proxy / `Authenticator`). Wrapped internally in `JdkHttpExecutor`. Ignored when `httpExecutor` is also set |
| `httpExecutor` | `null` | Full escape hatch — an `HttpExecutor` that performs the raw HTTP call. Use this to route facilitator HTTP through OkHttp, Apache HttpClient, Reactor Netty, or any other stack. See §A2.2 for an OkHttp recipe |

HTTP execution precedence: `httpExecutor` > `httpClient` (wrapped in `JdkHttpExecutor`) > default JDK client built from `connectTimeout`.

### A2.2 Using OkHttp (or any other HTTP stack) via `HttpExecutor`

The SDK keeps all OKX-specific protocol logic (HMAC auth, envelope unwrapping, error-code mapping, retry on 429 / `50011`) inside `OKXFacilitatorClient`. An executor is only responsible for **raw HTTP execution**, so adapting any stack is a ~25-line class:

```java
import com.okx.x402.facilitator.HttpExecutor;
import okhttp3.*;

public class OkHttpExecutor implements HttpExecutor {

    private static final MediaType JSON = MediaType.get("application/json");
    private final OkHttpClient http;

    public OkHttpExecutor(OkHttpClient http) {
        this.http = http;
    }

    @Override
    public HttpExecResult execute(String method, java.net.URI uri, String body,
                                  java.util.Map<String, String> headers,
                                  java.time.Duration timeout) throws IOException {
        // Apply per-call timeout by cloning the shared client.
        OkHttpClient perCall = http.newBuilder().callTimeout(timeout).build();

        Request.Builder rb = new Request.Builder().url(uri.toString());
        headers.forEach(rb::header);
        if ("POST".equals(method)) {
            rb.post(RequestBody.create(body == null ? "" : body, JSON));
        } else {
            rb.get();
        }

        try (Response resp = perCall.newCall(rb.build()).execute()) {
            ResponseBody rbody = resp.body();
            return new HttpExecResult(resp.code(), rbody != null ? rbody.string() : "");
        }
    }
}
```

Wire it up:

```java
OkHttpClient myShared = new OkHttpClient.Builder()
        .connectionPool(sharedPool)
        .addInterceptor(tracingInterceptor)
        .proxy(corporateProxy)
        .build();

OKXFacilitatorConfig cfg = new OKXFacilitatorConfig(apiKey, secretKey, passphrase);
cfg.httpExecutor = new OkHttpExecutor(myShared);
cfg.requestTimeout = Duration.ofSeconds(60);

OKXFacilitatorClient facilitator = new OKXFacilitatorClient(cfg);
```

Add `com.squareup.okhttp3:okhttp` to your own build. The SDK does not depend on OkHttp.

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

Required **only** if any route has `asyncSettle = true`. Without it,
`PaymentProcessor.postHandle` throws `IllegalStateException`.

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
| `asyncSettle` | boolean | `false` | Set `true` to return 200 to the caller immediately; pair with `settleExecutor` (Part A §A4) |
| `priceFunction` | `DynamicPrice` lambda | null | Per-request price (e.g. based on query params) instead of a flat `price` |
| `accepts` | `List<AcceptOption>` | null | List multiple payment options on the same URL — see §B1.1 |

### B1.1 Multi-accept on a single route (`accepts`)

To offer several payment options at the same endpoint (e.g. USDT **or**
USDG, or `exact` **or** `aggr_deferred`), populate
`RouteConfig.accepts` with a list of `AcceptOption`. Each option becomes
one entry in the 402 `PAYMENT-REQUIRED` envelope's `accepts` array, and
the client picks which one to pay. This is the Java equivalent of the
Go SDK's `PaymentOptions` and the TypeScript SDK's `PaymentOption[]`.

```java
PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
route.network = "eip155:196";        // shared defaults
route.payTo   = payTo;
route.accepts = List.of(
    AcceptOption.builder()
        .scheme("exact")
        .price("$0.01")               // asset omitted → registry default (USDT)
        .build(),
    AcceptOption.builder()
        .scheme("exact")
        .asset("0x4ae46a509f6b1d9056937ba4500cb143933d2dc8")   // USDG
        .price("$0.01")
        .build()
);
```

Semantics:

- Any `AcceptOption` field left null inherits from the owning `RouteConfig`.
- Per-option `price` / `priceFunction` / `asset` override the route-level
  values. `maxTimeoutSeconds` is per-option too; `0` means "inherit".
- `syncSettle` / `asyncSettle` are **route-level only** — they apply to
  every option.
- The server verifies the option identified by the client's
  `PaymentPayload.accepted` (matched by `scheme + network + asset`); a
  payload pointing at an option the server did not offer is rejected
  with 402 `"no matching payment option"`.
- When `accepts` is null or empty, the legacy scalar fields
  (`scheme`/`asset`/`price`/`priceFunction`/`payTo`/`maxTimeoutSeconds`)
  are used — existing integrations keep working unchanged.

### B1.2 Client-side selector (`PaymentRequirementsSelector`)

When the 402 envelope lists multiple options, the client picks one.
`OKXHttpClient` uses `PaymentRequirementsSelector` — injectable via
`OKXHttpClientConfig.paymentRequirementsSelector`:

```java
OKXHttpClientConfig cfg = new OKXHttpClientConfig(signer);
cfg.paymentRequirementsSelector = (version, accepts) -> {
    // Prefer USDG, fall back to first
    for (PaymentRequirements r : accepts) {
        if ("0x4ae46a509f6b1d9056937ba4500cb143933d2dc8".equalsIgnoreCase(r.asset)) {
            return r;
        }
    }
    return accepts.get(0);
};
OKXHttpClient client = new OKXHttpClient(cfg);
```

Default behaviour is preserved: match by `network`, else first option —
same as pre-upgrade.

## B2. Settle-timeout polling

| Setting | Default | Meaning |
|---|---|---|
| `pollInterval(Duration)` | 1 s | Gap between status polls after a `"timeout"` settle result |
| `pollDeadline(Duration)` | 5 s | Total budget for recovery polling |
| `onSettlementTimeout(hook)` | none | Custom decision if polling still did not confirm — return `SettlementTimeoutResult.confirmed()` to treat the tx as success, `notConfirmed()` to fall through to 402. Single-hook: last registration wins. Exceptions are caught and treated as `notConfirmed()`. |

## B3. Async-settle completion callback

```java
processor.onAsyncSettleComplete((payload, req, result, err) -> {
    if (err != null) metrics.settleFailed();
    else             metrics.settleOk(result.transaction);
});
```

Strongly recommended when `asyncSettle=true`, since the caller has
already been 200'd and has no other way to learn settle outcome.

## B4. Payment-exempt / early-reject (`onProtectedRequest`)

Runs after route match and before the payment header is read. The hook
receives the request and the matched `RouteConfig`, and returns one of
three decisions:

| Return | Effect |
|---|---|
| `ProtectedRequestResult.proceed()` | Continue to the normal payment flow |
| `ProtectedRequestResult.grantAccess()` | Bypass payment — verify + settle are skipped entirely |
| `ProtectedRequestResult.abort(reason)` | Reject with **HTTP 403** and body `{"error":"<reason>"}` |

```java
// API-key tier bypass
processor.onProtectedRequest((req, route) -> {
    if ("internal".equals(req.getHeader("x-api-key"))) {
        return ProtectedRequestResult.grantAccess();
    }
    return ProtectedRequestResult.proceed();
});
```

Multiple hooks are supported; they run in registration order and the
first hook returning `grantAccess()` or `abort(reason)` wins —
subsequent hooks are skipped for that request. `abort` uses HTTP 403,
not 402; the 402 `PAYMENT-REQUIRED` envelope is reserved for the
"payment required but missing/invalid" path.

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
| `new OKXHttpClient(OKXHttpClientConfig)` | `config.network` (default `"eip155:196"`) |

When a server returns multiple `accepts` entries, the client picks the
first one whose `network` matches the configured value; otherwise falls
back to the first entry.

### B7.1 Timeouts and custom `HttpClient` — `OKXHttpClientConfig`

Same options-object pattern as the facilitator. Defaults: 10s connect,
30s request.

```java
OKXHttpClientConfig cfg = new OKXHttpClientConfig(signer);
cfg.network = "eip155:195";              // optional
cfg.requestTimeout = Duration.ofSeconds(60);
cfg.httpClient = mySharedHttpClient;     // optional — connection pool / proxy / tracing

OKXHttpClient client = new OKXHttpClient(cfg);
```

The `requestTimeout` is applied on the convenience `get(URI)` path.
Callers of the lower-level `request(HttpRequest)` API control their own
request timeout via `HttpRequest.Builder.timeout()` — the SDK does not
modify caller-built requests.

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
| `asyncSettle` | `false` |

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
