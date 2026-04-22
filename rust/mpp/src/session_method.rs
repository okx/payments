//! EvmSessionMethod: SessionMethod implementation backed by SA API.
//!
//! Replaces mpp-rs TempoSessionMethod. Forwards all session operations
//! (open/voucher/topUp/close) to SA API.
//!
//! Additional capabilities not in mpp-rs:
//! - `settle()`: mid-session settlement without closing channel
//! - `status()`: read-only channel state query
//! - Idle timer: per-channel 5-min no-interaction timeout auto-settles via
//!   `/session/settle` (no signature needed) and relies on SA API 24h fallback
//!   for the eventual `close`.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use mpp::protocol::core::{PaymentCredential, Receipt};
use mpp::protocol::intents::SessionRequest;
use mpp::protocol::traits::{SessionMethod, VerificationError};
use tokio::sync::Mutex;

use crate::sa_client::SaApiClient;
use crate::store::{ChannelRecord, SessionStore, InMemorySessionStore};
use crate::types::{ChannelStatus, SessionMethodDetails, SessionReceipt, DEFAULT_CHAIN_ID};

const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Session credential action names (spec §8.3).
const ACTION_OPEN: &str = "open";
const ACTION_VOUCHER: &str = "voucher";
const ACTION_TOPUP: &str = "topUp";
const ACTION_CLOSE: &str = "close";

/// Per-channel timer state.
struct ChannelTimer {
    reset_tx: tokio::sync::watch::Sender<()>,
}

/// EVM Session Method backed by OKX SA API.
#[derive(Clone)]
pub struct EvmSessionMethod {
    sa_client: Arc<dyn SaApiClient>,
    store: Arc<dyn SessionStore>,
    timers: Arc<Mutex<HashMap<String, ChannelTimer>>>,
    /// Method details for challenge generation (chainId, escrowContract, ...).
    method_details: Option<serde_json::Value>,
    idle_timeout: Duration,
}

impl EvmSessionMethod {
    /// Create with the default in-memory channel store and 5-min idle timeout.
    pub fn new(sa_client: Arc<dyn SaApiClient>) -> Self {
        Self::with_store(sa_client, Arc::new(InMemorySessionStore::new()))
    }

    /// Create with a custom channel store.
    pub fn with_store(sa_client: Arc<dyn SaApiClient>, store: Arc<dyn SessionStore>) -> Self {
        Self {
            sa_client,
            store,
            timers: Arc::new(Mutex::new(HashMap::new())),
            method_details: None,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }

    /// Override the challenge `methodDetails` (chainId/escrowContract/...).
    pub fn with_method_details(mut self, details: serde_json::Value) -> Self {
        self.method_details = Some(details);
        self
    }

    /// Override the idle-timeout duration.
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Convenience builder from a typed [`SessionMethodDetails`].
    ///
    /// Equivalent to `with_method_details(serde_json::to_value(&details)?)` but
    /// keeps typed defaults close to the seller code. Prefer this over the
    /// raw JSON variant for new integrations.
    pub fn with_typed_method_details(
        mut self,
        details: SessionMethodDetails,
    ) -> Result<Self, serde_json::Error> {
        self.method_details = Some(serde_json::to_value(&details)?);
        Ok(self)
    }

    /// Minimal-boilerplate configuration: only requires the escrow contract
    /// address; fills in `chainId = 196` (X Layer) and leaves the rest empty.
    pub fn with_escrow(self, escrow_contract: impl Into<String>) -> Self {
        let details = SessionMethodDetails {
            chain_id: DEFAULT_CHAIN_ID,
            escrow_contract: escrow_contract.into(),
            channel_id: None,
            min_voucher_delta: None,
            fee_payer: None,
            splits: None,
        };
        // `to_value` on a concrete struct cannot fail, so unwrap is safe.
        self.with_typed_method_details(details).unwrap()
    }

    /// Access the underlying channel store (for handler integration).
    pub fn store(&self) -> Arc<dyn SessionStore> {
        self.store.clone()
    }

    /// Mid-session settlement: submit the highest voucher on-chain, channel stays open.
    pub async fn settle(&self, channel_id: &str) -> Result<SessionReceipt, VerificationError> {
        let receipt = self
            .sa_client
            .session_settle(channel_id)
            .await
            .map_err(to_verification_error)?;
        self.store
            .put(ChannelRecord::from_receipt(receipt.clone()))
            .await;
        Ok(receipt)
    }

    /// Read-only channel status query.
    pub async fn status(&self, channel_id: &str) -> Result<ChannelStatus, VerificationError> {
        self.sa_client
            .session_status(channel_id)
            .await
            .map_err(to_verification_error)
    }

    /// Start or reset the idle timer for a channel.
    ///
    /// On timeout: call `/session/settle` (no client signature required) and
    /// clean up local state. Registration is synchronous (we hold the map lock
    /// until the entry is in place) so concurrent readers see the timer
    /// immediately after this returns.
    async fn touch_timer(&self, channel_id: &str) {
        if channel_id.is_empty() {
            return;
        }
        let channel_id = channel_id.to_string();
        let sa_client = self.sa_client.clone();
        let store = self.store.clone();
        let timers = self.timers.clone();
        let idle_timeout = self.idle_timeout;

        // Synchronously register (or reset) the timer.
        let reset_tx = {
            let mut timers_lock = self.timers.lock().await;
            if let Some(timer) = timers_lock.get(&channel_id) {
                let _ = timer.reset_tx.send(());
                return;
            }
            let (reset_tx, _rx) = tokio::sync::watch::channel(());
            timers_lock.insert(
                channel_id.clone(),
                ChannelTimer {
                    reset_tx: reset_tx.clone(),
                },
            );
            reset_tx
        };

        tokio::spawn(async move {
            let mut reset_rx = reset_tx.subscribe();
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(idle_timeout) => {
                        tracing::info!(%channel_id, "session idle timeout, auto-settling");
                        match sa_client.session_settle(&channel_id).await {
                            Ok(_) => tracing::info!(%channel_id, "auto-settle ok; SA 24h fallback will close"),
                            Err(e) => tracing::warn!(%channel_id, error = ?e, "auto-settle failed"),
                        }
                        store.remove(&channel_id).await;
                        timers.lock().await.remove(&channel_id);
                        return;
                    }
                    _ = reset_rx.changed() => continue,
                }
            }
        });
    }

    async fn remove_timer(&self, channel_id: &str) {
        self.timers.lock().await.remove(channel_id);
    }

    async fn record_receipt(&self, receipt: &SessionReceipt) {
        self.store
            .put(ChannelRecord::from_receipt(receipt.clone()))
            .await;
    }
}

