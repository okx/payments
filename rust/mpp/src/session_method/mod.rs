//! `EvmSessionMethod` — OKX SA API implementation of `SessionMethod`.
//!
//! ## Design notes
//!
//! 1. **Vouchers are local.** `ACTION_VOUCHER` is verified and stored locally;
//!    nothing is forwarded to SA. Every voucher submission runs
//!    `deduct_from_channel`. Byte-level idempotency only skips verification and
//!    `highest_voucher_*` updates — **deduct still runs**. A client can sign
//!    one large voucher once and replay the exact bytes many times to drain
//!    the balance; `spent` accumulates until it hits `highest`, then we return
//!    70015 to force the client to sign a higher one.
//! 2. **Merchant drives settle / close.** The merchant calls
//!    `settle_with_authorization()` / `close_with_authorization()`; the SDK
//!    locally signs SettleAuth / CloseAuth and POSTs a flat payload (no
//!    `challenge` wrapper) to SA.
//! 3. **No idle timer.** The merchant owns lifecycle decisions.
//! 4. **Payee consistency check.** On `ACTION_OPEN` we verify
//!    `signer.address() == challenge.recipient`; mismatches refuse to write
//!    the store.
//! 5. **Persistence is the merchant's responsibility.** A `store.get` miss
//!    returns 70010 directly; the SDK does not auto-recover via
//!    `session_status` because the recoverable subset lacks
//!    `cumulativeAmount` and `highest_voucher_signature` (insufficient to
//!    rebuild voucher state). Merchants needing cross-process durability
//!    should implement [`SessionStore`](crate::SessionStore) (SQLite / Redis
//!    / etc.); this crate ships an in-process [`InMemorySessionStore`] only.
//!
//! Signer injection: `with_signer` accepts any [`alloy::signers::Signer`]
//! implementor — local key (`PrivateKeySigner`), AWS KMS, Ledger / Trezor,
//! WalletConnect bridges, or merchant-defined wrappers. Internally stored as
//! `Arc<dyn Signer + Send + Sync>` for sharing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use alloy_primitives::{Address, Bytes, U256};
use alloy_signer::Signer;
use tokio::sync::Mutex as AsyncMutex;

use crate::eip712::{
    sign_close_authorization, sign_settle_authorization, verify_voucher, DomainMeta,
};
use crate::error::SaApiError;
use crate::nonce::{NonceProvider, UuidNonceProvider};
use crate::sa_client::SaApiClient;
use crate::store::{
    ChannelRecord, ChannelUpdater, InMemorySessionStore, SessionStore,
};
use crate::types::{
    ChannelStatus, CloseRequestPayload, SessionMethodDetails, SessionReceipt, SettleRequestPayload,
    DEFAULT_CHAIN_ID,
};

mod decode;
mod handlers;
mod trait_impl;
use decode::*;

// ===================== ChannelLocks =====================

/// Per-channel mutex pool. `submit_voucher` / `settle` / `close` for the
/// same channelId run serially to prevent lost updates on concurrent
/// vouchers. Different channelIds are fully independent.
#[derive(Default)]
struct ChannelLocks {
    inner: StdMutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl ChannelLocks {
    /// Acquire the per-channelId lock; reads and writes hold it for the duration.
    async fn lock(&self, channel_id: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let mutex = {
            let mut map = self.inner.lock().unwrap();
            map.entry(channel_id.to_string())
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        };
        mutex.lock_owned().await
    }
}

// ===================== EvmSessionMethod =====================

/// Default deadline = `U256::MAX` (effectively never expires).
fn default_deadline() -> U256 {
    U256::MAX
}

/// EVM Session Method backed by OKX SA API.
#[derive(Clone)]
pub struct EvmSessionMethod {
    pub(super) sa_client: Arc<dyn SaApiClient>,
    pub(super) store: Arc<dyn SessionStore>,
    /// Method details for challenge generation (chainId, escrowContract, ...).
    pub(super) method_details: Option<serde_json::Value>,

    /// `dyn Signer` lets merchants plug in KMS, Ledger, WalletConnect, or any
    /// other remote signer — not just `PrivateKeySigner`. Any
    /// `alloy::signers::Signer` implementor works.
    signer: Option<Arc<dyn Signer + Send + Sync>>,
    /// Cached `signer.address()`. `None` means no signer has been injected.
    pub(super) payee_address: Option<Address>,
    nonce_provider: Arc<dyn NonceProvider>,
    /// Deadline for SettleAuthorization / CloseAuthorization signatures.
    /// Default `U256::MAX`; configurable.
    default_deadline: U256,
    /// Per-channelId mutex pool.
    channel_locks: Arc<ChannelLocks>,
    /// Stash for voucher-action deduct results (`spent`, `units`), keyed by
    /// `challenge_id` (matches `credential.challenge.id` from
    /// `verify_session` / `respond`). `respond()` reads and removes the
    /// entry immediately to prevent unbounded growth.
    pub(super) voucher_deduct_results: Arc<StdMutex<HashMap<String, (u128, u64)>>>,
    /// EIP-712 domain `name` / `version`. Defaults to the OKX
    /// EvmPaymentChannel canonical values; override via `with_domain_meta(...)`
    /// when forking the contract with a different domain.
    pub(super) domain_meta: DomainMeta,
}

impl EvmSessionMethod {
    /// Construct with the default in-memory store.
    pub fn new(sa_client: Arc<dyn SaApiClient>) -> Self {
        Self::with_store(sa_client, Arc::new(InMemorySessionStore::new()))
    }

