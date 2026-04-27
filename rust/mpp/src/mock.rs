//! MockSaApiClient —— 固定成功的 SA API 桩，用于本地 dev / 端到端流程演示。
//!
//! **不要在生产环境使用。** 这个 client:
//! - 对所有 `SaApiClient` 方法返回 `Ok(...)` 固定结构
//! - 不发任何网络请求
//! - `reference` / `timestamp` 是可识别的假值（含 `MOCK` 字样）
//!
//! 典型用法：
//!
//! ```
//! use std::sync::Arc;
//! use mpp_evm::{EvmChargeMethod, MockSaApiClient};
//!
//! let client = Arc::new(MockSaApiClient::default());
//! let charge_method = EvmChargeMethod::new(client);
//! ```

use async_trait::async_trait;

use crate::error::SaApiError;
use crate::sa_client::SaApiClient;
use crate::types::{
    ChannelStatus, ChargeReceipt, CloseRequestPayload, SessionReceipt, SettleRequestPayload,
};

/// 固定返回成功响应的 SA API 桩。
///
/// 默认构造即可使用，无配置项。chain_id 固定 196 (X Layer)。
#[derive(Debug, Clone, Default)]
pub struct MockSaApiClient;

impl MockSaApiClient {
    /// 便利构造。等价 `MockSaApiClient::default()`。
    pub fn new() -> Self {
        Self
    }
}

/// 从 credential JSON 里尽可能抽 challenge_id，失败就给个默认值。
fn extract_challenge_id(credential: &serde_json::Value) -> String {
    credential
        .get("challenge")
        .and_then(|c| c.get("id"))
        .and_then(|id| id.as_str())
        .unwrap_or("mock-challenge-id")
        .to_string()
}

fn extract_channel_id(credential: &serde_json::Value) -> String {
    credential
        .get("payload")
        .and_then(|p| p.get("channelId"))
        .and_then(|id| id.as_str())
        .unwrap_or("0xmockchannelid000000000000000000000000000000000000000000000000")
        .to_string()
}

fn mock_timestamp() -> String {
    "2026-04-22T00:00:00Z".to_string()
}

fn mock_charge_receipt(credential: &serde_json::Value) -> ChargeReceipt {
    ChargeReceipt {
        method: "evm".into(),
        reference: "0xMOCK_TX_HASH_0000000000000000000000000000000000000000000000000000000000".into(),
        status: "success".into(),
        timestamp: mock_timestamp(),
        chain_id: 196,
        confirmations: Some(1),
        challenge_id: Some(extract_challenge_id(credential)),
        external_id: None,
    }
}

fn mock_session_receipt(channel_id: &str, intent: &str) -> SessionReceipt {
    SessionReceipt {
        method: "evm".into(),
        intent: intent.into(),
        status: "success".into(),
        timestamp: mock_timestamp(),
        chain_id: 196,
        channel_id: channel_id.to_string(),
        reference: None,
        deposit: Some("1000000".into()),
        // 旧字段保留 None（DRAFT 2 不返）
        challenge_id: None,
        accepted_cumulative: None,
        spent: None,
        confirmations: None,
        units: None,
    }
}

#[async_trait]
impl SaApiClient for MockSaApiClient {
    // ---------- Charge ----------
    async fn charge_settle(
        &self,
        credential: &serde_json::Value,
    ) -> Result<ChargeReceipt, SaApiError> {
        Ok(mock_charge_receipt(credential))
    }

    async fn charge_verify_hash(
        &self,
        credential: &serde_json::Value,
    ) -> Result<ChargeReceipt, SaApiError> {
        Ok(mock_charge_receipt(credential))
    }

