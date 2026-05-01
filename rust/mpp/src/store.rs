//! Local channel state store for the Seller SDK.
//!
//! Stores the SDK-local [`ChannelRecord`]: on-chain parameters required
//! for voucher verification (payer / payee / authorized_signer / escrow /
//! chain_id), highest voucher (byte-level replay protection), and
//! throttling params (min_voucher_delta).
//!
//! `SessionStore` is a pluggable trait:
//! - Default [`InMemorySessionStore`]: in-process HashMap, lost on
//!   restart; suitable for demos / single-process setups.
//! - Production merchants should implement
//!   `SqliteSessionStore` / `RedisSessionStore` (the SQLite template in
//!   §3.5 is a starting point) and inject via
//!   [`EvmSessionMethod::with_store`].
//!
//! Persistence is the merchant's responsibility. On `get` miss, the SDK
//! returns `None` — it does not auto-recover from `/session/status`
//! because the recoverable subset doesn't include `cumulativeAmount` or
//! `highest_voucher_signature` (insufficient to reconstruct voucher state).
//! Merchants needing cross-process durability must implement their own
//! persistent store.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use alloy_primitives::{Address, Bytes};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::SaApiError;

/// The SDK's local channel record. Minimal 12-field shape — does **not**
/// store `settled_on_chain` / `finalized` / `close_requested_at` /
/// `last_receipt` / `challenge` (differs from Tempo's `ChannelState`); see
/// the §3.5 design notes.
///
/// `spent` / `units` (billing fields) match TS Session.ts ChannelState:
/// `available = highest_voucher_amount - spent` is the deductible balance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelRecord {
    pub channel_id: String,
    pub chain_id: u64,
    pub escrow_contract: Address,

    /// Payer address.
    pub payer: Address,
    /// Payee (merchant) address. The SDK verifies this equals
    /// `signer.address()` on `ACTION_OPEN`.
    pub payee: Address,
    /// Voucher signer address. When `channel.authorizedSigner ==
    /// address(0)`, it's resolved to `payer` at open time, so the storage
    /// layer always sees a valid non-zero address.
    pub authorized_signer: Address,

    /// Cumulative deposit (initialized at open, incremented at topUp).
    pub deposit: u128,
    /// Highest cumulative voucher amount the SDK has accepted.
    pub highest_voucher_amount: u128,
    /// 65-byte signature for `highest_voucher_amount`. Used for:
    /// 1. Forwarding to SA API as `voucherSignature` during settle / close.
    /// 2. Byte-level idempotency in `submit_voucher` (matching
    ///    `highest_voucher_amount` and identical signature bytes →
    ///    treated as a replay; verification skipped).
    pub highest_voucher_signature: Option<Bytes>,

    /// Throttling: minimum voucher increment, configured via
    /// `SessionMethodDetails.minVoucherDelta`. `None` disables throttling.
    pub min_voucher_delta: Option<u128>,

    /// Total amount already deducted (base units). Each
    /// `deduct_from_channel` call adds to it. Invariant:
    /// `spent <= highest_voucher_amount`.
    pub spent: u128,
    /// Total billed calls (`deduct_from_channel` invocation count).
    pub units: u64,
}

impl ChannelRecord {
    /// `authorized_signer` is already resolved to a valid address at open
    /// time (`address(0) → payer`); this accessor returns it directly for
    /// local verification.
    pub fn voucher_signer(&self) -> Address {
        self.authorized_signer
    }
}

/// Closure type for atomic [`ChannelRecord`] updates. Returning `Err`
/// fails the whole `update` and the prior value stays in place (database
/// transaction semantics).
pub type ChannelUpdater = Box<dyn FnOnce(&mut ChannelRecord) -> Result<(), SaApiError> + Send>;

