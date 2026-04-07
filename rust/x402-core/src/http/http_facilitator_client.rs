//! OKX HTTP Facilitator Client implementation.
//!
//! Mirrors: `@x402/core/src/http/httpFacilitatorClient.ts`
//! Extended with OKX HMAC-SHA256 authentication.
//!
//! OKX Facilitator responses are wrapped in `{"code":0, "data": {...}, "msg":""}`.
//! This client automatically unwraps the `data` field.

use async_trait::async_trait;
use reqwest::Client;
use serde::de::DeserializeOwned;

use crate::error::X402Error;
use crate::facilitator::FacilitatorClient;
use crate::http::hmac::build_auth_headers;
use crate::types::{
    SettleRequest, SettleResponse, SettleStatusResponse, SupportedResponse, VerifyRequest,
    VerifyResponse,
};

/// OKX Facilitator API path prefix.
const API_PREFIX: &str = "/api/v6/pay/x402";

/// Default OKX Facilitator URL.
///
/// Mirrors TS: `DEFAULT_FACILITATOR_URL` from `httpFacilitatorClient.ts`
const DEFAULT_FACILITATOR_URL: &str = "https://web3.okx.com";

/// OKX API response wrapper.
/// All OKX Facilitator responses are wrapped in this structure.
///
/// Example: `{"code":0, "data": {...}, "msg":"", "error_code":"0", "error_message":""}`
#[derive(Debug, serde::Deserialize)]
struct OkxApiResponse<T> {
    code: i32,
    data: Option<T>,
    msg: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
}

/// HTTP client for communicating with the OKX x402 Facilitator.
///
/// Mirrors TS: `HTTPFacilitatorClient` from `core/src/http/httpFacilitatorClient.ts`
/// Extended with OKX HMAC-SHA256 signing on every request.
///
/// # Example
/// ```no_run
/// use x402_core::http::OkxHttpFacilitatorClient;
///
/// // With default URL (https://web3.okx.com)
/// let client = OkxHttpFacilitatorClient::new(
///     "your-api-key",
///     "your-secret-key",
///     "your-passphrase",
/// );
///
/// // With custom URL
/// let client = OkxHttpFacilitatorClient::with_url(
///     "https://custom-facilitator.example.com",
///     "your-api-key",
///     "your-secret-key",
///     "your-passphrase",
/// );
/// ```
pub struct OkxHttpFacilitatorClient {
    http: Client,
    base_url: String,
    api_key: String,
    secret_key: String,
    passphrase: String,
}

impl OkxHttpFacilitatorClient {
    /// Create a new OKX Facilitator client with the default URL (`https://web3.okx.com`).
    ///
    /// Mirrors TS: `new HTTPFacilitatorClient()` (uses `DEFAULT_FACILITATOR_URL` when url is omitted)
    ///
    /// # Arguments
    /// - `api_key` - Seller's OKX API key
    /// - `secret_key` - Seller's OKX secret key
    /// - `passphrase` - Seller's OKX passphrase
    pub fn new(
        api_key: &str,
        secret_key: &str,
        passphrase: &str,
    ) -> Self {
        Self::with_url(DEFAULT_FACILITATOR_URL, api_key, secret_key, passphrase)
    }

    /// Create a new OKX Facilitator client with a custom URL.
    ///
    /// # Arguments
    /// - `base_url` - Facilitator base URL (e.g., "https://web3.okx.com")
    /// - `api_key` - Seller's OKX API key
    /// - `secret_key` - Seller's OKX secret key
    /// - `passphrase` - Seller's OKX passphrase
    pub fn with_url(
        base_url: &str,
        api_key: &str,
        secret_key: &str,
        passphrase: &str,
    ) -> Self {
        Self {
            http: Client::builder()
                .use_native_tls()
                .build()
                .expect("failed to build HTTP client"),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            secret_key: secret_key.to_string(),
            passphrase: passphrase.to_string(),
        }
    }

    /// Build the full URL for a given API path.
    fn url(&self, path: &str) -> String {
        format!("{}{}{}", self.base_url, API_PREFIX, path)
    }

    /// Build the request path for HMAC signing.
    fn request_path(&self, path: &str) -> String {
        format!("{}{}", API_PREFIX, path)
    }

