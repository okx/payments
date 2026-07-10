//! OKX HTTP Facilitator Client implementation.
//! Extended with OKX HMAC-SHA256 authentication.
//!
//! OKX Facilitator responses are wrapped in `{"code":0, "data": {...}, "msg":""}`.
//! This client automatically unwraps the `data` field.

use std::time::Duration;

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
const DEFAULT_FACILITATOR_URL: &str = "https://web3.okx.com";

/// OKX API response wrapper.
/// All OKX Facilitator responses are wrapped in this structure.
///
/// Example: `{"code":0, "data": {...}, "msg":"", "error_code":"0", "error_message":""}`
#[derive(Debug, serde::Deserialize)]
struct OkxApiResponse {
    code: i32,
    /// Raw JSON so a business error (`code != 0`) surfaces its `msg` even when
    /// `data` is `{}` or wrong-shaped.
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
}

/// HTTP client for communicating with the OKX x402 Facilitator.
/// Extended with OKX HMAC-SHA256 signing on every request.
///
/// # Example
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use x402_core::http::OkxHttpFacilitatorClient;
///
/// // With default URL (https://web3.okx.com)
/// let client = OkxHttpFacilitatorClient::new(
///     "your-api-key",
///     "your-secret-key",
///     "your-passphrase",
/// )?;
///
/// // With custom URL
/// let client = OkxHttpFacilitatorClient::with_url(
///     "https://custom-facilitator.example.com",
///     "your-api-key",
///     "your-secret-key",
///     "your-passphrase",
/// )?;
/// # Ok(())
/// # }
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
    /// # Arguments
    /// - `api_key` - Seller's OKX API key
    /// - `secret_key` - Seller's OKX secret key
    /// - `passphrase` - Seller's OKX passphrase
    pub fn new(api_key: &str, secret_key: &str, passphrase: &str) -> Result<Self, X402Error> {
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
    ) -> Result<Self, X402Error> {
        let http = Client::builder()
            .use_native_tls()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| X402Error::Config(format!("failed to build HTTP client: {}", e)))?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            secret_key: secret_key.to_string(),
            passphrase: passphrase.to_string(),
        })
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
        // Check `code` before parsing `data` so a business error surfaces its `msg`.
        if let Ok(wrapper) = serde_json::from_str::<OkxApiResponse>(body) {
            if wrapper.code != 0 {
                let msg = wrapper
                    .error_message
                    .filter(|s| !s.is_empty())
                    .or_else(|| wrapper.msg.filter(|s| !s.is_empty()))
                    .unwrap_or_else(|| format!("OKX API error code: {}", wrapper.code));
                return Err(X402Error::Other(msg));
            }
            match wrapper.data {
                Some(serde_json::Value::Null) | None => {
                    return Err(X402Error::Other("OKX API returned null data".into()))
                }
                Some(data) => return serde_json::from_value::<T>(data).map_err(X402Error::Serialization),
            }
        }

        // Fallback: non-OKX facilitator returning the raw `T` directly.
        serde_json::from_str::<T>(body).map_err(X402Error::Serialization)
    }

    /// Like [`Self::unwrap_okx_response`] but tolerates an empty result: with
    /// `code == 0`, a `null` / absent `data` returns `Ok(None)`.
    fn unwrap_okx_response_opt<T: DeserializeOwned>(body: &str) -> Result<Option<T>, X402Error> {
        let wrapper: OkxApiResponse = serde_json::from_str(body)?;
        if wrapper.code != 0 {
            let msg = wrapper
                .error_message
                .filter(|s| !s.is_empty())
                .or_else(|| wrapper.msg.filter(|s| !s.is_empty()))
                .unwrap_or_else(|| format!("OKX API error code: {}", wrapper.code));
            return Err(X402Error::Other(msg));
        }
        match wrapper.data {
            Some(serde_json::Value::Null) | None => Ok(None),
            Some(data) => serde_json::from_value::<T>(data)
                .map(Some)
                .map_err(X402Error::Serialization),
        }
    }
}

