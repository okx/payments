//! SaApiClient trait + default OKX SA API implementation.
//!
//! The trait is pluggable: developers can provide their own implementation.
//! Default implementation calls OKX SA API with HMAC authentication.
//!
//! Source: [Pay] MPP EVM API 方案 — 10 endpoints under /api/v6/pay/mpp/

use crate::error::SaApiError;
use crate::types::{ChannelStatus, ChargeReceipt, SaApiResponse, SessionReceipt};
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
    async fn session_open(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError>;
    async fn session_voucher(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError>;
    async fn session_top_up(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError>;
    async fn session_settle(&self, channel_id: &str) -> Result<SessionReceipt, SaApiError>;
    async fn session_close(
        &self,
        credential: &serde_json::Value,
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
            .map_err(|e| SaApiError::new(8000, format!("request failed: {}", e)))?;

        let sa_resp: SaApiResponse<T> = resp
            .json()
            .await
            .map_err(|e| SaApiError::new(8000, format!("parse response failed: {}", e)))?;

        if sa_resp.code != 0 {
            return Err(SaApiError::new(sa_resp.code, sa_resp.msg));
        }

        sa_resp
            .data
            .ok_or_else(|| SaApiError::new(8000, "empty data in response"))
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, SaApiError> {
        let url = format!("{}{}", self.base_url, path);

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
            .map_err(|e| SaApiError::new(8000, format!("request failed: {}", e)))?;

        let sa_resp: SaApiResponse<T> = resp
            .json()
            .await
            .map_err(|e| SaApiError::new(8000, format!("parse response failed: {}", e)))?;

        if sa_resp.code != 0 {
            return Err(SaApiError::new(sa_resp.code, sa_resp.msg));
        }

        sa_resp
            .data
            .ok_or_else(|| SaApiError::new(8000, "empty data in response"))
    }
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

    async fn session_voucher(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError> {
        self.post("/api/v6/pay/mpp/session/voucher", credential)
            .await
    }

    async fn session_top_up(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError> {
        self.post("/api/v6/pay/mpp/session/topUp", credential).await
    }

    async fn session_settle(&self, channel_id: &str) -> Result<SessionReceipt, SaApiError> {
        let body = serde_json::json!({ "channelId": channel_id });
        self.post("/api/v6/pay/mpp/session/settle", &body).await
    }

    async fn session_close(
        &self,
        credential: &serde_json::Value,
    ) -> Result<SessionReceipt, SaApiError> {
        self.post("/api/v6/pay/mpp/session/close", credential).await
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
        let err = client.session_settle("0xabc").await.unwrap_err();
        assert!(err.msg.contains("empty data"));
    }
}