/// Pluggable channel-storage trait.
///
/// **Not** coupled to SA API: the trait makes no HTTP calls — pure data
/// access. The SDK does not auto-recover from SA API on a miss either;
/// SA's recoverable subset doesn't include `cumulativeAmount` or
/// `highest_voucher_signature`, leaving voucher state incomplete.
/// Merchants needing cross-process durability must implement persistence.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Read a channel. `None` means there is no local record.
    async fn get(&self, channel_id: &str) -> Option<ChannelRecord>;

    /// Write a channel; overwrites existing entries.
    async fn put(&self, record: ChannelRecord);

    /// Delete a channel. Called by `EvmSessionMethod` after a successful close.
    async fn remove(&self, channel_id: &str);

    /// Atomic closure update: load the current record, run `updater`,
    /// write back.
    /// `None` (channel absent) → returns `70010 channel_not_found`.
    /// `updater` returns `Err` → no write happens; the error propagates.
    async fn update(
        &self,
        channel_id: &str,
        updater: ChannelUpdater,
    ) -> Result<ChannelRecord, SaApiError>;
}

/// Default implementation: in-process `HashMap` synchronized with a std
/// `Mutex` (operations are short and don't `await`).
///
/// **Lost on restart**: a process restart or crash drops all channel
/// state. Production deployments should provide a persistent
/// implementation — see the §3.5 warning.
#[derive(Debug, Default, Clone)]
pub struct InMemorySessionStore {
    inner: Arc<Mutex<HashMap<String, ChannelRecord>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Recover from a poisoned mutex by taking the inner data anyway.
    /// `ChannelRecord` is plain data with no broken-invariant risk: even
    /// if a previous holder panicked mid-update, the `clone-then-write`
    /// pattern in [`Self::update`] ensures the stored map is unchanged on
    /// closure panic. Returning `into_inner()` keeps the SDK alive across
    /// transient panics instead of taking down every subsequent request.
    fn lock_inner(&self) -> MutexGuard<'_, HashMap<String, ChannelRecord>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn get(&self, channel_id: &str) -> Option<ChannelRecord> {
        self.lock_inner().get(channel_id).cloned()
    }

    async fn put(&self, record: ChannelRecord) {
        self.lock_inner().insert(record.channel_id.clone(), record);
    }

    async fn remove(&self, channel_id: &str) {
        self.lock_inner().remove(channel_id);
    }

    async fn update(
        &self,
        channel_id: &str,
        updater: ChannelUpdater,
    ) -> Result<ChannelRecord, SaApiError> {
        let mut map = self.lock_inner();
        let record = map
            .get_mut(channel_id)
            .ok_or_else(|| SaApiError::new(70010, "channel not found"))?;
        // Apply the updater to a clone, then write back ONLY on success.
        // This honors the documented transaction semantics: Err leaves the
        // store unchanged. Cloning is cheap (ChannelRecord is small) and
        // matches upstream mpp-rs's value-based `update_channel` contract.
        let mut draft = record.clone();
        updater(&mut draft)?;
        *record = draft.clone();
        Ok(draft)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    fn fixture_record(channel_id: &str, deposit: u128, highest: u128) -> ChannelRecord {
        ChannelRecord {
            channel_id: channel_id.to_string(),
            chain_id: 196,
            escrow_contract: address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b"),
            payer: address!("aabbccddee11223344556677889900aabbccddee"),
            payee: address!("742d35Cc6634c0532925a3b844bC9e7595F8fE00"),
            authorized_signer: address!("aabbccddee11223344556677889900aabbccddee"),
            deposit,
            highest_voucher_amount: highest,
            highest_voucher_signature: None,
            min_voucher_delta: None,
            spent: 0,
            units: 0,
        }
    }

    #[tokio::test]
    async fn put_then_get_returns_record() {
        let store = InMemorySessionStore::new();
        store.put(fixture_record("0xa", 1000, 0)).await;
        let got = store.get("0xa").await.unwrap();
        assert_eq!(got.channel_id, "0xa");
        assert_eq!(got.deposit, 1000);
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let store = InMemorySessionStore::new();
        assert!(store.get("0xnope").await.is_none());
    }

    #[tokio::test]
    async fn put_overwrites_previous_record() {
        let store = InMemorySessionStore::new();
        store.put(fixture_record("0xa", 1000, 0)).await;
        store.put(fixture_record("0xa", 2000, 100)).await;
        let got = store.get("0xa").await.unwrap();
        assert_eq!(got.deposit, 2000);
        assert_eq!(got.highest_voucher_amount, 100);
    }

    #[tokio::test]
    async fn remove_clears_record() {
        let store = InMemorySessionStore::new();
        store.put(fixture_record("0xa", 1000, 0)).await;
        store.remove("0xa").await;
        assert!(store.get("0xa").await.is_none());
    }

    #[tokio::test]
    async fn update_applies_closure_atomically() {
        let store = InMemorySessionStore::new();
        store.put(fixture_record("0xa", 1000, 100)).await;

        let updated = store
            .update(
                "0xa",
                Box::new(|r| {
                    r.highest_voucher_amount = 250;
                    r.highest_voucher_signature = Some(Bytes::from(vec![0xab; 65]));
                    Ok(())
                }),
            )
            .await
            .unwrap();
        assert_eq!(updated.highest_voucher_amount, 250);

        // Verify the stored value was actually modified.
        let got = store.get("0xa").await.unwrap();
        assert_eq!(got.highest_voucher_amount, 250);
        assert_eq!(got.highest_voucher_signature.unwrap().len(), 65);
    }

    #[tokio::test]
    async fn update_missing_channel_returns_70010() {
        let store = InMemorySessionStore::new();
        let result = store.update("0xnope", Box::new(|_| Ok(()))).await;
        match result {
            Err(e) => assert_eq!(e.code, 70010),
            Ok(_) => panic!("expected error for missing channel"),
        }
    }

    #[tokio::test]
    async fn update_propagates_closure_error_and_does_not_modify() {
        let store = InMemorySessionStore::new();
        store.put(fixture_record("0xa", 1000, 100)).await;

        let result = store
            .update(
                "0xa",
                Box::new(|r| {
                    // Closure mutates the draft, then returns Err.
                    // The store must NOT persist the mutation.
                    r.highest_voucher_amount = 999;
                    Err(SaApiError::new(70013, "delta too small"))
                }),
            )
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, 70013);

        // Stored value must remain at the pre-update value (100).
        // This honors the trait's transactional contract: closure Err →
        // no write happens.
        let got = store.get("0xa").await.unwrap();
        assert_eq!(
            got.highest_voucher_amount, 100,
            "closure Err must roll back; record was modified to {} but should be 100",
            got.highest_voucher_amount,
        );
    }

    #[tokio::test]
    async fn channel_record_round_trips_serde() {
        let original = fixture_record("0xa", 1000, 250);
        let json = serde_json::to_string(&original).unwrap();
        let parsed: ChannelRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn voucher_signer_returns_authorized_signer() {
        let r = fixture_record("0xa", 100, 0);
        assert_eq!(r.voucher_signer(), r.authorized_signer);
    }

    /// Regression: a panic that poisons the inner mutex must NOT take down
    /// every subsequent call. We poison via a sync block (Mutex poisons
    /// only on panic-while-holding), then verify a fresh `get`/`put` still
    /// works. The data itself is unaffected because `update`'s
    /// clone-then-write pattern leaves the stored map untouched on closure
    /// panic.
    #[tokio::test]
    async fn poisoned_mutex_does_not_kill_subsequent_calls() {
        let store = InMemorySessionStore::new();
        store.put(fixture_record("0xa", 1000, 100)).await;

        // Poison the inner mutex by panicking while holding it.
        let inner = store.inner.clone();
        let _ = std::thread::spawn(move || {
            let _guard = inner.lock().unwrap();
            panic!("poison the mutex");
        })
        .join();
        assert!(store.inner.is_poisoned(), "mutex must be poisoned");

        // Subsequent calls must still succeed.
        let got = store.get("0xa").await.expect("get post-poison");
        assert_eq!(got.deposit, 1000);
        store.put(fixture_record("0xb", 2000, 0)).await;
        assert_eq!(store.get("0xb").await.unwrap().deposit, 2000);
    }
}
