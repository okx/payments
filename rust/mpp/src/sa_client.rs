//! SaApiClient trait + default OKX SA API implementation.
//!
//! The trait is pluggable: developers can provide their own implementation.
//! Default implementation calls OKX SA API with HMAC authentication.
//!
//! Source: [Pay] MPP EVM API 方案 — 10 endpoints under /api/v6/pay/mpp/

use crate::error::SaApiError;
use crate::types::{
    ChannelStatus, ChargeReceipt, CloseRequestPayload, SaApiResponse, SessionReceipt,
    SettleRequestPayload,
};
use async_trait::async_trait;

// ==================== Trait (Pluggable Interface) ====================

/// Pluggable SA API client interface.
///
/// Developers can implement this trait to customize the backend.
/// Default implementation: [`OkxSaApiClient`].
#[async_trait]
pub trait SaApiClient: Send + Sync {
    // Charge
    async fn charge_settle(
        &self,
        credential: &serde_json::Value,
    ) -> Result<ChargeReceipt, SaApiError>;
    async fn charge_verify_hash(
        &self,
        credential: &serde_json::Value,
    ) -> Result<ChargeReceipt, SaApiError>;

    // Session
    //
    // ⚠ DRAFT 2 改动（与上一版 trait 不同）：
    //   - 删除 session_voucher（端点废弃，SDK 不再调用，本地 submit_voucher 处理）
    //   - session_settle 改接强类型 SettleRequestPayload（带 voucherSig + payeeSig
    //     + nonce + deadline）
    //   - session_close 改接强类型 CloseRequestPayload（同上）
    //   - 不再有 challenge wrapper（请求 body 直接是 payload 字段）
    async fn session_open(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError>;
    async fn session_top_up(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError>;
    async fn session_settle(
        &self,
        payload: &SettleRequestPayload,
    ) -> Result<SessionReceipt, SaApiError>;
    async fn session_close(
        &self,
        payload: &CloseRequestPayload,
    ) -> Result<SessionReceipt, SaApiError>;
    async fn session_status(&self, channel_id: &str) -> Result<ChannelStatus, SaApiError>;
}

// ==================== Default Implementation (OKX SA API) ====================

/// Default OKX SA API client with HMAC authentication.
///
/// ```rust,no_run
/// use mpp_evm::OkxSaApiClient;
///
/// let client = OkxSaApiClient::new(
///     "your-api-key".into(),
///     "your-secret-key".into(),
///     "your-passphrase".into(),
/// );
///
/// // Or with custom base URL (for sandbox/testing)
/// let client = OkxSaApiClient::with_base_url(
///     "https://sandbox.okx.com".into(),
///     "your-api-key".into(),
///     "your-secret-key".into(),
///     "your-passphrase".into(),
/// );
/// ```
#[derive(Debug, Clone)]
pub struct OkxSaApiClient {
    base_url: String,
    api_key: String,
    secret_key: String,
    passphrase: String,
    client: reqwest::Client,
}

impl OkxSaApiClient {
    /// Create with default production URL.
    pub fn new(api_key: String, secret_key: String, passphrase: String) -> Self {
        Self::with_base_url(
            "https://web3.okx.com".into(),
            api_key,
            secret_key,
            passphrase,
        )
    }

    /// Create with custom base URL (for testing/sandbox).
    pub fn with_base_url(
        base_url: String,
        api_key: String,
        secret_key: String,
        passphrase: String,
    ) -> Self {
        Self {
            base_url,
            api_key,
            secret_key,
            passphrase,
            client: reqwest::Client::new(),
        }
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, SaApiError> {
        let url = format!("{}{}", self.base_url, path);
        let body_str = serde_json::to_string(body)
            .map_err(|e| SaApiError::new(8000, format!("serialize error: {}", e)))?;

        // Debug-level request log. Body contains credential (on-chain-public EIP-3009
        // sig + amounts). Auth headers (API key / sign / passphrase / timestamp) are
        // intentionally NOT logged. Enable with `RUST_LOG=mpp_evm=debug`.
        tracing::debug!(
            target: "mpp_evm::sa",
            method = "POST",
            %url,
            body = %body_str,
            "SA API → request"
        );

        let headers = build_auth_headers(
            &self.api_key,
            &self.secret_key,
            &self.passphrase,
            "POST",
            path,
            &body_str,
        )?;

        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(target: "mpp_evm::sa", %url, error = %e, "SA API ← transport error");
                SaApiError::new(8000, format!("request failed: {}", e))
            })?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| {
            tracing::warn!(target: "mpp_evm::sa", %url, %status, error = %e, "SA API ← body read error");
            SaApiError::new(8000, format!("read response failed: {}", e))
        })?;
        tracing::debug!(
            target: "mpp_evm::sa",
            method = "POST",
            %url,
            %status,
            bytes = text.len(),
            body = %text,
            "SA API ← response"
        );

        parse_sa_response::<T>(&url, status, &text)
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, SaApiError> {
        let url = format!("{}{}", self.base_url, path);

        tracing::debug!(
            target: "mpp_evm::sa",
            method = "GET",
            %url,
            "SA API → request"
        );

        let headers = build_auth_headers(
            &self.api_key,
            &self.secret_key,
            &self.passphrase,
            "GET",
            path,
            "",
        )?;

        let resp = self
            .client
            .get(&url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(target: "mpp_evm::sa", %url, error = %e, "SA API ← transport error");
                SaApiError::new(8000, format!("request failed: {}", e))
            })?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| {
            tracing::warn!(target: "mpp_evm::sa", %url, %status, error = %e, "SA API ← body read error");
            SaApiError::new(8000, format!("read response failed: {}", e))
        })?;
        tracing::debug!(
            target: "mpp_evm::sa",
            method = "GET",
            %url,
            %status,
            bytes = text.len(),
            body = %text,
            "SA API ← response"
        );

