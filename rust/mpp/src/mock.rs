//! MockSaApiClient —— 固定成功的 SA API 桩，用于本地 dev / 端到端流程演示。
//!
//! **不要在生产环境使用。** 这个 client:
//! - 对所有 `SaApiClient` 方法返回 `Ok(...)` 固定结构
//! - 不发任何网络请求
//! - `reference` / `challenge_id` / `timestamp` 是可识别的假值（含 `MOCK` 字样）
//!
//! 典型用法：
//!
//! ```
//! use std::sync::Arc;
//! use mpp_evm::{EvmChargeMethod, MockSaApiClient};
//!
//! let client = Arc::new(MockSaApiClient::default());
//! let charge_method = EvmChargeMethod::new(client);
//! // ... 把 charge_method 挂到 axum handler 上
//! ```

use async_trait::async_trait;

use crate::error::SaApiError;
use crate::sa_client::SaApiClient;
use crate::types::{ChannelStatus, ChargeReceipt, SessionReceipt};

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

/// 生成可识别的 mock 时间戳（固定串，不依赖系统时钟，便于测试）。
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

fn mock_session_receipt(credential: &serde_json::Value, intent: &str) -> SessionReceipt {
    SessionReceipt {
        method: "evm".into(),
        intent: intent.into(),
        status: "success".into(),
        timestamp: mock_timestamp(),
        chain_id: 196,
        challenge_id: extract_challenge_id(credential),
        channel_id: "mock-channel-id".into(),
        accepted_cumulative: "0".into(),
        spent: None,
        reference: None,
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
        Ok(mock_session_receipt(credential, "session.open"))
    }

    async fn session_voucher(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError> {
        // voucher 通常累加 accepted_cumulative; 这里固定成一个示例值
        let mut r = mock_session_receipt(credential, "session.voucher");
        r.accepted_cumulative = "100000".into();
        Ok(r)
    }

    async fn session_top_up(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError> {
        let mut r = mock_session_receipt(credential, "session.topUp");
        r.accepted_cumulative = "100000".into();
        Ok(r)
    }

    async fn session_settle(&self, channel_id: &str) -> Result<SessionReceipt, SaApiError> {
        let credential = serde_json::json!({
            "challenge": { "id": format!("mock-settle-{channel_id}") }
        });
        let mut r = mock_session_receipt(&credential, "session.settle");
        r.channel_id = channel_id.to_string();
        r.reference =
            Some("0xMOCK_SETTLE_TX_HASH_0000000000000000000000000000000000000000000000000000".into());
        r.spent = Some("100000".into());
        r.confirmations = Some(1);
        Ok(r)
    }

    async fn session_close(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError> {
        let mut r = mock_session_receipt(credential, "session.close");
        r.spent = Some("100000".into());
        r.reference =
            Some("0xMOCK_CLOSE_TX_HASH_00000000000000000000000000000000000000000000000000000".into());
        Ok(r)
    }

    async fn session_status(&self, channel_id: &str) -> Result<ChannelStatus, SaApiError> {
        Ok(ChannelStatus {
            channel_id: channel_id.to_string(),
            payer: "0xMOCK_PAYER_ADDRESS_00000000000000000000".into(),
            payee: "0xMOCK_PAYEE_ADDRESS_00000000000000000000".into(),
            token: "0xMOCK_TOKEN_ADDRESS_00000000000000000000".into(),
            deposit: "1000000000".into(),
            cumulative_amount: "100000".into(),
            settled_on_chain: "0".into(),
            session_status: "active".into(),
            remaining_balance: "999900000".into(),
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
    async fn session_voucher_has_accepted_cumulative() {
        let client = MockSaApiClient::new();
        let cred = serde_json::json!({"challenge": {"id": "ch-v-1"}});
        let r = client.session_voucher(&cred).await.unwrap();
        assert_eq!(r.intent, "session.voucher");
        assert_eq!(r.accepted_cumulative, "100000");
    }

    #[tokio::test]
    async fn session_settle_echoes_channel_id() {
        let client = MockSaApiClient::new();
        let r = client.session_settle("ch-abc").await.unwrap();
        assert_eq!(r.channel_id, "ch-abc");
        assert!(r.reference.is_some());
        assert_eq!(r.spent.as_deref(), Some("100000"));
    }

    #[tokio::test]
    async fn session_status_returns_active() {
        let client = MockSaApiClient::new();
        let s = client.session_status("ch-xyz").await.unwrap();
        assert_eq!(s.channel_id, "ch-xyz");
        assert_eq!(s.session_status, "active");
        assert_eq!(s.deposit, "1000000000");
    }
}