#[async_trait]
impl FacilitatorClient for OkxHttpFacilitatorClient {
    async fn get_supported(&self) -> Result<SupportedResponse, X402Error> {
        let path = "/supported";
        let url = self.url(path);
        tracing::info!(target: "x402-facilitator", "→ GET {}", url);

        let headers = build_auth_headers(
            &self.api_key,
            &self.secret_key,
            &self.passphrase,
            "GET",
            &self.request_path(path),
            "",
        )?;

        let response = self.http.get(&url).headers(headers).send().await?;

        let status = response.status();
        let body = response.text().await?;

        if !status.is_success() {
            tracing::warn!(
                target: "x402-facilitator",
                "← GET {} status={} body={}",
                path, status.as_u16(), body
            );
            return Err(X402Error::Other(format!(
                "facilitator /supported returned {}: {}",
                status.as_u16(),
                body
            )));
        }

        tracing::info!(
            target: "x402-facilitator",
            "← GET {} status={} body={}",
            path, status.as_u16(), body
        );
        Self::unwrap_okx_response(&body)
    }

    async fn verify(&self, request: &VerifyRequest) -> Result<VerifyResponse, X402Error> {
        let path = "/verify";
        let url = self.url(path);
        let req_body = serde_json::to_string(request)?;

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
        let body = response.text().await?;

        if !status.is_success() {
            return Err(X402Error::Other(format!(
                "facilitator /verify returned {}: {}",
                status.as_u16(),
                body
            )));
        }

        Self::unwrap_okx_response(&body)
    }