    // ---------- Session ----------
    async fn session_open(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError> {
        let cid = extract_channel_id(credential);
        let mut r = mock_session_receipt(&cid, "session");
        r.reference = Some(
            "0xMOCK_OPEN_TX_HASH_000000000000000000000000000000000000000000000000000000".into(),
        );
        Ok(r)
    }

    async fn session_top_up(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError> {
        let cid = extract_channel_id(credential);
        let mut r = mock_session_receipt(&cid, "session");
        r.deposit = Some("2000000".into()); // mock：topUp 后 deposit 翻倍
        r.reference = Some(
            "0xMOCK_TOPUP_TX_HASH_00000000000000000000000000000000000000000000000000000".into(),
        );
        Ok(r)
    }

    async fn session_settle(
        &self,
        payload: &SettleRequestPayload,
    ) -> Result<SessionReceipt, SaApiError> {
        let mut r = mock_session_receipt(&payload.channel_id, "session");
        r.reference = Some(
            "0xMOCK_SETTLE_TX_HASH_0000000000000000000000000000000000000000000000000000".into(),
        );
        Ok(r)
    }

    async fn session_close(
        &self,
        payload: &CloseRequestPayload,
    ) -> Result<SessionReceipt, SaApiError> {
        let mut r = mock_session_receipt(&payload.channel_id, "session");
        r.reference = Some(
            "0xMOCK_CLOSE_TX_HASH_00000000000000000000000000000000000000000000000000000".into(),
        );
        Ok(r)
    }

    async fn session_status(&self, channel_id: &str) -> Result<ChannelStatus, SaApiError> {
        Ok(ChannelStatus {
            channel_id: channel_id.to_string(),
            payer: "0xMOCK_PAYER_ADDRESS_00000000000000000000".into(),
            payee: "0xMOCK_PAYEE_ADDRESS_00000000000000000000".into(),
            token: "0xMOCK_TOKEN_ADDRESS_00000000000000000000".into(),
            deposit: "1000000000".into(),
            settled_on_chain: "0".into(),
            session_status: "OPEN".into(),
            remaining_balance: "999900000".into(),
            cumulative_amount: None, // DRAFT 2 不返
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn charge_settle_returns_mock_receipt() {
        let client = MockSaApiClient::new();
        let cred = serde_json::json!({
            "challenge": { "id": "ch-test-1" }
        });
        let r = client.charge_settle(&cred).await.unwrap();
        assert_eq!(r.method, "evm");
        assert_eq!(r.status, "success");
        assert_eq!(r.chain_id, 196);
        assert_eq!(r.challenge_id.as_deref(), Some("ch-test-1"));
        assert!(r.reference.contains("MOCK"));
    }

    #[tokio::test]
    async fn charge_settle_without_challenge_id_uses_default() {
        let client = MockSaApiClient::new();
        let cred = serde_json::json!({});
        let r = client.charge_settle(&cred).await.unwrap();
        assert_eq!(r.challenge_id.as_deref(), Some("mock-challenge-id"));
    }

    #[tokio::test]
    async fn session_open_includes_deposit() {
        let client = MockSaApiClient::new();
        let cred = serde_json::json!({"payload": {"channelId": "0xabc"}});
        let r = client.session_open(&cred).await.unwrap();
        assert_eq!(r.channel_id, "0xabc");
        assert!(r.deposit.is_some());
    }

    #[tokio::test]
    async fn session_settle_echoes_channel_id_from_payload() {
        let client = MockSaApiClient::new();
        let payload = SettleRequestPayload {
            action: Some("settle".into()),
            channel_id: "ch-abc".into(),
            cumulative_amount: "100".into(),
            voucher_signature: "0xv".into(),
            payee_signature: "0xp".into(),
            nonce: "1".into(),
            deadline: "999".into(),
        };
        let r = client.session_settle(&payload).await.unwrap();
        assert_eq!(r.channel_id, "ch-abc");
        assert!(r.reference.is_some());
    }

    #[tokio::test]
    async fn session_close_uses_payload_channel_id() {
        let client = MockSaApiClient::new();
        let payload = CloseRequestPayload {
            action: None,
            channel_id: "ch-close".into(),
            cumulative_amount: "500".into(),
            voucher_signature: "0xv".into(),
            payee_signature: "0xp".into(),
            nonce: "1".into(),
            deadline: "999".into(),
        };
        let r = client.session_close(&payload).await.unwrap();
        assert_eq!(r.channel_id, "ch-close");
    }

    #[tokio::test]
    async fn session_status_returns_open_without_cumulative() {
        let client = MockSaApiClient::new();
        let s = client.session_status("ch-xyz").await.unwrap();
        assert_eq!(s.channel_id, "ch-xyz");
        assert_eq!(s.session_status, "OPEN");
        assert!(s.cumulative_amount.is_none());
    }
}
