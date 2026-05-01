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
| Idle-timeout auto-settle | ❌ (merchant drives lifecycle — see [Known limitations](#known-limitations)) |
| Dual protocol: MPP + x402 on same URL | ✅ (via `payment-router-axum`) |
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
boilerplate is near zero.

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

// 2. Handler — successful extraction means paid + verified + not expired.
//    `WithReceipt` attaches the `Payment-Receipt` header automatically.
async fn photo(charge: MppCharge<OnePhoto>) -> WithReceipt<Json<Value>> {
    WithReceipt {
        receipt: charge.receipt,
        body: Json(json!({ "url": "https://example.com/photo.jpg" })),
    }
}

// 3. Build the challenger and install it as axum state.
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
use mpp_evm::{EvmSessionMethod, OkxSaApiClient, axum as mpp_axum};
use axum::{routing::{get, post}, Router};
use std::sync::Arc;

let sa = Arc::new(OkxSaApiClient::new("k".into(), "s".into(), "p".into()));
let session = Arc::new(
    EvmSessionMethod::new(sa)
        .with_escrow("0x1234...")       // only required config
);

// Mount seller-initiated endpoints for settle + status.
let router: Router = Router::new()
    .route("/session/settle", post(mpp_axum::session_settle))
    .route("/session/status", get(mpp_axum::session_status))
    .with_state(session);
```

See [`examples/src/mpp_session_server.rs`](../examples/src/mpp_session_server.rs).

## Custom signer integration

`EvmSessionMethod::with_signer` accepts any `alloy::signers::Signer + Send + Sync + 'static` — internally stored as `Arc<dyn Signer + Send + Sync>` so a single method instance can serve concurrent signing calls.

### Local private key

Dev / unit tests; **never use raw env vars in production**.

```rust,ignore
use alloy_signer_local::PrivateKeySigner;
use mpp_evm::{EvmSessionMethod, OkxSaApiClient};

let signer: PrivateKeySigner = std::env::var("MERCHANT_PK")?.parse()?;
let method = EvmSessionMethod::new(sa_client).with_signer(signer);
```

### AWS KMS

Private key never leaves KMS; recommended for production.

```rust,ignore
use alloy_signer_aws::AwsSigner;
use aws_config::BehaviorVersion;

let aws_cfg = aws_config::load_defaults(BehaviorVersion::latest()).await;
let kms = aws_sdk_kms::Client::new(&aws_cfg);
let aws_signer = AwsSigner::new(kms, "alias/merchant-payee".into(), Some(196)).await?;
let method = EvmSessionMethod::new(sa_client).with_signer(aws_signer);
```

### Ledger hardware wallet

```rust,ignore
use alloy_signer_ledger::{LedgerSigner, HDPath};

let ledger = LedgerSigner::new(HDPath::LedgerLive(0), Some(196)).await?;
let method = EvmSessionMethod::new(sa_client).with_signer(ledger);
```

### Custom remote signer (WalletConnect / self-hosted signing service / any RPC)

Implement `Signer` over your transport. The four methods below are the full surface SDK touches:

```rust,ignore
use alloy_signer::{Signer, Result, Signature};
use alloy_primitives::{Address, B256, ChainId};
use async_trait::async_trait;

struct RemoteSignerClient {
    // HTTP client / gRPC channel / message queue ...
}

#[async_trait]
impl Signer for RemoteSignerClient {
    async fn sign_hash(&self, hash: &B256) -> Result<Signature> {
        // POST hash to your backend; backend returns 65-byte r‖s‖v.
        todo!()
    }
    fn address(&self) -> Address { todo!() }
    fn chain_id(&self) -> Option<ChainId> { Some(196) }
    fn set_chain_id(&mut self, _: Option<ChainId>) {}
}

let method = EvmSessionMethod::new(sa_client)
    .with_signer(RemoteSignerClient { /* ... */ });
```

Don't forget to call `.verify_payee(expected_payee_addr)?` after `.with_signer(...)` — fast-fails at startup when the signer's address mismatches the merchant's configured payee, instead of waiting for the first `open` to be rejected.

---

## Custom store integration

`EvmSessionMethod::with_store` swaps the default `InMemorySessionStore` for any `SessionStore` implementation. Same channel concurrency is serialised by SDK-internal `ChannelLocks`; **cross-process** concurrency is the store's responsibility (SQL transactions / Redis WATCH/MULTI / etc.).

The `update` method is the only one with a hard contract: it MUST atomically read → mutate via the closure → write. If the closure returns `Err`, the store MUST NOT persist any partial change.

### SQLite (sqlx)

```rust,ignore
use async_trait::async_trait;
use mpp_evm::{ChannelRecord, EvmSessionMethod, SaApiError, SessionStore};
use mpp_evm::store::ChannelUpdater;
use sqlx::SqlitePool;
use std::sync::Arc;

struct SqliteSessionStore { pool: SqlitePool }

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn get(&self, channel_id: &str) -> Option<ChannelRecord> {
        let row: Option<(String,)> = sqlx::query_as("SELECT json FROM channels WHERE id = ?")
            .bind(channel_id)
            .fetch_optional(&self.pool).await.ok().flatten();
        row.and_then(|(j,)| serde_json::from_str(&j).ok())
    }
    async fn put(&self, record: ChannelRecord) {
        let json = serde_json::to_string(&record).unwrap();
        let _ = sqlx::query("INSERT OR REPLACE INTO channels (id, json) VALUES (?, ?)")
            .bind(&record.channel_id).bind(json)
            .execute(&self.pool).await;
    }
    async fn remove(&self, channel_id: &str) {
        let _ = sqlx::query("DELETE FROM channels WHERE id = ?")
            .bind(channel_id).execute(&self.pool).await;
    }
    async fn update(&self, channel_id: &str, updater: ChannelUpdater)
        -> Result<ChannelRecord, SaApiError>
    {
        // BEGIN IMMEDIATE; SELECT; closure; UPDATE; COMMIT;
        // sqlx transaction auto-rolls back on Err.
        todo!()
    }
}

let method = EvmSessionMethod::with_store(
    sa_client,
    Arc::new(SqliteSessionStore { pool }),
);
```

### Redis (with WATCH/MULTI for atomic update)

```rust,ignore
use async_trait::async_trait;
use redis::aio::ConnectionManager;
use mpp_evm::{ChannelRecord, SaApiError, SessionStore};
use mpp_evm::store::ChannelUpdater;

struct RedisSessionStore {
    conn: ConnectionManager,
    prefix: String, // e.g. "tenant_a:" — multi-tenancy
}

#[async_trait]
impl SessionStore for RedisSessionStore {
    async fn get(&self, channel_id: &str) -> Option<ChannelRecord> {
        let mut c = self.conn.clone();
        let json: Option<String> = redis::cmd("GET")
            .arg(format!("{}{channel_id}", self.prefix))
            .query_async(&mut c).await.ok();
        json.and_then(|j| serde_json::from_str(&j).ok())
    }
    // put / remove: straightforward SET / DEL.
    // update: WATCH key; read; run closure; MULTI; SET; EXEC. Retry on nil EXEC.
    # async fn put(&self, _: ChannelRecord) { todo!() }
    # async fn remove(&self, _: &str) { todo!() }
    # async fn update(&self, _: &str, _: ChannelUpdater) -> Result<ChannelRecord, SaApiError> { todo!() }
}
```

### Postgres (`SELECT ... FOR UPDATE` for cross-process exclusion)

```rust,ignore
# use async_trait::async_trait;
# use mpp_evm::{ChannelRecord, SaApiError, SessionStore};
# use mpp_evm::store::ChannelUpdater;
struct PgSessionStore { pool: sqlx::PgPool }

#[async_trait]
impl SessionStore for PgSessionStore {
    // update body:
    //   BEGIN;
    //   SELECT json FROM channels WHERE id = $1 FOR UPDATE;
    //   <parse, run closure, serialise>
    //   UPDATE channels SET json = $2 WHERE id = $1;
    //   COMMIT;
    # async fn get(&self, _: &str) -> Option<ChannelRecord> { todo!() }
    # async fn put(&self, _: ChannelRecord) { todo!() }
    # async fn remove(&self, _: &str) { todo!() }
    # async fn update(&self, _: &str, _: ChannelUpdater) -> Result<ChannelRecord, SaApiError> { todo!() }
}
```

### Decorator (metrics / cache / sharding on top of any inner store)

```rust,ignore
# use async_trait::async_trait;
# use mpp_evm::{ChannelRecord, SaApiError, SessionStore};
# use mpp_evm::store::ChannelUpdater;
struct ObservedStore<S> {
    inner: S,
    metrics: prometheus::HistogramVec,
}

#[async_trait]
impl<S: SessionStore> SessionStore for ObservedStore<S> {
    async fn get(&self, channel_id: &str) -> Option<ChannelRecord> {
        let _t = self.metrics.with_label_values(&["get"]).start_timer();
        self.inner.get(channel_id).await
    }
    // put / remove / update: same delegation pattern.
    # async fn put(&self, _: ChannelRecord) { todo!() }
    # async fn remove(&self, _: &str) { todo!() }
    # async fn update(&self, _: &str, _: ChannelUpdater) -> Result<ChannelRecord, SaApiError> { todo!() }
}
```

---

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
- `session_method` — `EvmSessionMethod` (`impl mpp::protocol::traits::SessionMethod`).
  Merchant drives settle/close lifecycle explicitly via
  `settle_with_authorization()` / `close_with_authorization()`; no idle timer.
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

## Known limitations

### No idle-timer auto-settle

`EvmSessionMethod` does not run a background timer to auto-settle abandoned
sessions. The merchant calls `settle_with_authorization()` /
`close_with_authorization()` explicitly. If a payer abandons a session
without closing, the deposit stays escrowed on-chain until the merchant
settles or the contract's own timeout fires (typically 12-24h).

### topUp partial-failure leaves stale local deposit

`handle_topup` forwards the credential to SA first (which broadcasts the
on-chain top-up), then increments the local `deposit` counter. If the
local update fails (typically because the in-memory record was lost on
SDK restart and no persistent `SessionStore` is configured), on-chain
state is updated but the local cap is stale. A pre-flight check rejects
the topUp before reaching SA when the channel record is missing, so the
common restart-then-topup path stays consistent — the residual race is
a `get` succeeding then `update` failing, which is rare.

Mitigations: configure a persistent `SessionStore` impl (SQLite / Redis /
Postgres / DynamoDB) for production; when stale local state is suspected,
re-fetch on-chain truth via `session_status` and rebuild the
`ChannelRecord` manually. A `refresh_from_chain` helper is on the roadmap.

### Voucher signers must be EOAs

`verify_voucher` recovers the signer via secp256k1 ecrecover. Smart-contract
wallets (EIP-1271, ERC-4337, Safe, Argent, Coinbase Smart Wallet) are not
supported as voucher signers — the local-only verification path can't
make on-chain `isValidSignature` calls. Use an EOA delegate via
`authorizedSigner` at channel-open time if the payer is a smart-contract
wallet.

## Testing

```bash
cargo test -p mpp-evm                         # 60 unit tests, all local
cargo test -p mpp-evm --test sandbox -- --ignored   # against SA sandbox
```

Sandbox tests require: `MPP_SA_SANDBOX_URL / _KEY / _SECRET / _PASSPHRASE`.

## References

- MPP integration design (spec §8 data model) —
  https://okg-block.sg.larksuite.com/wiki/HVuEwbo3fiTndzkNAmKlZll5gdg
- MPP EVM API design —
  https://okg-block.sg.larksuite.com/wiki/OXbOwA4rviD3tQkUKIElaRRpgfe
- mpp-rs upstream — https://github.com/tempoxyz/mpp-rs (v0.10)
- mpp-specs — https://github.com/okx/mpp-specs

## License

Apache-2.0. See [LICENSE](../LICENSE).