        parse_sa_response::<T>(&url, status, &text)
    }
}

/// Parse SA API envelope, with raw body captured into the error message so
/// HTTP 5xx / non-JSON / malformed envelope问题能在错误里直接看到原文。
///
/// 两阶段反序列化：先只抽 `code / msg`（`data` 用 `serde_json::Value` 兜住），
/// `code != 0` 直接走业务错误分支，不去 parse `data`。这是必要的 —— SA 在
/// 业务失败时 `data` 常为 `{}`（空对象而不是 `null`），如果一次性 parse 成
/// `SaApiResponse<T>` 会因 `T` 的必填字段缺失而失败，把真实 `code/msg` 吞掉。
fn parse_sa_response<T: serde::de::DeserializeOwned>(
    url: &str,
    status: reqwest::StatusCode,
    text: &str,
) -> Result<T, SaApiError> {
    // Phase 1: parse envelope with `data: Value` 兜底，任何 T 的 shape 都不会让 envelope 挂掉。
    let envelope: SaApiResponse<serde_json::Value> = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            let preview: String = text.chars().take(512).collect();
            tracing::warn!(
                target: "mpp_evm::sa",
                %url,
                %status,
                error = %e,
                body_preview = %preview,
                "SA API ← envelope parse failed"
            );
            return Err(SaApiError::new(
                8000,
                format!("parse envelope failed (status={}): {}: {}", status, e, preview),
            ));
        }
    };

    // Phase 2: 按 code 分流。业务错误在这里返回，不再触碰 data。
    if envelope.code != 0 {
        tracing::info!(
            target: "mpp_evm::sa",
            %url,
            %status,
            code = envelope.code,
            msg = %envelope.msg,
            "SA API ← business error"
        );
        return Err(SaApiError::new(envelope.code, envelope.msg));
    }

    // Phase 3: code == 0 时才 deserialize data 成 T。`data` 为 null / 不存在 → empty-data 错误。
    let data_value = envelope.data.ok_or_else(|| {
        tracing::warn!(target: "mpp_evm::sa", %url, %status, "SA API ← empty data in ok response");
        SaApiError::new(8000, "empty data in response")
    })?;
    serde_json::from_value::<T>(data_value.clone()).map_err(|e| {
        let preview: String = data_value.to_string().chars().take(512).collect();
        tracing::warn!(
            target: "mpp_evm::sa",
            %url,
            %status,
            error = %e,
            data_preview = %preview,
            "SA API ← data shape mismatch (code=0 but data missing required fields)"
        );
        SaApiError::new(
            8000,
            format!("parse data failed (status={}, code=0): {}: {}", status, e, preview),
        )
    })
}

