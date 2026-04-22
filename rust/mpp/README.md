# mpp-evm

OKX Payments — MPP EVM Seller SDK (Rust).

Integrates the MPP protocol (Machine Payments Protocol) into the OKX Payments
SDK. Implements `mpp-rs`'s `ChargeMethod` / `SessionMethod` traits backed by
the OKX SA API so merchants don't operate an RPC node or manage on-chain
state themselves.

Target chain: **X Layer (chainId 196)**. ERC-20 tokens with EIP-3009
`transferWithAuthorization` support.

## Scope

| Feature | Supported |
|---------|-----------|
| Charge (one-shot) — transaction mode (EIP-3009) | ✅ |
| Charge (one-shot) — hash mode (client-broadcast) | ✅ |
| Charge splits (multi-recipient) | ✅ |
| Session open / voucher / topUp / close (non-SSE) | ✅ |
| Session mid-settle + status queries | ✅ |
| Idle-timeout auto-settle (5 min, configurable) | ✅ |
| Dual protocol: MPP + x402 on same URL | ❌ (deferred — blocked on upstream `PaymentVerifier` being route-aware) |
| Session SSE streaming + `payment-need-voucher` events | ❌ (out of scope this iteration) |

## Install

```toml
# Cargo.toml
[dependencies]
mpp-evm = { path = "../payments/rust/mpp" }  # or git = "...", rev = "..."
mpp = { git = "https://github.com/tempoxyz/mpp-rs", features = ["server", "evm", "tower", "axum"] }
```

Optional features:

- `mock` — pulls in `MockSaApiClient` (fixed success responses, for local dev / examples only). **Never enable in production dep chains.**
- `handlers` — pulls in drop-in Axum handlers for `/session/settle` + `/session/status`. Skip if you write your own endpoints.

```toml
# In dev / examples:
mpp-evm = { path = "...", features = ["mock", "handlers"] }
```

### Known caveat: submodule fetch

The `okx/payments` monorepo contains Solidity submodules under
`contracts/evm/lib/` (forge-std, OpenZeppelin, Permit2) that aren't needed
for Rust consumers. Cargo recursively clones them by default. Work around
with either:

1. Move Rust crates to a standalone repo (recommended long-term).
2. Set `[net] git-fetch-with-cli = true` in `~/.cargo/config.toml` to let
   `git` skip submodules locally.
3. Use `cargo --offline` after a first `cargo fetch` when submodules are
   unavailable.

## Quick start — Charge

Use upstream `MppCharge<C>` extractor + `WithReceipt<T>` response wrapper + our
`EvmChargeChallenger` (`impl ChargeChallenger` for EVM + SA API). Handler
boilerplate 近零。

```rust,no_run
use axum::{routing::get, Json, Router};
use mpp::server::axum::{ChargeChallenger, ChargeConfig, MppCharge, WithReceipt};
use mpp_evm::{EvmChargeChallenger, EvmChargeMethod, OkxSaApiClient};
use serde_json::{json, Value};
use std::sync::Arc;

// 1. Per-route price. amount() MUST be base units (MPP spec §: base-10 integer string).
struct OnePhoto;
impl ChargeConfig for OnePhoto {
    fn amount() -> &'static str { "10000" }                 // 0.01 pathUSD (6 decimals)
    fn description() -> Option<&'static str> { Some("One photo") }
}

// 2. Handler —— extractor 成功说明已付款 + 已验签 + 未过期。WithReceipt 自动挂 Payment-Receipt header。
async fn photo(charge: MppCharge<OnePhoto>) -> WithReceipt<Json<Value>> {
    WithReceipt {
        receipt: charge.receipt,
        body: Json(json!({ "url": "https://example.com/photo.jpg" })),
    }
}

// 3. 构造 challenger 挂到 axum state。
#[tokio::main]
async fn main() {
    let sa = Arc::new(OkxSaApiClient::new(
        "OK-ACCESS-KEY".into(),
        "OK-ACCESS-SECRET".into(),
        "OK-ACCESS-PASSPHRASE".into(),
    ));
    let challenger: Arc<dyn ChargeChallenger> = Arc::new(
        EvmChargeChallenger::builder(EvmChargeMethod::new(sa), "api.example.com", "HMAC-SECRET")
            .currency("0x74b7F16337b8972027F6196A17a631aC6dE26d22")
            .recipient("0x4b22fdbc399bd422b6fefcbce95f76642ea29df1")
            .chain_id(196)
            .fee_payer(true)
            .build(),
    );

    let app = Router::new()
        .route("/photo", get(photo))
        .with_state(challenger);
    // axum::serve(listener, app).await.unwrap();
}
```