    /// Inject a custom [`SessionStore`]. The default
    /// [`InMemorySessionStore`] is an in-process `HashMap` — wiped on restart
    /// — and is only suitable for dev / tests. Production deployments must
    /// plug in a persistent store; merchants implement the four async
    /// [`SessionStore`] methods on top of any backend (SQLite / Redis /
    /// Postgres / DynamoDB / ...).
    ///
    /// `update` is an **atomic closure contract** (transaction / `WATCH` /
    /// `SELECT FOR UPDATE` / etc.). Same-channel concurrency is serialized
    /// by the SDK's internal lock; cross-process concurrency must be
    /// handled by the merchant's store.
    ///
    /// See
    /// [README → Custom store integration](https://github.com/okx/payments/blob/main/rust/mpp/README.md#custom-store-integration)
    /// for full SQLite / Redis / Postgres / decorator examples.
    pub fn with_store(sa_client: Arc<dyn SaApiClient>, store: Arc<dyn SessionStore>) -> Self {
        Self {
            sa_client,
            store,
            method_details: None,
            signer: None,
            payee_address: None,
            nonce_provider: Arc::new(UuidNonceProvider),
            default_deadline: default_deadline(),
            channel_locks: Arc::new(ChannelLocks::default()),
            voucher_deduct_results: Arc::new(StdMutex::new(HashMap::new())),
            domain_meta: DomainMeta::default(),
        }
    }

    /// Inject a signer. Accepts any [`alloy::signers::Signer`](Signer)
    /// implementor — local private key, AWS KMS, Ledger / Trezor hardware
    /// wallets, WalletConnect bridges, or merchant-defined remote signing
    /// wrappers. Stored internally as `Arc<dyn Signer + Send + Sync>`.
    ///
    /// ```ignore
    /// use alloy_signer_local::PrivateKeySigner;
    ///
    /// let signer: PrivateKeySigner = std::env::var("MERCHANT_PK")?.parse()?;
    /// let method = EvmSessionMethod::new(sa_client).with_signer(signer);
    /// ```
    ///
    /// See
    /// [README → Custom signer integration](https://github.com/okx/payments/blob/main/rust/mpp/README.md#custom-signer-integration)
    /// for KMS / Ledger / custom wrapper examples.
    pub fn with_signer<S: Signer + Send + Sync + 'static>(mut self, signer: S) -> Self {
        self.payee_address = Some(signer.address());
        self.signer = Some(Arc::new(signer));
        self
    }

    /// Startup fast-fail check: assert the injected signer's address equals
    /// the merchant's configured payee address. Mismatches return 8000
    /// immediately — better than discovering the same mismatch only when
    /// the first `open` request rejects on
    /// `challenge.recipient != signer.address()`.
    ///
    /// Chained usage:
    /// ```ignore
    /// let method = EvmSessionMethod::new(sa)
    ///     .with_signer(signer)
    ///     .verify_payee(expected_payee_addr)?;
    /// ```
    pub fn verify_payee(self, expected: Address) -> Result<Self, SaApiError> {
        match self.payee_address {
            Some(actual) if actual == expected => Ok(self),
            Some(actual) => Err(SaApiError::new(
                8000,
                format!(
                    "payee mismatch: signer.address={actual:#x} but expected={expected:#x}; \
                     SDK signer must be merchant's receiving address"
                ),
            )),
            None => Err(SaApiError::new(
                8000,
                "no signer configured (call .with_signer before .verify_payee)",
            )),
        }
    }

    /// Inject a custom nonce provider (defaults to [`UuidNonceProvider`]).
    pub fn with_nonce_provider(mut self, p: Arc<dyn NonceProvider>) -> Self {
        self.nonce_provider = p;
        self
    }