#[async_trait]
impl SaApiClient for OkxSaApiClient {
    async fn charge_settle(
        &self,
        credential: &serde_json::Value,
    ) -> Result<ChargeReceipt, SaApiError> {
        self.post("/api/v6/pay/mpp/charge/settle", credential).await
    }

    async fn charge_verify_hash(
        &self,
        credential: &serde_json::Value,
    ) -> Result<ChargeReceipt, SaApiError> {
        self.post("/api/v6/pay/mpp/charge/verifyHash", credential)
            .await
    }

    async fn session_open(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError> {
        self.post("/api/v6/pay/mpp/session/open", credential).await
    }

    async fn session_top_up(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError> {
        self.post("/api/v6/pay/mpp/session/topUp", credential).await
    }

    async fn session_settle(
        &self,
        payload: &SettleRequestPayload,
    ) -> Result<SessionReceipt, SaApiError> {
        // SA API spec body 形状: { "payload": <SettleRequestPayload> }
        // (跟 close 一样,外层 wrap 一个 payload 字段。HTML reference 工具同此结构)
        let body = serde_json::json!({
            "payload": serde_json::to_value(payload)
                .map_err(|e| SaApiError::new(8000, format!("serialize SettleRequestPayload: {e}")))?
        });
        self.post("/api/v6/pay/mpp/session/settle", &body).await
    }

    async fn session_close(
        &self,
        payload: &CloseRequestPayload,
    ) -> Result<SessionReceipt, SaApiError> {
        // SA API spec body 形状: { "payload": <CloseRequestPayload> }
        let body = serde_json::json!({
            "payload": serde_json::to_value(payload)
                .map_err(|e| SaApiError::new(8000, format!("serialize CloseRequestPayload: {e}")))?
        });
        self.post("/api/v6/pay/mpp/session/close", &body).await
    }

    async fn session_status(&self, channel_id: &str) -> Result<ChannelStatus, SaApiError> {
        let path = format!("/api/v6/pay/mpp/session/status?channelId={}", channel_id);
        self.get(&path).await
    }
}

// ==================== OKX HMAC Auth ====================

/// OKX HMAC auth header names (exposed for tests / advanced tooling).
pub const HEADER_API_KEY: &str = "OK-ACCESS-KEY";
pub const HEADER_SIGN: &str = "OK-ACCESS-SIGN";
pub const HEADER_TIMESTAMP: &str = "OK-ACCESS-TIMESTAMP";
pub const HEADER_PASSPHRASE: &str = "OK-ACCESS-PASSPHRASE";

/// Build OKX HMAC-SHA256 authentication headers.
/// Message format: {timestamp}{METHOD}{request_path}{body}
fn build_auth_headers(
    api_key: &str,
    secret_key: &str,
    passphrase: &str,
    method: &str,
    request_path: &str,
    body: &str,
) -> Result<reqwest::header::HeaderMap, SaApiError> {
    use base64::Engine;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let message = format!("{}{}{}{}", timestamp, method, request_path, body);

    let mut mac = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes())
        .map_err(|e| SaApiError::new(8000, format!("HMAC error: {}", e)))?;
    mac.update(message.as_bytes());
    let signature = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(HEADER_API_KEY, api_key.parse().unwrap());
    headers.insert(HEADER_SIGN, signature.parse().unwrap());
    headers.insert(HEADER_TIMESTAMP, timestamp.parse().unwrap());
    headers.insert(HEADER_PASSPHRASE, passphrase.parse().unwrap());

    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mk_client(base_url: String) -> OkxSaApiClient {
        OkxSaApiClient::with_base_url(
            base_url,
            "test-key".into(),
            "test-secret".into(),
            "test-passphrase".into(),
        )
    }

    #[tokio::test]
    async fn charge_settle_posts_to_correct_path_with_auth_headers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v6/pay/mpp/charge/settle"))
            .and(header_exists(HEADER_API_KEY))
            .and(header_exists(HEADER_SIGN))
            .and(header_exists(HEADER_TIMESTAMP))
            .and(header_exists(HEADER_PASSPHRASE))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {
                    "method": "evm",
                    "reference": "0xtx",
                    "status": "success",
                    "timestamp": "2026-04-01T12:00:00Z",
                    "chainId": 196
                },
                "msg": ""
            })))
            .mount(&server)
            .await;

        let client = mk_client(server.uri());
        let receipt = client
            .charge_settle(&serde_json::json!({"payload":{"type":"transaction"}}))
            .await
            .unwrap();
        assert_eq!(receipt.reference, "0xtx");
        assert_eq!(receipt.chain_id, 196);
    }

    #[tokio::test]
    async fn non_zero_code_maps_to_sa_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v6/pay/mpp/charge/verifyHash"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 70004,
                "data": null,
                "msg": "invalid signature"
            })))
            .mount(&server)
            .await;

        let client = mk_client(server.uri());
        let err = client
            .charge_verify_hash(&serde_json::json!({"payload":{"type":"hash"}}))
            .await
            .unwrap_err();
        assert_eq!(err.code, 70004);
        assert!(err.msg.contains("invalid signature"));
    }

    #[tokio::test]
    async fn session_status_uses_query_string_get() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v6/pay/mpp/session/status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {
                    "channelId": "0xabc",
                    "payer": "0xp", "payee": "0xq", "token": "0xt",
                    "deposit": "10000", "cumulativeAmount": "1000",
                    "settledOnChain": "500", "sessionStatus": "OPEN",
                    "remainingBalance": "9000"
                },
                "msg": ""
            })))
            .mount(&server)
            .await;

        let client = mk_client(server.uri());
        let status = client.session_status("0xabc").await.unwrap();
        assert_eq!(status.channel_id, "0xabc");
        assert_eq!(status.session_status, "OPEN");
    }

    /// SA 实际在业务错误时 `data: {}`（空对象，不是 null）。两阶段 parse 必须
    /// 先 route 到业务错误分支，而不是尝试把 `{}` 反序列化成 `ChargeReceipt`
    /// （必填 `method` 字段缺失）导致真实 code/msg 被吞。
    #[tokio::test]
    async fn business_error_with_empty_object_data_still_returns_code_and_msg() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v6/pay/mpp/charge/settle"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 70003,
                "data": {},
                "msg": "authorization.value does not match expected primary amount. authValue=100 expected=50"
            })))
            .mount(&server)
            .await;

        let client = mk_client(server.uri());
        let err = client
            .charge_settle(&serde_json::json!({"payload":{"type":"transaction"}}))
            .await
            .unwrap_err();
        assert_eq!(err.code, 70003);
        assert!(err.msg.contains("authValue=100 expected=50"), "msg was: {}", err.msg);
    }

    #[tokio::test]
    async fn empty_data_yields_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v6/pay/mpp/session/settle"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0, "data": null, "msg": ""
            })))
            .mount(&server)
            .await;
        let client = mk_client(server.uri());
        let payload = SettleRequestPayload {
            action: Some("settle".into()),
            channel_id: "0xabc".into(),
            cumulative_amount: "100".into(),
            voucher_signature: "0xv".into(),
            payee_signature: "0xp".into(),
            nonce: "1".into(),
            deadline: "999".into(),
        };
        let err = client.session_settle(&payload).await.unwrap_err();
        assert!(err.msg.contains("empty data"));
    }

    #[tokio::test]
    async fn session_settle_posts_payload_as_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v6/pay/mpp/session/settle"))
            .and(wiremock::matchers::body_json(serde_json::json!({
                "action": "settle",
                "channelId": "0xabc",
                "cumulativeAmount": "250",
                "voucherSignature": "0xvsig",
                "payeeSignature": "0xpsig",
                "nonce": "1234",
                "deadline": "9999"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {
                    "method": "evm",
                    "intent": "session",
                    "status": "success",
                    "timestamp": "2026-04-01T12:00:00Z",
                    "chainId": 196,
                    "channelId": "0xabc",
                    "deposit": "10000"
                },
                "msg": ""
            })))
            .mount(&server)
            .await;
        let client = mk_client(server.uri());
        let payload = SettleRequestPayload {
            action: Some("settle".into()),
            channel_id: "0xabc".into(),
            cumulative_amount: "250".into(),
            voucher_signature: "0xvsig".into(),
            payee_signature: "0xpsig".into(),
            nonce: "1234".into(),
            deadline: "9999".into(),
        };
        let receipt = client.session_settle(&payload).await.unwrap();
        assert_eq!(receipt.channel_id, "0xabc");
        assert_eq!(receipt.deposit.as_deref(), Some("10000"));
    }
}