fn to_verification_error(err: crate::error::SaApiError) -> VerificationError {
    let problem = err.to_problem_details(None);
    VerificationError::new(problem.detail)
}

fn extract_str<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

impl SessionMethod for EvmSessionMethod {
    fn method(&self) -> &str {
        "evm"
    }

    fn challenge_method_details(&self) -> Option<serde_json::Value> {
        self.method_details.clone()
    }

    fn verify_session(
        &self,
        credential: &PaymentCredential,
        _request: &SessionRequest,
    ) -> impl Future<Output = Result<Receipt, VerificationError>> + Send {
        let sa_client = self.sa_client.clone();
        let credential = credential.clone();
        let challenge_id = credential.challenge.id.clone();
        let this = self.clone();

        async move {
            let credential_json = serde_json::to_value(&credential)
                .map_err(|e| VerificationError::new(format!("serialize credential: {}", e)))?;

            let action = extract_str(&credential.payload, "action");
            let payload_channel_id = extract_str(&credential.payload, "channelId");

            let sa_result: Result<SessionReceipt, crate::error::SaApiError> = match action {
                ACTION_OPEN => sa_client.session_open(&credential_json).await,
                ACTION_VOUCHER => sa_client.session_voucher(&credential_json).await,
                ACTION_TOPUP => sa_client.session_top_up(&credential_json).await,
                ACTION_CLOSE => sa_client.session_close(&credential_json).await,
                other => {
                    return Err(VerificationError::new(format!(
                        "unknown session action: {:?}",
                        other
                    )));
                }
            };

            let receipt = sa_result.map_err(|sa_err| {
                let problem = sa_err.to_problem_details(Some(&challenge_id));
                VerificationError::new(problem.detail)
            })?;

            // Side-effects per action.
            match action {
                ACTION_OPEN | ACTION_VOUCHER | ACTION_TOPUP => {
                    this.record_receipt(&receipt).await;
                    let cid = if receipt.channel_id.is_empty() {
                        payload_channel_id
                    } else {
                        &receipt.channel_id
                    };
                    this.touch_timer(cid).await;
                }
                ACTION_CLOSE => {
                    this.record_receipt(&receipt).await;
                    let cid = if receipt.channel_id.is_empty() {
                        payload_channel_id
                    } else {
                        &receipt.channel_id
                    };
                    this.remove_timer(cid).await;
                }
                _ => unreachable!(),
            }

            let ref_for_receipt = receipt
                .reference
                .clone()
                .unwrap_or_else(|| receipt.channel_id.clone());
            Ok(Receipt::success("evm", ref_for_receipt))
        }
    }