See [`examples/src/mpp_photo_server.rs`](../examples/src/mpp_photo_server.rs)
for a runnable example (supports `MPP_MOCK=1` for zero-cred local dev).

## Quick start — Session (non-SSE)

```rust
use mpp_evm::{EvmSessionMethod, OkxSaApiClient, handlers};
use axum::{routing::{get, post}, Router};
use std::sync::Arc;

let sa = Arc::new(OkxSaApiClient::new("k".into(), "s".into(), "p".into()));
let session = Arc::new(
    EvmSessionMethod::new(sa)
        .with_escrow("0x1234...")       // only required config
);

// Mount seller-initiated endpoints for settle + status.
let router: Router = Router::new()
    .route("/session/settle", post(handlers::session_settle))
    .route("/session/status", get(handlers::session_status))
    .with_state(session);
```

See [`examples/src/mpp_session_server.rs`](../examples/src/mpp_session_server.rs).

## Error code mapping

SA API error codes → RFC 9457 Problem Details (`error::SaApiError::to_problem_details`):

| SA code | HTTP | Problem type suffix |
|---------|------|---------------------|
| 8000    | 500  | `service-error` |
| 70000   | 400  | `bad-request` |
| 70001   | 422  | `unsupported-chain` |
| 70002   | 403  | `payer-blocked` |
| 70003   | 402  | `malformed-credential` |
| 70004   | 402  | `session/invalid-signature` |
| 70005/70006 | 400 | `invalid-split` |
| 70007   | 402  | `tx-not-confirmed` |
| 70008   | 410  | `session/channel-finalized` |
| 70009   | 401  | `payment-expired` |
| 70010   | 404  | `session/channel-not-found` |
| 70011   | 400  | `session/invalid-escrow-config` |
| 70012   | 402  | `session/amount-exceeds-deposit` |
| 70013   | 402  | `session/delta-too-small` |
| 70014   | 409  | `session/channel-closing` |

## Module map

- `sa_client` — SA API HTTP client (`OkxSaApiClient`) + pluggable `SaApiClient` trait.
- `charge_method` — `EvmChargeMethod` (`impl mpp::protocol::traits::ChargeMethod`).
- `session_method` — `EvmSessionMethod` + idle-timer auto-settle.
- `challenger` — `EvmChargeChallenger` (`impl mpp::server::axum::ChargeChallenger`) for
  use with the upstream `MppCharge<C>` extractor.
- `store` — `SessionStore` trait + `InMemorySessionStore` default.
  Name intentionally distinct from upstream `tempo::session_method::ChannelStore`
  (different model: we store last-receipt summary, not on-chain channel state).
- `types` — Spec §8 data model (method details / payloads / receipts /
  EIP-712 voucher domain).
- `challenge` — `method="evm"` challenge builders (charge + session).
- `handlers` *(feature = "handlers")* — Drop-in Axum handlers for
  `/session/settle` + `/session/status`.
- `mock` *(feature = "mock")* — `MockSaApiClient` for local dev. **Never in production.**
- `error` — `SaApiError` + RFC 9457 mapping.

## Testing

```bash
cargo test -p mpp-evm                         # 60 unit tests, all local
cargo test -p mpp-evm --test sandbox -- --ignored   # against SA sandbox
```

Sandbox tests require: `MPP_SA_SANDBOX_URL / _KEY / _SECRET / _PASSPHRASE`.

## References

- MPP 集成方案 (spec §8 data model) —
  https://okg-block.sg.larksuite.com/wiki/HVuEwbo3fiTndzkNAmKlZll5gdg
- MPP EVM API 方案 —
  https://okg-block.sg.larksuite.com/wiki/OXbOwA4rviD3tQkUKIElaRRpgfe
- mpp-rs upstream — https://github.com/tempoxyz/mpp-rs (v0.9.3)
- mpp-specs — https://github.com/okx/mpp-specs