    /// Override the EIP-712 domain `name` / `version`. Defaults to the OKX
    /// canonical values (`"EVM Payment Channel"` / `"1"`).
    ///
    /// When forking the contract with a different domain `name` or
    /// `version`, **you must** call this with byte-exact values matching
    /// the deployed contract — otherwise voucher verification, SettleAuth,
    /// and CloseAuth signatures will all fail to match on-chain.
    pub fn with_domain_meta(
        mut self,
        name: impl Into<std::borrow::Cow<'static, str>>,
        version: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Self {
        self.domain_meta = DomainMeta::new(name, version);
        self
    }

    /// Override the signature deadline (defaults to `U256::MAX`, never expires).
    pub fn with_deadline(mut self, d: U256) -> Self {
        self.default_deadline = d;
        self
    }

    /// Set the challenge `methodDetails` (chainId / escrowContract / ...).
    pub fn with_method_details(mut self, details: serde_json::Value) -> Self {
        self.method_details = Some(details);
        self
    }

    /// Typed builder: set via [`SessionMethodDetails`].
    pub fn with_typed_method_details(
        mut self,
        details: SessionMethodDetails,
    ) -> Result<Self, serde_json::Error> {
        self.method_details = Some(serde_json::to_value(&details)?);
        Ok(self)
    }

    /// Minimal builder: only escrow; `chain_id` defaults to X Layer.
    pub fn with_escrow(self, escrow_contract: impl Into<String>) -> Self {
        let details = SessionMethodDetails {
            chain_id: DEFAULT_CHAIN_ID,
            escrow_contract: escrow_contract.into(),
            channel_id: None,
            min_voucher_delta: None,
            fee_payer: None,
            splits: None,
        };
        self.with_typed_method_details(details).unwrap()
    }

    /// Store handle, for handler integrations.
    pub fn store(&self) -> Arc<dyn SessionStore> {
        self.store.clone()
    }

    /// Read-only channel status query (proxies SA API).
    pub async fn status(&self, channel_id: &str) -> Result<ChannelStatus, SaApiError> {
        self.sa_client.session_status(channel_id).await
    }

    // ===================== submit_voucher (local processing, 9-step guards) =====================

    /// Process a voucher locally: verify signature + atomically update
    /// `highest_voucher_*`. **Business code should not call this directly**;
    /// [`SessionMethod::verify_session`]'s `ACTION_VOUCHER` branch does.
    ///
    /// Byte-level idempotency (channelId + cum + signature all equal) only
    /// skips verification and `highest_voucher_*` updates — the caller
    /// still runs `deduct_from_channel` to bill this request, matching mppx
    /// / OKX TS Session behavior. This lets a client sign one large voucher
    /// and replay the bytes many times: server `spent` keeps climbing until
    /// it hits `highest`, then 70015 forces the client to bump `cum` and
    /// re-sign.
    pub async fn submit_voucher(
        &self,
        channel_id: &str,
        cumulative_amount: u128,
        signature: Bytes,
    ) -> Result<(), SaApiError> {
        // A. Per-channel lock.
        let _guard = self.channel_locks.lock(channel_id).await;

        // B. Load the local record (miss → 70010; no auto-recovery — see module note #5).
        let channel = self
            .store
            .get(channel_id)
            .await
            .ok_or_else(|| SaApiError::new(70010, "channel not found in local store"))?;

        // C. Upper-bound guard (cumulative <= deposit).
        if cumulative_amount > channel.deposit {
            return Err(SaApiError::new(70012, "amount exceeds deposit"));
        }

        // D. Byte-level replay (exact cum + signature) → skip verify + highest update.
        if cumulative_amount <= channel.highest_voucher_amount {
            let exact_replay = channel.highest_voucher_signature.as_ref().is_some_and(|s| {
                s == &signature && cumulative_amount == channel.highest_voucher_amount
            });
            if exact_replay {
                tracing::debug!(
                    channel_id,
                    cumulative_amount,
                    "voucher byte-level replay — skipping verify+highest update, deduct still applies"
                );
                return Ok(());
            }
            // The protocol code table (70000-70014) has no dedicated
            // "not increasing" code; delta <= 0 is the extreme case of
            // 70013 voucher_delta_too_small, so we reuse it.
            return Err(SaApiError::new(
                70013,
                "voucher cumulative not strictly increasing (delta <= 0)",
            ));
        }

        // E. min_delta throttle.
        if let Some(min_delta) = channel.min_voucher_delta {
            if cumulative_amount - channel.highest_voucher_amount < min_delta {
                return Err(SaApiError::new(70013, "delta too small"));
            }
        }

        // F. Local EIP-712 verification.
        let channel_id_b256 = parse_b256(channel_id)?;
        verify_voucher(
            &self.domain_meta,
            channel.escrow_contract,
            channel.chain_id,
            channel_id_b256,
            cumulative_amount,
            &signature,
            channel.voucher_signer(),
        )
        .map_err(|e| SaApiError::new(70004, format!("verify voucher: {e}")))?;

        // G. Atomic local store update.
        let updater: ChannelUpdater = Box::new(move |c: &mut ChannelRecord| {
            c.highest_voucher_amount = cumulative_amount;
            c.highest_voucher_signature = Some(signature);
            Ok(())
        });
        self.store.update(channel_id, updater).await?;

        Ok(())
    }

    // ===================== deduct_from_channel (billing) =====================

    /// Atomic deduct. `available = highest_voucher_amount - spent`; if
    /// `available < amount`, returns `70015 insufficient-balance`,
    /// otherwise `spent += amount; units += 1`. Returns the updated
    /// [`ChannelRecord`] snapshot.
    ///
    /// Matches TS `Session.ts::deduct(challengeId, amount)`. Merchant
    /// code can call this outside of `verify_session` (e.g. non-linear
    /// billing tied to actual service consumption); the `voucher` action
    /// already calls it internally with `SessionRequest.amount`.
    pub async fn deduct_from_channel(
        &self,
        channel_id: &str,
        amount: u128,
    ) -> Result<ChannelRecord, SaApiError> {
        let _guard = self.channel_locks.lock(channel_id).await;
        let updater: ChannelUpdater = Box::new(move |c: &mut ChannelRecord| {
            let available = c
                .highest_voucher_amount
                .checked_sub(c.spent)
                .ok_or_else(|| SaApiError::new(8000, "spent exceeds highest voucher"))?;
            if available < amount {
                return Err(SaApiError::new(
                    70015,
                    format!(
                        "insufficient balance: requested {amount} but available {available}"
                    ),
                ));
            }
            c.spent = c
                .spent
                .checked_add(amount)
                .ok_or_else(|| SaApiError::new(8000, "spent overflow"))?;
            c.units = c
                .units
                .checked_add(1)
                .ok_or_else(|| SaApiError::new(8000, "units overflow"))?;
            Ok(())
        });
        self.store.update(channel_id, updater).await
    }

    // ===================== settle / close (merchant-driven) =====================

    /// Settle proactively: load latest local voucher → locally sign
    /// SettleAuth → call SA `/session/settle`.
    pub async fn settle_with_authorization(
        &self,
        channel_id: &str,
    ) -> Result<SessionReceipt, SaApiError> {
        let _guard = self.channel_locks.lock(channel_id).await;
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| SaApiError::new(8000, "no signer configured (call .with_signer)"))?;
        let payee = self
            .payee_address
            .ok_or_else(|| SaApiError::new(8000, "payee address missing"))?;

        let channel = self
            .store
            .get(channel_id)
            .await
            .ok_or_else(|| SaApiError::new(70010, "channel not found in local store"))?;

        let cumulative = channel.highest_voucher_amount;
        let voucher_sig_bytes = channel
            .highest_voucher_signature
            .clone()
            .ok_or_else(|| SaApiError::new(70000, "no voucher to settle"))?;

        let channel_id_b256 = parse_b256(channel_id)?;
        let nonce = self.nonce_provider.allocate(payee, channel_id_b256).await?;
        let deadline = self.default_deadline;

        let signed = sign_settle_authorization(
            &self.domain_meta,
            signer.as_ref(),
            channel.escrow_contract,
            channel.chain_id,
            channel_id_b256,
            cumulative,
            nonce,
            deadline,
        )
        .await?;

        let payload = SettleRequestPayload {
            action: Some("settle".into()),
            channel_id: channel_id.to_string(),
            cumulative_amount: cumulative.to_string(),
            voucher_signature: hex_with_prefix(&voucher_sig_bytes),
            payee_signature: hex_with_prefix(&signed.signature),
            nonce: nonce.to_string(),
            deadline: deadline.to_string(),
        };
        self.sa_client.session_settle(&payload).await
    }