    fn respond(
        &self,
        credential: &PaymentCredential,
        _receipt: &Receipt,
    ) -> Option<serde_json::Value> {
        // Voucher is a content request — caller emits business payload. Only
        // management actions (open/topUp/close) get a management-response body.
        let action = extract_str(&credential.payload, "action");
        if matches!(action, ACTION_OPEN | ACTION_TOPUP | ACTION_CLOSE) {
            let channel_id = extract_str(&credential.payload, "channelId");
            // Best-effort: return the latest stored SessionReceipt. Falls back
            // to a minimal shape if the store has no record (e.g., custom
            // SessionStore that chose not to persist).
            let store = self.store.clone();
            let channel_id = channel_id.to_string();
            let action_owned = action.to_string();
            // `respond` is synchronous — use try_lock via tokio blocking only
            // if a runtime exists. Safe minimal approach: block on read.
            let record = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::try_current()
                    .ok()
                    .and_then(|h| h.block_on(store.get(&channel_id)))
            });

            let body = match record {
                Some(r) => serde_json::to_value(&r.last_receipt).unwrap_or_else(
                    |_| serde_json::json!({ "action": action_owned, "status": "ok" }),
                ),
                None => serde_json::json!({
                    "action": action_owned,
                    "status": "ok",
                    "channelId": channel_id,
                }),
            };
            Some(body)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SaApiError;
    use crate::types::ChannelStatus;
    use async_trait::async_trait;
    use mpp::protocol::core::{Base64UrlJson, ChallengeEcho};
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct MockSa {
        settle_hits: StdMutex<Vec<String>>,
        open_count: StdMutex<u32>,
        voucher_count: StdMutex<u32>,
        close_count: StdMutex<u32>,
        next_error: StdMutex<Option<SaApiError>>,
    }

    fn session_receipt(channel_id: &str, accepted: &str) -> SessionReceipt {
        SessionReceipt {
            method: "evm".into(),
            intent: "session".into(),
            status: "success".into(),
            timestamp: "2026-04-01T12:00:00Z".into(),
            chain_id: 196,
            challenge_id: "ch-1".into(),
            channel_id: channel_id.into(),
            accepted_cumulative: accepted.into(),
            spent: None,
            reference: Some(format!("0xtx-{}", accepted)),
            confirmations: None,
            units: None,
        }
    }

    #[async_trait]
    impl SaApiClient for MockSa {
        async fn charge_settle(
            &self,
            _c: &serde_json::Value,
        ) -> Result<crate::types::ChargeReceipt, SaApiError> {
            unreachable!()
        }
        async fn charge_verify_hash(
            &self,
            _c: &serde_json::Value,
        ) -> Result<crate::types::ChargeReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_open(&self, c: &serde_json::Value) -> Result<SessionReceipt, SaApiError> {
            *self.open_count.lock().unwrap() += 1;
            if let Some(e) = self.next_error.lock().unwrap().take() {
                return Err(e);
            }
            let cid = c["payload"]["channelId"].as_str().unwrap_or("0xopen");
            Ok(session_receipt(cid, "0"))
        }
        async fn session_voucher(
            &self,
            c: &serde_json::Value,
        ) -> Result<SessionReceipt, SaApiError> {
            *self.voucher_count.lock().unwrap() += 1;
            let cid = c["payload"]["channelId"].as_str().unwrap_or("");
            let cum = c["payload"]["cumulativeAmount"].as_str().unwrap_or("0");
            Ok(session_receipt(cid, cum))
        }
        async fn session_top_up(
            &self,
            c: &serde_json::Value,
        ) -> Result<SessionReceipt, SaApiError> {
            let cid = c["payload"]["channelId"].as_str().unwrap_or("");
            Ok(session_receipt(cid, "0"))
        }
        async fn session_settle(&self, cid: &str) -> Result<SessionReceipt, SaApiError> {
            self.settle_hits.lock().unwrap().push(cid.into());
            Ok(session_receipt(cid, "500"))
        }
        async fn session_close(&self, c: &serde_json::Value) -> Result<SessionReceipt, SaApiError> {
            *self.close_count.lock().unwrap() += 1;
            let cid = c["payload"]["channelId"].as_str().unwrap_or("");
            Ok(SessionReceipt {
                spent: Some("500".into()),
                ..session_receipt(cid, "500")
            })
        }
        async fn session_status(&self, _cid: &str) -> Result<ChannelStatus, SaApiError> {
            unreachable!()
        }
    }

