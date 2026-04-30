//! SaApiClient trait + default OKX SA API implementation.
//!
//! The trait is pluggable: developers can provide their own implementation.
//! Default implementation calls OKX SA API with HMAC authentication.
//!
//! Source: [Pay] MPP EVM API plan — 10 endpoints under /api/v6/pay/mpp/

use std::time::Duration;

use crate::error::SaApiError;
use crate::types::{
    ChannelStatus, ChargeReceipt, CloseRequestPayload, SaApiResponse, SessionReceipt,
    SettleRequestPayload,
};
use async_trait::async_trait;

/// HTTP request timeout for SA API calls.
///
/// Caps the wait time when the SA backend hangs. 30s leaves room for
/// on-chain receipts (open / topup are broadcast and mined server-side,
/// typically 5-15s). On network failures we cut at 30s to keep the
/// failure window bounded, so tokio workers don't pin forever and faults
/// don't cascade.
const SA_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

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

    // Session endpoints.
    //
    // - No `/session/voucher` endpoint (deprecated); vouchers are
    //   processed locally in the SDK (`EvmSessionMethod::submit_voucher`).
    // - `session_settle` / `session_close` take a strongly-typed payload
    //   (voucherSig + payeeSig + nonce + deadline); the request body is
    //   the payload fields directly (no challenge wrapper).
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
        let client = reqwest::Client::builder()
            .timeout(SA_HTTP_TIMEOUT)
            .build()
            .expect("reqwest::Client::builder() with default config should not fail");
        Self {
            base_url,
            api_key,
            secret_key,
            passphrase,
            client,
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

/// Parse SA API envelope, capturing the raw body in the error message so
/// HTTP 5xx / non-JSON / malformed envelope failures show the original
/// text on the wire.
///
/// Two-phase deserialization: first read only `code / msg` (with `data`
/// caught as `serde_json::Value`); if `code != 0`, go straight to the
/// business-error branch without parsing `data`. This is necessary —
/// on business failure SA often sends `data: {}` (empty object, not
/// `null`); a single-shot `SaApiResponse<T>` parse would fail on
/// missing required `T` fields and swallow the real `code/msg`.
fn parse_sa_response<T: serde::de::DeserializeOwned>(
    url: &str,
    status: reqwest::StatusCode,
    text: &str,
) -> Result<T, SaApiError> {
    // Phase 1: parse the envelope with `data: Value` as a catch-all so
    // any `T` shape is decoupled from envelope parsing.
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

    // Phase 2: branch on `code`. Business errors return here without touching `data`.
    if envelope.code != 0 {
        tracing::info!(
            target: "mpp_evm::sa",
            %url,
            %status,
            code = envelope.code,
            msg = %envelope.msg,
            "SA API ← business error"
        );
        // SA backend occasionally returns a negative code (e.g. -1
        // "unknown error"). `SaApiError.code` is u32, so negative / out-of-range
        // values map to 8000 (service error), preserving the original code in `msg`.
        let (mapped_code, mapped_msg) = if envelope.code >= 0
            && envelope.code <= u32::MAX as i64
        {
            (envelope.code as u32, envelope.msg)
        } else {
            (
                8000,
                format!("SA backend code={}: {}", envelope.code, envelope.msg),
            )
        };
        return Err(SaApiError::new(mapped_code, mapped_msg));
    }

    // Phase 3: only deserialize `data` into `T` when `code == 0`.
    //          `data` null / missing → empty-data error.
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
        // SA API spec body shape: { "payload": <SettleRequestPayload> }
        // (same as close — the outer object wraps a `payload` field; the
        // HTML reference tool uses the same structure).
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
        // SA API spec body shape: { "payload": <CloseRequestPayload> }
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
    headers.insert(HEADER_API_KEY, parse_header_value("api_key", api_key)?);
    headers.insert(HEADER_SIGN, parse_header_value("signature", &signature)?);
    headers.insert(HEADER_TIMESTAMP, parse_header_value("timestamp", &timestamp)?);
    headers.insert(HEADER_PASSPHRASE, parse_header_value("passphrase", passphrase)?);

    Ok(headers)
}

/// Convert a string to a `HeaderValue`. Non-ASCII / control characters are
/// reported as `SaApiError(8000)` instead of panicking — guards against
/// misconfigured api_key / passphrase taking the SDK down on every request.
fn parse_header_value(
    name: &str,
    value: &str,
) -> Result<reqwest::header::HeaderValue, SaApiError> {
    value.parse().map_err(|_| {
        SaApiError::new(
            8000,
            format!("non-ASCII or invalid header value for {name}"),
        )
    })
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

    /// On business errors SA actually returns `data: {}` (empty object,
    /// not null). The two-phase parse must route to the business-error
    /// branch first; otherwise it would try to deserialize `{}` into
    /// `ChargeReceipt` (whose required `method` field is missing) and
    /// swallow the real `code/msg`.
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
                "payload": {
                    "action": "settle",
                    "channelId": "0xabc",
                    "cumulativeAmount": "250",
                    "voucherSignature": "0xvsig",
                    "payeeSignature": "0xpsig",
                    "nonce": "1234",
                    "deadline": "9999"
                }
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
