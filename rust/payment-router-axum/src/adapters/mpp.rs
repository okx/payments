//! `MppAdapter` — MPP protocol adapter (spec §3 / §6.2).
//!
//! Thin shell. Delegates challenge construction / verify (HMAC + expiry
//! + SA-API roundtrip) to [`mpp_evm::EvmMpp`]; formats headers via
//! upstream `format_www_authenticate` / `format_receipt` pub helpers.
//! No custom crypto, no custom serialization.
//!
//! On request:
//! 1. Parse `Authorization: Payment <b64>` header.
//! 2. Peek `credential.challenge.intent` to dispatch:
//!    - `"charge"`  → [`EvmMpp::verify_charge`]
//!    - `"session"` → [`EvmMpp::verify_session`]
//!    Both verifiers run HMAC + expiry + SA verify internally.
//! 3. On success: call `inner` (real axum handler), then append
//!    `Payment-Receipt` header formatted via upstream `format_receipt`.
//! 4. On failure / missing: return 402 with `WWW-Authenticate: Payment ...`
//!    built from upstream `format_www_authenticate`.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use http::{header, request::Parts, HeaderMap, HeaderValue, Request, Response, StatusCode};
use mpp::protocol::core::headers::{
    format_receipt, format_www_authenticate, PAYMENT_RECEIPT_HEADER, WWW_AUTHENTICATE_HEADER,
};
use mpp_evm::{ChargeRouteParams, EvmMpp, SessionRouteParams};
use tower::util::BoxCloneSyncService;
use tower::{Service, ServiceExt};

use crate::adapter::{ChallengeFuture, ChallengeResponse, InnerService, ProtocolAdapter};
use crate::config::AdapterConfig;

/// Typed per-route config consumed by [`MppAdapter`].
///
/// `intent` selects which EVM method handles this route — `"charge"`
/// (one-shot) or `"session"` (channel-based pay-as-you-go).
///
/// Service-level state (chain id, recipient, secret key, realm) lives
/// on the [`EvmMpp`] facade; method-level state ([`mpp_evm::EvmCharge`]
/// configuration, [`mpp_evm::EvmSessionMethod`] state) is also held
/// there. This struct carries only what varies per route.
#[derive(Debug, Clone, Default)]
pub struct MppRouteConfig {
    /// Either `"charge"` or `"session"`. Defaults to `"charge"` when
    /// omitted (`""`).
    pub intent: String,
    /// Payment amount as a base-units integer string.
    pub amount: String,
    /// ERC-20 token contract address.
    pub currency: String,
    /// Optional human-readable description (echoed into the
    /// `WWW-Authenticate` challenge).
    pub description: Option<String>,
    /// Optional merchant reference id (charge only; session has no
    /// slot for it in the upstream `SessionRequest`).
    pub external_id: Option<String>,
    /// Session-only: billing unit (e.g. `"request"`, `"byte"`).
    pub unit_type: Option<String>,
    /// Session-only: suggested initial deposit (base units, stringified).
    pub suggested_deposit: Option<String>,
}

/// MPP adapter. Spec §9: built-in priority = 10 (tried before x402).
///
/// Wraps an [`EvmMpp`] facade which carries the service-level config
/// (`chain_id` / `recipient` / `secret_key` / `realm`) and both
/// charge + session method handlers. The adapter reads
/// [`MppRouteConfig::intent`] on every request to pick which method
/// handles the route.
#[derive(Clone)]
pub struct MppAdapter {
    mpp: Arc<EvmMpp>,
    priority: u32,
}

impl MppAdapter {
    /// Construct with the default priority (10).
    pub fn new(mpp: Arc<EvmMpp>) -> Self {
        Self { mpp, priority: 10 }
    }

    /// Override the default priority. User extensions should prefer 100+.
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }
}

impl ProtocolAdapter for MppAdapter {
    fn name(&self) -> &str {
        "mpp"
    }

    fn priority(&self) -> u32 {
        self.priority
    }