    /// Unwrap OKX API response wrapper, extracting the `data` field.
    /// OKX wraps all responses in `{"code":0, "data": {...}, "msg":""}`.
    fn unwrap_okx_response<T: DeserializeOwned>(body: &str) -> Result<T, X402Error> {
        // First try to parse as OKX wrapper
        if let Ok(wrapper) = serde_json::from_str::<OkxApiResponse<T>>(body) {
            if wrapper.code != 0 {
                let msg = wrapper
                    .error_message
                    .or(wrapper.msg)
                    .unwrap_or_else(|| format!("OKX API error code: {}", wrapper.code));
                return Err(X402Error::Other(msg));
            }
            if let Some(data) = wrapper.data {
                return Ok(data);
            }
            return Err(X402Error::Other("OKX API returned null data".into()));
        }

        // Fallback: try to parse as raw response (for non-OKX facilitators)
        serde_json::from_str::<T>(body).map_err(|e| X402Error::Serialization(e))
    }
}

#[async_trait]
impl FacilitatorClient for OkxHttpFacilitatorClient {
    async fn get_supported(&self) -> Result<SupportedResponse, X402Error> {
        let path = "/supported";
        let url = self.url(path);
        tracing::debug!("[x402] GET {}", url);

        let headers = build_auth_headers(
            &self.api_key,
            &self.secret_key,
            &self.passphrase,
            "GET",
            &self.request_path(path),
            "",
        )?;

        let response = self
            .http
            .get(&url)
            .headers(headers)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(X402Error::Other(format!(
                "facilitator /supported returned {}: {}",
                status.as_u16(),
                body
            )));
        }

        Self::unwrap_okx_response(&body)
    }

    async fn verify(&self, request: &VerifyRequest) -> Result<VerifyResponse, X402Error> {
        let path = "/verify";
        let url = self.url(path);
        let req_body = serde_json::to_string(request)?;
        tracing::debug!("[x402] POST {} request={}", url, req_body);

        let headers = build_auth_headers(
            &self.api_key,
            &self.secret_key,
            &self.passphrase,
            "POST",
            &self.request_path(path),
            &req_body,
        )?;

        let response = self
            .http
            .post(&url)
            .headers(headers)
            .header("Content-Type", "application/json")
            .body(req_body)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(X402Error::Other(format!(
                "facilitator /verify returned {}: {}",
                status.as_u16(),
                body
            )));
        }

        Self::unwrap_okx_response(&body)
    }

    async fn settle(&self, request: &SettleRequest) -> Result<SettleResponse, X402Error> {
        let path = "/settle";
        let url = self.url(path);
        let req_body = serde_json::to_string(request)?;
        tracing::debug!("[x402] POST {} request={}", url, req_body);

        let headers = build_auth_headers(
            &self.api_key,
            &self.secret_key,
            &self.passphrase,
            "POST",
            &self.request_path(path),
            &req_body,
        )?;

        let response = self
            .http
            .post(&url)
            .headers(headers)
            .header("Content-Type", "application/json")
            .body(req_body)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::debug!("[x402] POST {} → {} body={}", url, status.as_u16(), body);

        if !status.is_success() {
            return Err(X402Error::Other(format!(
                "facilitator /settle returned {}: {}",
                status.as_u16(),
                body
            )));
        }

        Self::unwrap_okx_response(&body)
    }

    async fn get_settle_status(&self, tx_hash: &str) -> Result<SettleStatusResponse, X402Error> {
        // URL-encode txHash for safety (0x + hex is already URL-safe, but be defensive)
        let encoded_hash: String = tx_hash
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{:02X}", b),
            })
            .collect();
        let path = format!("/settle/status?txHash={}", encoded_hash);
        let url = self.url(&path);
        tracing::debug!("[x402] GET {}", url);

        let headers = build_auth_headers(
            &self.api_key,
            &self.secret_key,
            &self.passphrase,
            "GET",
            &self.request_path(&path),
            "",
        )?;

        let response = self
            .http
            .get(&url)
            .headers(headers)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::debug!("[x402] GET {} → {} body={}", url, status.as_u16(), body);

        if !status.is_success() {
            return Err(X402Error::Other(format!(
                "facilitator /settle/status returned {}: {}",
                status.as_u16(),
                body
            )));
        }

        Self::unwrap_okx_response(&body)
    }
}