    async fn verify_signature_only(
        &self,
        request: &VerifyRequest,
    ) -> Result<VerifyResponse, X402Error> {
        // Signature-only verify: validates just the signature (no balance/
        // nonce/blacklist/etc). Same contract as `/verify`.
        let path = "/verify-signature";
        let url = self.url(path);
        let req_body = serde_json::to_string(request)?;

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
        let body = response.text().await?;

        if !status.is_success() {
            return Err(X402Error::Other(format!(
                "facilitator {} returned {}: {}",
                path,
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
        let body = response.text().await?;

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

        let headers = build_auth_headers(
            &self.api_key,
            &self.secret_key,
            &self.passphrase,
            "GET",
            &self.request_path(&path),
            "",
        )?;

        let response = self.http.get(&url).headers(headers).send().await?;

        let status = response.status();
        let body = response.text().await?;

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

// ─────────────────────────── subscription endpoints ────────────────────────

use serde::Serialize;
use crate::subscription::{
    CancelPendingChangeRequest, CancelSubscriptionRequest, ChangeResponse,
    ChangeSubscriptionRequest, ChargeResponse, ChargesResponse, CreateSubscriptionRequest,
    CreateSubscriptionResponse, PendingPlanChange, SubscriptionChargeReq,
    SubscriptionFacilitatorClient, SubscriptionFinalizeExpiredReq, SubscriptionStatus,
    TxResultResponse,
};

/// Shared request helpers for the subscription endpoints (OK-ACCESS signed,
/// OKX-envelope unwrapped) — same pattern as verify/settle above.
impl OkxHttpFacilitatorClient {
    async fn sub_post<B: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<R, X402Error> {
        let req_body = serde_json::to_string(body)?;
        let url = self.url(path);
        // Logs the protocol body only — never the OK-ACCESS auth headers / API key.
        tracing::info!(target: "x402-facilitator", "→ POST {} body={}", url, req_body);
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
        Self::handle_sub("POST", path, response).await
    }

    async fn sub_get<R: DeserializeOwned>(&self, path: &str) -> Result<R, X402Error> {
        let url = self.url(path);
        tracing::info!(target: "x402-facilitator", "→ GET {}", url);
        let headers = build_auth_headers(
            &self.api_key,
            &self.secret_key,
            &self.passphrase,
            "GET",
            &self.request_path(path),
            "",
        )?;
        let response = self.http.get(&url).headers(headers).send().await?;
        Self::handle_sub("GET", path, response).await
    }

    async fn handle_sub<R: DeserializeOwned>(
        method: &str,
        path: &str,
        response: reqwest::Response,
    ) -> Result<R, X402Error> {
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            tracing::warn!(
                target: "x402-facilitator",
                "← {} {} status={} body={}",
                method, path, status.as_u16(), body
            );
            return Err(X402Error::Other(format!(
                "facilitator {} returned {}: {}",
                path,
                status.as_u16(),
                body
            )));
        }
        tracing::info!(
            target: "x402-facilitator",
            "← {} {} status={} body={}",
            method, path, status.as_u16(), body
        );
        Self::unwrap_okx_response(&body)
    }

    /// GET whose `data` may be empty (`null`) — yields `Ok(None)`.
    async fn sub_get_opt<R: DeserializeOwned>(&self, path: &str) -> Result<Option<R>, X402Error> {
        let url = self.url(path);
        tracing::info!(target: "x402-facilitator", "→ GET {}", url);
        let headers = build_auth_headers(
            &self.api_key,
            &self.secret_key,
            &self.passphrase,
            "GET",
            &self.request_path(path),
            "",
        )?;
        let response = self.http.get(&url).headers(headers).send().await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            tracing::warn!(
                target: "x402-facilitator",
                "← GET {} status={} body={}",
                path, status.as_u16(), body
            );
            return Err(X402Error::Other(format!(
                "facilitator {} returned {}: {}",
                path,
                status.as_u16(),
                body
            )));
        }
        tracing::info!(
            target: "x402-facilitator",
            "← GET {} status={} body={}",
            path, status.as_u16(), body
        );
        Self::unwrap_okx_response_opt(&body)
    }
}

#[async_trait]
impl SubscriptionFacilitatorClient for OkxHttpFacilitatorClient {
    async fn create_subscription(
        &self,
        req: &CreateSubscriptionRequest,
    ) -> Result<CreateSubscriptionResponse, X402Error> {
        tracing::info!(
            "[x402-sub] create_subscription: chainIndex={} payer={} planTier={} syncSettle={}",
            req.chain_index, req.terms.payer, req.terms.plan_tier, req.sync_settle
        );
        self.sub_post("/subscriptions", req).await
    }

    async fn charge(
        &self,
        sub_id: &str,
        sync_settle: bool,
    ) -> Result<ChargeResponse, X402Error> {
        tracing::info!("[x402-sub] charge: subId={} syncSettle={}", sub_id, sync_settle);
        let body = SubscriptionChargeReq { sub_id: sub_id.to_string(), sync_settle };
        self.sub_post("/subscriptions/charge", &body).await
    }

    async fn change_subscription(
        &self,
        req: &ChangeSubscriptionRequest,
    ) -> Result<ChangeResponse, X402Error> {
        tracing::info!(
            "[x402-sub] change_subscription: chainIndex={} oldSubId={} payer={} planTier={} syncSettle={}",
            req.chain_index, req.old_sub_id, req.new_terms.payer, req.new_terms.plan_tier, req.sync_settle
        );
        self.sub_post("/subscriptions/change", req).await
    }

    async fn cancel_subscription(
        &self,
        req: &CancelSubscriptionRequest,
    ) -> Result<TxResultResponse, X402Error> {
        tracing::info!("[x402-sub] cancel_subscription: subId={}", req.sub_id);
        self.sub_post("/subscriptions/cancel", req).await
    }

    async fn cancel_pending_change(
        &self,
        req: &CancelPendingChangeRequest,
    ) -> Result<TxResultResponse, X402Error> {
        tracing::info!("[x402-sub] cancel_pending_change: subId={}", req.sub_id);
        self.sub_post("/subscriptions/cancel-pending-change", req).await
    }

    async fn finalize_expired(&self, sub_id: &str) -> Result<TxResultResponse, X402Error> {
        tracing::info!("[x402-sub] finalize_expired: subId={}", sub_id);
        let body = SubscriptionFinalizeExpiredReq { sub_id: sub_id.to_string() };
        self.sub_post("/subscriptions/finalize-expired", &body).await
    }

    async fn get_subscription(&self, sub_id: &str) -> Result<SubscriptionStatus, X402Error> {
        tracing::info!("[x402-sub] get_subscription: subId={}", sub_id);
        self.sub_get(&format!("/subscriptions/detail?subId={}", sub_id)).await
    }

    async fn get_charges(
        &self,
        sub_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<ChargesResponse, X402Error> {
        self.sub_get(&format!(
            "/subscriptions/charges?subId={}&limit={}&offset={}",
            sub_id, limit, offset
        ))
        .await
    }

    async fn get_pending_change(
        &self,
        sub_id: &str,
    ) -> Result<Option<PendingPlanChange>, X402Error> {
        self.sub_get_opt(&format!("/subscriptions/pending?subId={}", sub_id)).await
    }
}