    fn detect(&self, parts: &Parts) -> bool {
        parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                // Case-insensitive per RFC 7235. "Payment " prefix is the
                // scheme; trailing content is the credential.
                v.split(',')
                    .map(str::trim)
                    .any(|s| s.len() >= 8 && s[..8].eq_ignore_ascii_case("payment "))
            })
            .unwrap_or(false)
    }

    fn get_challenge<'a>(
        &'a self,
        _parts: &'a Parts,
        route_cfg: &'a AdapterConfig,
    ) -> ChallengeFuture<'a> {
        let mpp = self.mpp.clone();
        let cfg = route_cfg.downcast_ref::<MppRouteConfig>().cloned();
        Box::pin(async move {
            let cfg = cfg.ok_or_else(|| {
                "mpp adapter: route config is not MppRouteConfig".to_string()
            })?;
            if cfg.amount.is_empty() {
                return Err("mpp route config missing `amount`".to_string());
            }
            if cfg.currency.is_empty() {
                return Err("mpp route config missing `currency`".to_string());
            }

            let challenge = match resolve_intent(&cfg.intent)? {
                Intent::Charge => mpp
                    .charge_challenge(ChargeRouteParams {
                        amount: &cfg.amount,
                        currency: &cfg.currency,
                        description: cfg.description.as_deref(),
                        external_id: cfg.external_id.as_deref(),
                    })
                    .map_err(|e| format!("mpp charge challenge failed: {e}"))?,
                Intent::Session => mpp
                    .session_challenge(SessionRouteParams {
                        amount: &cfg.amount,
                        currency: &cfg.currency,
                        description: cfg.description.as_deref(),
                        external_id: cfg.external_id.as_deref(),
                        unit_type: cfg.unit_type.as_deref(),
                        suggested_deposit: cfg.suggested_deposit.as_deref(),
                        splits: None,
                    })
                    .map_err(|e| format!("mpp session challenge failed: {e}"))?,
            };
            let www = format_www_authenticate(&challenge)
                .map_err(|e| format!("mpp format_www_authenticate failed: {e}"))?;
            let mut map = HeaderMap::new();
            map.append(
                WWW_AUTHENTICATE_HEADER,
                HeaderValue::from_str(&www).map_err(|e| e.to_string())?,
            );
            Ok(Some(ChallengeResponse::headers_only(map)))
        })
    }

    fn make_service(&self, inner: InnerService) -> InnerService {
        BoxCloneSyncService::new(MppVerifyService {
            inner,
            mpp: self.mpp.clone(),
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum Intent {
    Charge,
    Session,
}

fn resolve_intent(raw: &str) -> Result<Intent, String> {
    match raw {
        "" | "charge" => Ok(Intent::Charge),
        "session" => Ok(Intent::Session),
        other => Err(format!(
            "mpp adapter: unsupported intent {other:?} (expected \"charge\" or \"session\")"
        )),
    }
}

/// Tower Service that decodes the `Authorization: Payment …` header,
/// dispatches to charge or session verify on the `EvmMpp` facade by
/// reading the credential's challenge `intent`, calls the inner handler
/// on success, and appends `Payment-Receipt` to the response.
#[derive(Clone)]
struct MppVerifyService {
    inner: InnerService,
    mpp: Arc<EvmMpp>,
}

impl Service<Request<Body>> for MppVerifyService {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<
        Box<dyn std::future::Future<Output = Result<Response<Body>, Infallible>> + Send + 'static>,
    >;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let mpp = self.mpp.clone();
        let inner = self.inner.clone();

        Box::pin(async move {
            let auth = match req
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
            {
                Some(a) => a,
                None => {
                    return Ok(error_response(
                        StatusCode::UNAUTHORIZED,
                        "missing Authorization header",
                    ));
                }
            };

            // Peek the credential's challenge.intent to choose the verify path.
            // Charge and session both carry their intent inside the b64
            // ChallengeEcho; we read it once before re-running the same string
            // through the dedicated verifier.
            let intent_from_credential = intent_from_authorization(&auth);

            let receipt = match intent_from_credential.as_deref() {
                Some("session") => match mpp.verify_session(&auth).await {
                    Ok(result) => result.receipt,
                    Err(e) => {
                        return Ok(error_response(StatusCode::PAYMENT_REQUIRED, &e));
                    }
                },
                // Default to charge for "charge" or missing/unknown intent.
                _ => match mpp.verify_charge(&auth).await {
                    Ok(r) => r,
                    Err(e) => {
                        return Ok(error_response(StatusCode::PAYMENT_REQUIRED, &e));
                    }
                },
            };

            let mut resp = inner.oneshot(req).await?;
            match format_receipt(&receipt) {
                Ok(header_str) => match HeaderValue::from_str(&header_str) {
                    Ok(hv) => {
                        if !resp.headers().contains_key(PAYMENT_RECEIPT_HEADER) {
                            resp.headers_mut().insert(PAYMENT_RECEIPT_HEADER, hv);
                        }
                    }
                    Err(e) => tracing::error!(err=%e, "invalid Payment-Receipt header"),
                },
                Err(e) => tracing::error!(err=%e, "format_receipt failed"),
            }
            Ok(resp)
        })
    }
}

/// Pull the `challenge.intent` out of an `Authorization: Payment <b64>`
/// header without doing full HMAC verification. The chosen verify path
/// (`mpp.verify_charge` / `mpp.verify_session`) re-parses and HMAC-checks;
/// this is a routing-only peek.
fn intent_from_authorization(auth: &str) -> Option<String> {
    let credential = mpp::protocol::core::parse_authorization(auth).ok()?;
    Some(credential.challenge.intent.to_string())
}

fn error_response(status: StatusCode, msg: &str) -> Response<Body> {
    let body = serde_json::json!({
        "type": "about:blank",
        "title": status.canonical_reason().unwrap_or(""),
        "status": status.as_u16(),
        "detail": msg,
    });
    let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/problem+json")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(status)
                .body(Body::empty())
                .expect("static error response")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Request;
    use mpp_evm::sa_client::SaApiClient;
    use mpp_evm::types::{
        ChannelStatus, ChargeReceipt, CloseRequestPayload, SessionReceipt, SettleRequestPayload,
    };
    use mpp_evm::{EvmCharge, EvmConfig, EvmMpp, EvmSessionMethod, SaApiError};

    fn parts_with_header(name: &str, val: &str) -> Parts {
        let req = Request::builder()
            .header(name, val)
            .body(Body::empty())
            .unwrap();
        let (parts, _) = req.into_parts();
        parts
    }

    fn parts_no_auth() -> Parts {
        let (parts, _) = Request::new(Body::empty()).into_parts();
        parts
    }

    /// Minimal `SaApiClient` stub. Detect/get_challenge tests never reach
    /// the SA backend; methods are wired only to fail loudly so a future
    /// mistake doesn't silently exercise them.
    #[derive(Debug, Default)]
    struct StubSa;
    #[async_trait::async_trait]
    impl SaApiClient for StubSa {
        async fn charge_settle(
            &self,
            _: &serde_json::Value,
        ) -> Result<ChargeReceipt, SaApiError> {
            unreachable!("StubSa: charge_settle")
        }
        async fn charge_verify_hash(
            &self,
            _: &serde_json::Value,
        ) -> Result<ChargeReceipt, SaApiError> {
            unreachable!("StubSa: charge_verify_hash")
        }
        async fn session_open(
            &self,
            _: &serde_json::Value,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!("StubSa: session_open")
        }
        async fn session_top_up(
            &self,
            _: &serde_json::Value,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!("StubSa: session_top_up")
        }
        async fn session_settle(
            &self,
            _: &SettleRequestPayload,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!("StubSa: session_settle")
        }
        async fn session_close(
            &self,
            _: &CloseRequestPayload,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!("StubSa: session_close")
        }
        async fn session_status(&self, _: &str) -> Result<ChannelStatus, SaApiError> {
            unreachable!("StubSa: session_status")
        }
    }

    fn fixture_mpp(with_session: bool) -> Arc<EvmMpp> {
        let sa: Arc<dyn SaApiClient> = Arc::new(StubSa);
        let mut builder = EvmMpp::builder(EvmConfig {
            chain_id: 196,
            recipient: "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1".into(),
            secret_key: "test-secret".into(),
            realm: "test.local".into(),
        })
        .with_charge(EvmCharge::new(sa.clone()).with_fee_payer(true));
        if with_session {
            builder = builder.with_session(EvmSessionMethod::new(sa));
        }
        Arc::new(builder.build().expect("build EvmMpp"))
    }

    fn adapter() -> MppAdapter {
        MppAdapter::new(fixture_mpp(false))
    }

    #[test]
    fn detect_true_on_payment_scheme() {
        assert!(adapter().detect(&parts_with_header("authorization", "Payment abc123")));
    }

    #[test]
    fn detect_case_insensitive() {
        assert!(adapter().detect(&parts_with_header("authorization", "payment ABC")));
        assert!(adapter().detect(&parts_with_header("authorization", "PAYMENT xyz")));
    }

    #[test]
    fn detect_mixed_schemes() {
        // RFC 9110: comma-separated schemes are allowed.
        assert!(adapter().detect(&parts_with_header(
            "authorization",
            "Bearer tok, Payment abc"
        )));
    }

    #[test]
    fn detect_false_on_bearer_only() {
        assert!(!adapter().detect(&parts_with_header("authorization", "Bearer token")));
    }

    #[test]
    fn detect_false_on_missing() {
        assert!(!adapter().detect(&parts_no_auth()));
    }

    #[test]
    fn name_and_priority() {
        let a = adapter();
        assert_eq!(a.name(), "mpp");
        assert_eq!(a.priority(), 10);
    }

    #[tokio::test]
    async fn get_challenge_charge_intent_produces_www_authenticate() {
        let adapter = MppAdapter::new(fixture_mpp(false));
        let cfg = AdapterConfig::new(MppRouteConfig {
            intent: "charge".into(),
            amount: "100".into(),
            currency: "0x74b7F16337b8972027F6196A17a631aC6dE26d22".into(),
            description: Some("One photo".into()),
            external_id: None,
            unit_type: None,
            suggested_deposit: None,
        });
        let resp = adapter
            .get_challenge(&parts_no_auth(), &cfg)
            .await
            .expect("challenge ok")
            .expect("non-empty challenge");
        assert!(
            resp.headers.contains_key(WWW_AUTHENTICATE_HEADER),
            "challenge must contain WWW-Authenticate"
        );
    }

    #[tokio::test]
    async fn get_challenge_session_intent_succeeds_when_session_registered() {
        let adapter = MppAdapter::new(fixture_mpp(true));
        let cfg = AdapterConfig::new(MppRouteConfig {
            intent: "session".into(),
            amount: "100".into(),
            currency: "0x74b7F16337b8972027F6196A17a631aC6dE26d22".into(),
            description: Some("API access".into()),
            external_id: None,
            unit_type: Some("request".into()),
            suggested_deposit: Some("10000".into()),
        });
        let resp = adapter
            .get_challenge(&parts_no_auth(), &cfg)
            .await
            .expect("session challenge ok")
            .expect("non-empty challenge");
        assert!(resp.headers.contains_key(WWW_AUTHENTICATE_HEADER));
    }

    #[tokio::test]
    async fn get_challenge_session_intent_errors_when_session_missing() {
        let adapter = MppAdapter::new(fixture_mpp(false));
        let cfg = AdapterConfig::new(MppRouteConfig {
            intent: "session".into(),
            amount: "100".into(),
            currency: "0x74b7F16337b8972027F6196A17a631aC6dE26d22".into(),
            description: None,
            external_id: None,
            unit_type: None,
            suggested_deposit: None,
        });
        let err = adapter
            .get_challenge(&parts_no_auth(), &cfg)
            .await
            .expect_err("session intent without session method must error");
        assert!(
            err.contains("session method not registered"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn get_challenge_unsupported_intent_errors() {
        let adapter = MppAdapter::new(fixture_mpp(false));
        let cfg = AdapterConfig::new(MppRouteConfig {
            intent: "topup".into(),
            amount: "100".into(),
            currency: "0xtoken".into(),
            description: None,
            external_id: None,
            unit_type: None,
            suggested_deposit: None,
        });
        let err = adapter
            .get_challenge(&parts_no_auth(), &cfg)
            .await
            .expect_err("unsupported intent must error");
        assert!(err.contains("unsupported intent"), "got: {err}");
    }
}