    fn credential_with(
        action: &str,
        channel_id: &str,
        extra: serde_json::Value,
    ) -> PaymentCredential {
        let mut payload = serde_json::json!({
            "action": action,
            "channelId": channel_id,
        });
        if let Some(obj) = payload.as_object_mut() {
            if let serde_json::Value::Object(ex) = extra {
                for (k, v) in ex {
                    obj.insert(k, v);
                }
            }
        }
        PaymentCredential {
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
            payload,
        }
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

    #[tokio::test(flavor = "multi_thread")]
    async fn open_records_receipt_and_starts_timer() {
        let mock = Arc::new(MockSa::default());
        let method = EvmSessionMethod::new(mock.clone());
        let cred = credential_with("open", "0xabc", serde_json::json!({}));
        method
            .verify_session(&cred, &dummy_request())
            .await
            .unwrap();
        assert_eq!(*mock.open_count.lock().unwrap(), 1);
        let rec = method.store.get("0xabc").await.unwrap();
        assert_eq!(rec.channel_id, "0xabc");
        assert_eq!(rec.accepted_cumulative, "0");
        assert!(method.timers.lock().await.contains_key("0xabc"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn voucher_updates_cumulative() {
        let mock = Arc::new(MockSa::default());
        let method = EvmSessionMethod::new(mock);
        let cred = credential_with(
            "voucher",
            "0xabc",
            serde_json::json!({ "cumulativeAmount": "250", "signature": "0xsig" }),
        );
        method
            .verify_session(&cred, &dummy_request())
            .await
            .unwrap();
        let rec = method.store.get("0xabc").await.unwrap();
        assert_eq!(rec.accepted_cumulative, "250");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn close_records_receipt_and_removes_timer() {
        let mock = Arc::new(MockSa::default());
        let method = EvmSessionMethod::new(mock.clone());

        // Open first to install a timer
        let open_cred = credential_with("open", "0xabc", serde_json::json!({}));
        method
            .verify_session(&open_cred, &dummy_request())
            .await
            .unwrap();
        assert!(method.timers.lock().await.contains_key("0xabc"));

        let close_cred = credential_with(
            "close",
            "0xabc",
            serde_json::json!({ "cumulativeAmount": "500", "signature": "0xsig" }),
        );
        method
            .verify_session(&close_cred, &dummy_request())
            .await
            .unwrap();
        assert_eq!(*mock.close_count.lock().unwrap(), 1);
        assert!(!method.timers.lock().await.contains_key("0xabc"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn idle_timeout_calls_settle_not_close() {
        let mock = Arc::new(MockSa::default());
        // Short timeout so the test runs fast.
        let method =
            EvmSessionMethod::new(mock.clone()).with_idle_timeout(Duration::from_millis(50));
        let cred = credential_with("open", "0xidle", serde_json::json!({}));
        method
            .verify_session(&cred, &dummy_request())
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(200)).await;

        // settle must have fired for the idle channel…
        assert!(
            mock.settle_hits
                .lock()
                .unwrap()
                .iter()
                .any(|c| c == "0xidle"),
            "settle was not called on idle timeout"
        );
        // …and auto-close must NOT have been called with an empty signature.
        assert_eq!(*mock.close_count.lock().unwrap(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn respond_returns_stored_receipt_for_open() {
        let mock = Arc::new(MockSa::default());
        let method = EvmSessionMethod::new(mock);
        let cred = credential_with("open", "0xresp", serde_json::json!({}));
        let receipt = method
            .verify_session(&cred, &dummy_request())
            .await
            .unwrap();
        let body = method
            .respond(&cred, &receipt)
            .expect("open should have body");
        assert_eq!(body["channelId"], "0xresp");
        assert_eq!(body["method"], "evm");
        assert_eq!(body["intent"], "session");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn respond_returns_none_for_voucher() {
        let mock = Arc::new(MockSa::default());
        let method = EvmSessionMethod::new(mock);
        let cred = credential_with(
            "voucher",
            "0xresp",
            serde_json::json!({ "cumulativeAmount": "100", "signature": "0xs" }),
        );
        let receipt = method
            .verify_session(&cred, &dummy_request())
            .await
            .unwrap();
        assert!(method.respond(&cred, &receipt).is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn with_escrow_builder_sets_default_chain_id() {
        let mock = Arc::new(MockSa::default());
        let method = EvmSessionMethod::new(mock).with_escrow("0xescrowaddr");
        let details = method.challenge_method_details().unwrap();
        assert_eq!(details["chainId"], 196);
        assert_eq!(details["escrowContract"], "0xescrowaddr");
        assert!(details.get("channelId").is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unknown_action_errors() {
        let mock = Arc::new(MockSa::default());
        let method = EvmSessionMethod::new(mock);
        let cred = credential_with("dance", "0xabc", serde_json::json!({}));
        let err = method
            .verify_session(&cred, &dummy_request())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown session action"));
    }
}