    /// Close proactively: load latest local voucher → locally sign
    /// CloseAuth → call SA `/session/close`; on success remove the
    /// `ChannelRecord` from the store.
    ///
    /// `cumulative_amount = None` uses the local `highest` (typical case);
    /// `Some(amt)` lets the caller specify it (B-1 path: payer supplies
    /// the final voucher).
    pub async fn close_with_authorization(
        &self,
        channel_id: &str,
        cumulative_amount: Option<u128>,
        provided_voucher_sig: Option<Bytes>,
    ) -> Result<SessionReceipt, SaApiError> {
        let _guard = self.channel_locks.lock(channel_id).await;
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| SaApiError::new(8000, "no signer configured (call .with_signer)"))?;
        let payee = self
            .payee_address
            .ok_or_else(|| SaApiError::new(8000, "payee address missing"))?;

        let channel = self
            .store
            .get(channel_id)
            .await
            .ok_or_else(|| SaApiError::new(70010, "channel not found in local store"))?;

        let cumulative = cumulative_amount.unwrap_or(channel.highest_voucher_amount);

        // Guard: refuse to close at a cum below the local highest. Mirrors
        // `submit_voucher`'s 70012 invariant — close is more sensitive than
        // voucher (settles on-chain), so it must be at least as strict.
        // When `cumulative_amount = None` this passes naturally because
        // `cumulative` defaults to `highest_voucher_amount`.
        if cumulative < channel.highest_voucher_amount {
            return Err(SaApiError::new(
                70012,
                format!(
                    "close cum {} is below local highest voucher amount {}",
                    cumulative, channel.highest_voucher_amount
                ),
            ));
        }

        // Waiver branch: send empty string when there is no voucher. The
        // server accepts waiver on either `cum <= settledOnChain` or
        // `voucherSignature == ""`; the SDK passes through caller intent
        // and does no local judgement.
        let voucher_sig_bytes = provided_voucher_sig
            .or_else(|| channel.highest_voucher_signature.clone());

        let channel_id_b256 = parse_b256(channel_id)?;

        // No re-verification here: the voucher was either provided by the
        // payer through ACTION_CLOSE (verified at that entry) or it's the
        // local `highest` (already verified by `submit_voucher`).

        let nonce = self.nonce_provider.allocate(payee, channel_id_b256).await?;
        let deadline = self.default_deadline;

        let signed = sign_close_authorization(
            &self.domain_meta,
            signer.as_ref(),
            channel.escrow_contract,
            channel.chain_id,
            channel_id_b256,
            cumulative,
            nonce,
            deadline,
        )
        .await?;

        let voucher_signature = match voucher_sig_bytes {
            Some(b) => hex_with_prefix(&b),
            None => String::new(), // Waiver path: empty string triggers server-side waiver.
        };
        let payload = CloseRequestPayload {
            action: Some("close".into()),
            channel_id: channel_id.to_string(),
            cumulative_amount: cumulative.to_string(),
            voucher_signature,
            payee_signature: hex_with_prefix(&signed.signature),
            nonce: nonce.to_string(),
            deadline: deadline.to_string(),
        };

        let receipt = self.sa_client.session_close(&payload).await?;
        // Remove from store on close success (not a "finalized" flag).
        self.store.remove(channel_id).await;
        Ok(receipt)
    }
}


// ===================== Tests =====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eip712::Voucher;
    use alloy_primitives::{address, b256, hex, B256};
    use alloy_signer::SignerSync;
    use alloy_signer_local::PrivateKeySigner;
    use alloy_sol_types::SolStruct;
    use async_trait::async_trait;
    use mpp::protocol::core::{Base64UrlJson, ChallengeEcho, PaymentCredential};
    use mpp::protocol::intents::SessionRequest;
    use mpp::protocol::traits::SessionMethod;
    use std::sync::Mutex as StdMutex;

    fn fixture_signer() -> PrivateKeySigner {
        "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318"
            .parse()
            .unwrap()
    }

    // ===================== parse_did_pkh_eip155 =====================

    #[test]
    fn parse_did_pkh_happy_path_returns_address() {
        let did = "did:pkh:eip155:196:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3";
        let addr = parse_did_pkh_eip155(did, 196).unwrap();
        assert_eq!(addr, address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3"));
    }

    #[test]
    fn parse_did_pkh_chain_id_zero_is_valid() {
        let did = "did:pkh:eip155:0:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3";
        let addr = parse_did_pkh_eip155(did, 0).unwrap();
        assert_eq!(addr, address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3"));
    }

    #[test]
    fn parse_did_pkh_wrong_prefix_rejected() {
        // did:ethr:... is not pkh.
        let did = "did:ethr:eip155:196:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3";
        let err = parse_did_pkh_eip155(did, 196).unwrap_err();
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("did:pkh:eip155:"));
    }

    #[test]
    fn parse_did_pkh_chain_id_leading_zero_rejected() {
        let did = "did:pkh:eip155:0196:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3";
        let err = parse_did_pkh_eip155(did, 196).unwrap_err();
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("leading zero"));
    }

    #[test]
    fn parse_did_pkh_extra_colon_in_address_rejected() {
        // Address segment contains a colon → reject (suffix-forgery guard).
        let did = "did:pkh:eip155:196:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3:bonus";
        let err = parse_did_pkh_eip155(did, 196).unwrap_err();
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("invalid chars"));
    }

    #[test]
    fn parse_did_pkh_wrong_chain_id_rejected() {
        let did = "did:pkh:eip155:1:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3";
        let err = parse_did_pkh_eip155(did, 196).unwrap_err();
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("!= expected"));
    }

    #[test]
    fn parse_did_pkh_invalid_address_rejected() {
        let did = "did:pkh:eip155:196:not-an-address";
        let err = parse_did_pkh_eip155(did, 196).unwrap_err();
        assert_eq!(err.code, 70000);
    }

    // ===================== extract_payer_and_signer =====================

    #[test]
    fn extract_transaction_mode_uses_authorization_from() {
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "from": "0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3",
            },
        });
        let (payer, signer) = extract_payer_and_signer(&payload, None, 196).unwrap();
        let expected = address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3");
        assert_eq!(payer, expected);
        assert_eq!(signer, expected); // authorizedSigner missing → fall back to payer.
    }

    #[test]
    fn extract_transaction_mode_ignores_source() {
        // Transaction mode does not cross-check source against from: even
        // when the source DID's address segment differs from `from`, the
        // SDK uses `from` (it's the signature-bound authoritative value).
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "from": "0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3",
            },
        });
        let source = Some("did:pkh:eip155:196:0xaaaabbbbccccddddeeeeffff0000000011112222");
        let (payer, _) = extract_payer_and_signer(&payload, source, 196).unwrap();
        assert_eq!(payer, address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3"));
    }

    #[test]
    fn extract_hash_mode_parses_payer_from_source() {
        let payload = serde_json::json!({
            "type": "hash",
            "channelId": format!("0x{}", "ab".repeat(32)),
            "salt": format!("0x{}", "01".repeat(32)),
            "hash": format!("0x{}", "02".repeat(32)),
        });
        let source = Some("did:pkh:eip155:196:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3");
        let (payer, signer) = extract_payer_and_signer(&payload, source, 196).unwrap();
        let expected = address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3");
        assert_eq!(payer, expected);
        assert_eq!(signer, expected);
    }

    #[test]
    fn extract_hash_mode_missing_source_returns_70000() {
        let payload = serde_json::json!({ "type": "hash" });
        let err = extract_payer_and_signer(&payload, None, 196).unwrap_err();
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("missing source"));
    }

    #[test]
    fn extract_explicit_authorized_signer_takes_priority() {
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "from": "0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3",
            },
            "authorizedSigner": "0xaaaabbbbccccddddeeeeffff0000000011112222",
        });
        let (payer, signer) = extract_payer_and_signer(&payload, None, 196).unwrap();
        assert_eq!(payer, address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3"));
        assert_eq!(signer, address!("aaaabbbbccccddddeeeeffff0000000011112222"));
    }

    #[test]
    fn extract_authorized_signer_zero_falls_back_to_payer() {
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "from": "0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3",
            },
            "authorizedSigner": "0x0000000000000000000000000000000000000000",
        });
        let (payer, signer) = extract_payer_and_signer(&payload, None, 196).unwrap();
        assert_eq!(payer, signer);
        assert_eq!(payer, address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3"));
    }

    #[test]
    fn extract_authorized_signer_equals_payer_silently_accepted() {
        // Explicit `authorizedSigner == from` (redundant but valid) → silently accepted, not normalized.
        let payer_str = "0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3";
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": { "from": payer_str },
            "authorizedSigner": payer_str,
        });
        let (payer, signer) = extract_payer_and_signer(&payload, None, 196).unwrap();
        assert_eq!(payer, signer);
    }

    #[test]
    fn extract_unsupported_type_rejected() {
        let payload = serde_json::json!({ "type": "magic" });
        let err = extract_payer_and_signer(&payload, None, 196).unwrap_err();
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("unsupported payload type"));
    }

    // ===================== strip_sdk_only_open_fields =====================

    /// Build an open-credential fixture.
    ///
    /// `payload_type` determines the voucher-signature field name:
    /// - `"transaction"` → `voucherSignature` (`signature` is reserved for the EIP-3009 deposit sig).
    /// - `"hash"`        → `signature` (no deposit sig in hash mode; voucher takes the slot).
    ///
    /// The transaction fixture also stuffs a fake `signature` (EIP-3009)
    /// field to verify that `strip` doesn't remove it.
    fn fixture_credential_with_initial_voucher(
        payload_type: &str,
    ) -> PaymentCredential {
        let mut payload = serde_json::Map::new();
        payload.insert("action".into(), serde_json::json!("open"));
        payload.insert("type".into(), serde_json::json!(payload_type));
        payload.insert("channelId".into(), serde_json::json!("0xchan"));
        payload.insert("salt".into(), serde_json::json!("0xsalt"));
        payload.insert("cumulativeAmount".into(), serde_json::json!("0")); // ← SDK-only

        let voucher_sig = format!("0x{}", "ab".repeat(65));
        if payload_type == "hash" {
            // hash mode: `signature` IS the voucher sig — SDK-only, must be stripped.
            payload.insert("hash".into(), serde_json::json!(format!("0x{}", "cd".repeat(32))));
            payload.insert("signature".into(), serde_json::json!(voucher_sig));
        } else {
            // transaction mode: `signature` is the EIP-3009 deposit sig — SA must keep it.
            payload.insert(
                "signature".into(),
                serde_json::json!(format!("0x{}", "ef".repeat(65))),
            );
            payload.insert("voucherSignature".into(), serde_json::json!(voucher_sig));
        }

        PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-strip".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "session".into(),
                request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
                expires: Some("2026-04-29T10:00:00Z".into()),
                digest: None,
                opaque: None,
            },
            source: Some("did:pkh:eip155:196:0xabc".into()),
            payload: serde_json::Value::Object(payload),
        }
    }

    #[test]
    fn strip_transaction_removes_cumulative_and_voucher_sig_keeps_signature() {
        let cred = fixture_credential_with_initial_voucher("transaction");
        let stripped = strip_sdk_only_open_fields(&cred).unwrap();
        let payload = stripped.get("payload").and_then(|v| v.as_object()).unwrap();
        assert!(!payload.contains_key("cumulativeAmount"), "cumulativeAmount must be stripped");
        assert!(!payload.contains_key("voucherSignature"), "voucherSignature must be stripped");
        // EIP-3009 signature must be kept.
        assert!(payload.contains_key("signature"), "transaction signature (EIP-3009) must be kept");
        // Other fields preserved.
        assert_eq!(payload.get("action").and_then(|v| v.as_str()), Some("open"));
        assert_eq!(payload.get("type").and_then(|v| v.as_str()), Some("transaction"));
        assert_eq!(payload.get("channelId").and_then(|v| v.as_str()), Some("0xchan"));
        assert_eq!(payload.get("salt").and_then(|v| v.as_str()), Some("0xsalt"));
        // Top-level challenge / source preserved.
        assert!(stripped.get("challenge").is_some());
        assert_eq!(stripped.get("source").and_then(|v| v.as_str()), Some("did:pkh:eip155:196:0xabc"));
    }

    #[test]
    fn strip_hash_removes_cumulative_and_signature_keeps_hash() {
        let cred = fixture_credential_with_initial_voucher("hash");
        let stripped = strip_sdk_only_open_fields(&cred).unwrap();
        let payload = stripped.get("payload").and_then(|v| v.as_object()).unwrap();
        assert!(!payload.contains_key("cumulativeAmount"), "cumulativeAmount must be stripped");
        // hash mode: `signature` is the voucher sig (SDK-only) — must be stripped.
        assert!(!payload.contains_key("signature"), "hash-mode signature (voucher) must be stripped");
        // `hash` is required by SA — must be kept.
        assert!(payload.contains_key("hash"), "tx hash must be kept");
        // Other fields preserved.
        assert_eq!(payload.get("action").and_then(|v| v.as_str()), Some("open"));
        assert_eq!(payload.get("type").and_then(|v| v.as_str()), Some("hash"));
        assert_eq!(payload.get("channelId").and_then(|v| v.as_str()), Some("0xchan"));
    }

    #[test]
    fn strip_works_when_sdk_only_fields_absent() {
        // When the CLI doesn't send these fields, strip is a no-op (must not panic).
        let mut cred = fixture_credential_with_initial_voucher("transaction");
        if let Some(obj) = cred.payload.as_object_mut() {
            obj.remove("cumulativeAmount");
            obj.remove("voucherSignature");
        }
        let stripped = strip_sdk_only_open_fields(&cred).unwrap();
        let payload = stripped.get("payload").and_then(|v| v.as_object()).unwrap();
        assert_eq!(payload.get("action").and_then(|v| v.as_str()), Some("open"));
    }

    /// Sign a voucher with `fixture_signer` against the test channel/escrow.
    fn fixture_voucher_sig(cum: u128) -> Bytes {
        let signer = fixture_signer();
        let escrow = address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b");
        let channel_id = b256!("6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f");
        let domain = crate::eip712::build_domain(&DomainMeta::default(), 196, escrow);
        let voucher = Voucher {
            channelId: channel_id,
            cumulativeAmount: cum,
        };
        let digest = voucher.eip712_signing_hash(&domain);
        let sig = signer.sign_hash_sync(&digest).unwrap();
        Bytes::from(sig.as_bytes().to_vec())
    }

    fn fixture_channel_record() -> ChannelRecord {
        let signer = fixture_signer();
        ChannelRecord {
            channel_id: "0x6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f"
                .into(),
            chain_id: 196,
            escrow_contract: address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b"),
            payer: signer.address(),
            payee: signer.address(),
            authorized_signer: signer.address(),
            deposit: 1_000_000,
            highest_voucher_amount: 0,
            highest_voucher_signature: None,
            min_voucher_delta: None,
            spent: 0,
            units: 0,
        }
    }

    #[derive(Default)]
    struct StubSa {
        next_error: StdMutex<Option<SaApiError>>,
    }

    #[async_trait]
    impl SaApiClient for StubSa {
        async fn charge_settle(
            &self,
            _: &serde_json::Value,
        ) -> Result<crate::types::ChargeReceipt, SaApiError> {
            unreachable!()
        }
        async fn charge_verify_hash(
            &self,
            _: &serde_json::Value,
        ) -> Result<crate::types::ChargeReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_open(&self, _: &serde_json::Value) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_top_up(
            &self,
            _: &serde_json::Value,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_settle(
            &self,
            _: &SettleRequestPayload,
        ) -> Result<SessionReceipt, SaApiError> {
            if let Some(e) = self.next_error.lock().unwrap().take() {
                return Err(e);
            }
            Ok(SessionReceipt {
                method: "evm".into(),
                intent: "session".into(),
                status: "success".into(),
                timestamp: "2026-04-01T12:00:00Z".into(),
                chain_id: 196,
                channel_id: "0xabc".into(),
                reference: Some("0xtx".into()),
                deposit: Some("1000".into()),
                challenge_id: None,
                accepted_cumulative: None,
                spent: None,
                confirmations: None,
                units: None,
            })
        }
        async fn session_close(
            &self,
            _: &CloseRequestPayload,
        ) -> Result<SessionReceipt, SaApiError> {
            Ok(SessionReceipt {
                method: "evm".into(),
                intent: "session".into(),
                status: "success".into(),
                timestamp: "2026-04-01T12:00:00Z".into(),
                chain_id: 196,
                channel_id: "0xabc".into(),
                reference: Some("0xclose_tx".into()),
                deposit: Some("0".into()),
                challenge_id: None,
                accepted_cumulative: None,
                spent: None,
                confirmations: None,
                units: None,
            })
        }
        async fn session_status(&self, _: &str) -> Result<ChannelStatus, SaApiError> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn submit_voucher_round_trip() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        // Seed the store with the fixture record first.
        let record = fixture_channel_record();
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let cum = 100u128;
        let sig = fixture_voucher_sig(cum);
        method.submit_voucher(&cid, cum, sig.clone()).await.unwrap();

        let r = method.store.get(&cid).await.unwrap();
        assert_eq!(r.highest_voucher_amount, 100);
        assert_eq!(r.highest_voucher_signature, Some(sig));
    }

    #[tokio::test]
    async fn submit_voucher_strict_increasing() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let record = fixture_channel_record();
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let sig100 = fixture_voucher_sig(100);
        let sig50 = fixture_voucher_sig(50);

        method.submit_voucher(&cid, 100, sig100.clone()).await.unwrap();

        // Same cum + same sig → idempotent success.
        method.submit_voucher(&cid, 100, sig100.clone()).await.unwrap();

        // Lower cum with a valid sig → 70013 (delta <= 0, mapped to voucher_delta_too_small).
        let err = method.submit_voucher(&cid, 50, sig50).await.unwrap_err();
        assert_eq!(err.code, 70013);
    }

    #[tokio::test]
    async fn submit_voucher_amount_exceeds_deposit() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let mut record = fixture_channel_record();
        record.deposit = 1000;
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let sig = fixture_voucher_sig(2000);
        let err = method.submit_voucher(&cid, 2000, sig).await.unwrap_err();
        assert_eq!(err.code, 70012);
    }

    #[tokio::test]
    async fn submit_voucher_missing_record_returns_70010() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let sig = fixture_voucher_sig(1);
        let err = method
            .submit_voucher("0xnonexistent", 1, sig)
            .await
            .unwrap_err();
        assert_eq!(err.code, 70010);
    }

    #[tokio::test]
    async fn deduct_from_channel_increments_spent_and_units() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let mut record = fixture_channel_record();
        record.highest_voucher_amount = 1000;
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let r1 = method.deduct_from_channel(&cid, 300).await.unwrap();
        assert_eq!(r1.spent, 300);
        assert_eq!(r1.units, 1);

        let r2 = method.deduct_from_channel(&cid, 200).await.unwrap();
        assert_eq!(r2.spent, 500);
        assert_eq!(r2.units, 2);
    }

    #[tokio::test]
    async fn deduct_from_channel_insufficient_balance_returns_70015() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let mut record = fixture_channel_record();
        record.highest_voucher_amount = 100;
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let err = method.deduct_from_channel(&cid, 200).await.unwrap_err();
        assert_eq!(err.code, 70015);

        // store must not be mutated.
        let r = method.store.get(&cid).await.unwrap();
        assert_eq!(r.spent, 0);
        assert_eq!(r.units, 0);
    }

    #[tokio::test]
    async fn deduct_from_channel_missing_returns_70010() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());
        let err = method
            .deduct_from_channel("0xnonexistent", 1)
            .await
            .unwrap_err();
        assert_eq!(err.code, 70010);
    }

    /// Verify `with_signer` accepts any `Signer` impl, including merchant
    /// remote signers. Uses a minimal mock (locally wraps a
    /// `PrivateKeySigner`; semantically equivalent to a KMS / Ledger /
    /// WalletConnect remote-signer wrapper) to confirm the generic trait
    /// bound holds and `dyn Signer + Send + Sync` works on the
    /// `signer.address()` / `signer.sign_hash()` call paths.
    #[tokio::test]
    async fn with_signer_accepts_arbitrary_signer_impl() {
        use alloy_signer::Signature;

        struct WrappedSigner {
            inner: PrivateKeySigner,
        }

        #[async_trait]
        impl Signer for WrappedSigner {
            async fn sign_hash(&self, hash: &B256) -> alloy_signer::Result<Signature> {
                self.inner.sign_hash(hash).await
            }
            fn address(&self) -> Address {
                self.inner.address()
            }
            fn chain_id(&self) -> Option<alloy_primitives::ChainId> {
                self.inner.chain_id()
            }
            fn set_chain_id(&mut self, chain_id: Option<alloy_primitives::ChainId>) {
                self.inner.set_chain_id(chain_id);
            }
        }

        let wrapped = WrappedSigner {
            inner: fixture_signer(),
        };
        let expected_address = wrapped.address();

        // Key check: `with_signer` accepts any Signer impl, not just PrivateKeySigner.
        let method = EvmSessionMethod::new(Arc::new(StubSa::default())).with_signer(wrapped);

        // payee_address comes from signer.address(); should match the inner PrivateKeySigner.
        assert_eq!(method.payee_address, Some(expected_address));
        // The verify_payee chained fast-fail also goes through the trait method (no dyn-call issues).
        let method = method
            .verify_payee(expected_address)
            .expect("payee match must pass for self-wrapped signer");
        assert!(method.signer.is_some());
    }

    #[tokio::test]
    async fn settle_with_authorization_uses_local_highest() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let mut record = fixture_channel_record();
        record.highest_voucher_amount = 250;
        record.highest_voucher_signature = Some(Bytes::from(vec![0x01; 65]));
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let receipt = method.settle_with_authorization(&cid).await.unwrap();
        assert_eq!(receipt.reference.as_deref(), Some("0xtx"));
    }

    #[tokio::test]
    async fn settle_without_signer_fails_8000() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()));
        // No signer injected.
        let err = method
            .settle_with_authorization("0xabc")
            .await
            .unwrap_err();
        assert_eq!(err.code, 8000);
    }

    #[tokio::test]
    async fn close_removes_channel_record() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let mut record = fixture_channel_record();
        record.highest_voucher_amount = 300;
        record.highest_voucher_signature = Some(Bytes::from(vec![0x02; 65]));
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        method
            .close_with_authorization(&cid, None, None)
            .await
            .unwrap();
        assert!(method.store.get(&cid).await.is_none());
    }

    #[tokio::test]
    async fn close_rejects_cum_below_local_highest() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let mut record = fixture_channel_record();
        record.highest_voucher_amount = 300;
        record.highest_voucher_signature = Some(Bytes::from(vec![0x02; 65]));
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let err = method
            .close_with_authorization(&cid, Some(100), Some(Bytes::from(vec![0x03; 65])))
            .await
            .unwrap_err();
        assert_eq!(err.code, 70012);
        assert!(
            err.to_string().contains("below local highest"),
            "unexpected error message: {}",
            err
        );

        // Channel must remain in the store — close was rejected, not consumed.
        assert!(method.store.get(&cid).await.is_some());
    }

    fn dummy_request() -> SessionRequest {
        SessionRequest {
            amount: "100".into(),
            currency: "0xToken".into(),
            decimals: None,
            recipient: Some("0xPayee".into()),
            unit_type: None,
            suggested_deposit: None,
            method_details: None,
        }
    }

    #[tokio::test]
    async fn voucher_action_auto_deducts_from_request_amount() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let record = fixture_channel_record();
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let cum = 200u128;
        let sig = fixture_voucher_sig(cum);

        let cred = PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-voucher-1".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "session".into(),
                request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
                expires: None,
                digest: None,
                opaque: None,
            },
            source: None,
            payload: serde_json::json!({
                "action": "voucher",
                "channelId": cid,
                "cumulativeAmount": cum.to_string(),
                "signature": format!("0x{}", hex::encode(&sig)),
            }),
        };
        let mut req = dummy_request();
        req.amount = "150".into();

        let receipt = method.verify_session(&cred, &req).await.unwrap();
        assert_eq!(receipt.reference, cid);

        // store should record spent=150 / units=1.
        let r = method.store.get(&cid).await.unwrap();
        assert_eq!(r.spent, 150);
        assert_eq!(r.units, 1);
        assert_eq!(r.highest_voucher_amount, 200);

        // respond() should return spent / units.
        let body = method.respond(&cred, &receipt).expect("respond body for voucher");
        assert_eq!(body.get("spent").and_then(|v| v.as_str()), Some("150"));
        assert_eq!(body.get("units").and_then(|v| v.as_u64()), Some(1));

        // A second respond() with the same challenge_id returns None (already taken).
        assert!(method.respond(&cred, &receipt).is_none());
    }

    #[tokio::test]
    async fn voucher_byte_replay_keeps_deducting_until_balance_exhausted() {
        // Matches mppx / OKX TS Session: the same voucher bytes can be
        // replayed many times; each call still runs deduct, and 70015 only
        // fires once spent reaches highest. This lets a client sign one
        // large voucher and replay the same bytes for subsequent requests
        // without re-signing.
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let record = fixture_channel_record();
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let cum = 200u128;
        let sig = fixture_voucher_sig(cum);
        let make_cred = || PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-replay".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "session".into(),
                request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
                expires: None,
                digest: None,
                opaque: None,
            },
            source: None,
            payload: serde_json::json!({
                "action": "voucher",
                "channelId": cid,
                "cumulativeAmount": cum.to_string(),
                "signature": format!("0x{}", hex::encode(&sig)),
            }),
        };
        let mut req = dummy_request();
        req.amount = "80".into();

        // 1st call: deduct 80 → spent=80 units=1.
        let r1 = method.verify_session(&make_cred(), &req).await.unwrap();
        let body1 = method.respond(&make_cred(), &r1).expect("body1");
        assert_eq!(body1.get("spent").and_then(|v| v.as_str()), Some("80"));
        assert_eq!(body1.get("units").and_then(|v| v.as_u64()), Some(1));

        // 2nd call: byte-level replay of the same voucher → still deduct → spent=160 units=2.
        let r2 = method.verify_session(&make_cred(), &req).await.unwrap();
        let body2 = method.respond(&make_cred(), &r2).expect("body2");
        assert_eq!(body2.get("spent").and_then(|v| v.as_str()), Some("160"));
        assert_eq!(body2.get("units").and_then(|v| v.as_u64()), Some(2));

        // 3rd call: available = 200 - 160 = 40 < 80 → 70015, no deduction.
        let err = method
            .verify_session(&make_cred(), &req)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("insufficient"), "expected insufficient balance, got: {err}");

        // store stays at spent=160 units=2 (3rd failed call doesn't write).
        let s = method.store.get(&cid).await.unwrap();
        assert_eq!(s.spent, 160);
        assert_eq!(s.units, 2);
    }

    #[tokio::test]
    async fn submit_voucher_byte_replay_is_idempotent() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let record = fixture_channel_record();
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let cum = 100u128;
        let sig = fixture_voucher_sig(cum);

        // 1st call: new voucher accepted.
        method.submit_voucher(&cid, cum, sig.clone()).await.unwrap();
        let r1 = method.store.get(&cid).await.unwrap();
        assert_eq!(r1.highest_voucher_amount, cum);

        // 2nd call: same bytes replayed → still Ok; store state unchanged.
        // `submit_voucher` no longer exposes a replay signal to the caller —
        // deduct responsibility belongs to `handle_voucher`.
        method.submit_voucher(&cid, cum, sig).await.unwrap();
        let r2 = method.store.get(&cid).await.unwrap();
        assert_eq!(r2.highest_voucher_amount, cum);
        assert_eq!(r2.spent, r1.spent, "submit_voucher itself never deducts");
    }

    #[tokio::test]
    async fn voucher_action_insufficient_balance_after_overspend() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let record = fixture_channel_record();
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let cum = 100u128;
        let sig = fixture_voucher_sig(cum);
        let cred = PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-voucher-2".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "session".into(),
                request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
                expires: None,
                digest: None,
                opaque: None,
            },
            source: None,
            payload: serde_json::json!({
                "action": "voucher",
                "channelId": cid,
                "cumulativeAmount": cum.to_string(),
                "signature": format!("0x{}", hex::encode(&sig)),
            }),
        };
        // Request 200 but voucher only covers 100 → insufficient.
        let mut req = dummy_request();
        req.amount = "200".into();

        let err = method.verify_session(&cred, &req).await.unwrap_err();
        assert!(err.to_string().contains("insufficient balance"));
    }

    #[tokio::test]
    async fn unknown_action_errors() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());
        let cred = PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-1".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "session".into(),
                request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
                expires: None,
                digest: None,
                opaque: None,
            },
            source: None,
            payload: serde_json::json!({"action": "dance", "channelId": "0xa"}),
        };
        let err = method
            .verify_session(&cred, &dummy_request())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown session action"));
    }

    #[tokio::test]
    async fn topup_rejects_zero_additional_deposit_before_sa_call() {
        // StubSa::session_top_up is unreachable!(); if the guard fires correctly
        // we never reach SA. If the guard regressed, the test would panic via
        // unreachable!().
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());
        let cred = PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-1".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "session".into(),
                request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
                expires: None,
                digest: None,
                opaque: None,
            },
            source: None,
            payload: serde_json::json!({
                "action": "topUp",
                "channelId": "0xabc",
                "additionalDeposit": "0",
            }),
        };
        let err = method
            .verify_session(&cred, &dummy_request())
            .await
            .unwrap_err();
        // `verify_session` returns upstream `VerificationError`, which doesn't
        // expose the SaApiError code directly — match on the message instead.
        assert!(
            err.to_string().contains("greater than 0"),
            "unexpected error: {}",
            err
        );
    }
}
