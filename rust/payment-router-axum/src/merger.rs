//! Spec §10 / §9 challenge merger — parallel `get_challenge` + multi-row 402.
//!
//! When no adapter detected, we concurrently ask each adapter to produce its
//! 402 challenge headers, then merge them into a single response with multiple
//! same-name `WWW-Authenticate` rows (see spec §3 #3 / §9 "Challenge 合并" —
//! no comma-concatenation).
//!
//! A single adapter failing (`Err(msg)`) or returning `None` does not block
//! the others — remaining challenges still contribute. Per-adapter timeout is
//! spec P0-3 DEFER and is not implemented this iteration.

use std::sync::Arc;

use axum::body::Body;
use std::time::Duration;

use axum::body::Bytes;
use futures::future::join_all;
use http::{header::CONTENT_TYPE, request::Parts, HeaderMap, Response, StatusCode};

use crate::adapter::ProtocolAdapter;
use crate::types::{ErrorContext, ErrorHandler, ErrorPhase, UnifiedRouteConfig};

/// Merged 402 challenge: headers from every contributing adapter plus an
/// optional response body. The body is the first non-empty body produced
/// by any adapter (in `adapters[]` order); typically only x402 supplies a
/// body (containing `accepts[]`), while MPP keeps everything in headers.
/// (H5)
pub(crate) struct MergedChallenge {
    pub headers: HeaderMap,
    pub body: Option<Bytes>,
}

/// Maximum time we'll wait for any single adapter's `get_challenge` to
/// complete. Beyond this we treat the adapter as failing (skip merge,
/// report via `on_error`) so a slow / hung adapter (SA backend, third-party
/// facilitator) cannot DoS the entire 402 response. (H6)
const PER_ADAPTER_CHALLENGE_TIMEOUT: Duration = Duration::from_secs(2);

/// Run `get_challenge` on all adapters in parallel, merge resulting headers.
///
/// Returns a merged `HeaderMap` with (possibly multiple) `WWW-Authenticate`
/// entries, one per successful adapter. Failures (including timeouts) are
/// reported via `on_error`.
pub(crate) async fn merge_challenges(
    adapters: &[Arc<dyn ProtocolAdapter>],
    parts: &Parts,
    route_cfg: &UnifiedRouteConfig,
    route_key: &str,
    on_error: Option<&Arc<ErrorHandler>>,
) -> MergedChallenge {
    // Build futures that each carry the adapter name for error reporting,
    // and wrap each in a per-adapter timeout (H6) so one slow adapter
    // can't hang the whole 402 response.
    let futures = adapters.iter().map(|adapter| async move {
        let name = adapter.name().to_string();
        // If this adapter has no config for this route, skip it entirely (spec §3 #4).
        let cfg = match route_cfg.adapter_configs.get(&name) {
            Some(v) => v,
            None => return (name, Ok(None)),
        };
        let result = match tokio::time::timeout(
            PER_ADAPTER_CHALLENGE_TIMEOUT,
            adapter.get_challenge(parts, cfg),
        )
        .await
        {
            Ok(inner) => inner,
            Err(_elapsed) => Err(format!(
                "adapter {name} get_challenge timed out after {:?}",
                PER_ADAPTER_CHALLENGE_TIMEOUT
            )),
        };
        (name, result)
    });

    let results = join_all(futures).await;

    let mut merged_headers = HeaderMap::new();
    let mut merged_body: Option<Bytes> = None;
    for (protocol, result) in results {
        match result {
            Ok(Some(resp)) => {
                for (name, value) in resp.headers.iter() {
                    // append = multi-row semantics (spec §9: no comma concat).
                    merged_headers.append(name.clone(), value.clone());
                }
                // First adapter to provide a body wins (typically x402,
                // which needs `accepts[]` in the body). Subsequent
                // adapter bodies are dropped because a 402 response has
                // exactly one body.
                if merged_body.is_none() {
                    if let Some(b) = resp.body {
                        if !b.is_empty() {
                            merged_body = Some(b);
                        }
                    }
                }
            }
            Ok(None) => {
                // Adapter has no challenge for this route; silently skip.
            }
            Err(msg) => {
                if let Some(handler) = on_error {
                    let ctx = ErrorContext {
                        phase: ErrorPhase::Challenge,
                        protocol: protocol.clone(),
                        route: Some(route_key.to_string()),
                    };
                    let err: Box<dyn std::error::Error + Send + Sync> = msg.clone().into();
                    (handler)(err.as_ref(), ctx);
                }
                tracing::debug!(
                    protocol = %protocol,
                    route = %route_key,
                    err = %msg,
                    "adapter get_challenge failed; skipping",
                );
            }
        }
    }
    MergedChallenge {
        headers: merged_headers,
        body: merged_body,
    }
}

/// Assemble the 402 response from a merged challenge.
///
/// If `merged.body` is provided (e.g. from x402 with its `accepts[]`
/// array), use that as the response body and let the adapter's own
/// `Content-Type` header (already in `merged.headers`) flow through. We
/// do **not** force `application/problem+json` in that case, because
/// x402 sends its own JSON shape.
///
/// If no adapter supplies a body, fall back to RFC 9457 problem+json
/// minimal form (`{type, title, status}`) — the original MPP-only
/// behavior. (H5)
pub(crate) fn build_402_response(merged: MergedChallenge) -> Response<Body> {
    let MergedChallenge { headers, body } = merged;

    let mut builder = Response::builder().status(StatusCode::PAYMENT_REQUIRED);
    let body_bytes = match body {
        Some(b) => b.to_vec(),
        None => {
            // No adapter contributed a body; use the spec-default RFC 9457
            // minimal form. Set Content-Type explicitly so the response is
            // self-describing even when no header carrying it was merged.
            builder = builder.header(CONTENT_TYPE, "application/problem+json");
            let body = serde_json::json!({
                "type": "about:blank",
                "title": "Payment Required",
                "status": 402,
            });
            serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec())
        }
    };
    let mut resp = builder.body(Body::from(body_bytes)).unwrap_or_else(|_| {
        Response::builder()
            .status(StatusCode::PAYMENT_REQUIRED)
            .body(Body::empty())
            .expect("static 402 response")
    });
    // Use `iter()` + clone, not `into_iter()`. `into_iter` returns
    // `(Option<HeaderName>, HeaderValue)` where subsequent entries with the
    // same name yield `None` — easy to miss and silently drop multi-row values.
    for (name, value) in headers.iter() {
        resp.headers_mut().append(name.clone(), value.clone());
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ChallengeFuture, ChallengeResponse, InnerService, ProtocolAdapter};
    use http::{header::WWW_AUTHENTICATE, HeaderValue, Request};
    use serde_json::Value;

    struct StubAdapter {
        name: String,
        result: Result<Option<ChallengeResponse>, String>,
    }

    impl ProtocolAdapter for StubAdapter {
        fn name(&self) -> &str {
            &self.name
        }
        fn priority(&self) -> u32 {
            10
        }
        fn detect(&self, _parts: &Parts) -> bool {
            false
        }
        fn get_challenge<'a>(
            &'a self,
            _parts: &'a Parts,
            _route_cfg: &'a Value,
        ) -> ChallengeFuture<'a> {
            let result = self.result.clone();
            Box::pin(async move { result })
        }
        fn make_service(&self, inner: InnerService) -> InnerService {
            inner
        }
    }

    fn header_only(header_name: &str, value: &str) -> ChallengeResponse {
        ChallengeResponse::headers_only(header_with(header_name, value))
    }

    fn cfg() -> UnifiedRouteConfig {
        let mut map = std::collections::HashMap::new();
        map.insert("mpp".into(), serde_json::json!({}));
        map.insert("x402".into(), serde_json::json!({}));
        UnifiedRouteConfig {
            description: None,
            adapter_configs: map,
        }
    }

    fn parts() -> Parts {
        let (parts, _) = Request::new(axum::body::Body::empty()).into_parts();
        parts
    }

    fn header_with(name: &str, value: &str) -> HeaderMap {
        let mut m = HeaderMap::new();
        m.append(
            http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
        m
    }

    #[tokio::test]
    async fn both_adapters_contribute() {
        let mpp: Arc<dyn ProtocolAdapter> = Arc::new(StubAdapter {
            name: "mpp".into(),
            result: Ok(Some(header_only("WWW-Authenticate", "Payment realm=\"m\""))),
        });
        let x402: Arc<dyn ProtocolAdapter> = Arc::new(StubAdapter {
            name: "x402".into(),
            result: Ok(Some(header_only("WWW-Authenticate", "x402 realm=\"x\""))),
        });
        let merged = merge_challenges(&[mpp, x402], &parts(), &cfg(), "GET /photos", None).await;
        let values: Vec<_> = merged
            .headers
            .get_all(WWW_AUTHENTICATE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert_eq!(values.len(), 2, "multi-row WWW-Authenticate expected");
        assert!(values.iter().any(|v| v.contains("Payment")));
        assert!(values.iter().any(|v| v.contains("x402")));
    }

    #[tokio::test]
    async fn single_adapter_error_does_not_block_others() {
        let mpp: Arc<dyn ProtocolAdapter> = Arc::new(StubAdapter {
            name: "mpp".into(),
            result: Err("boom".into()),
        });
        let x402: Arc<dyn ProtocolAdapter> = Arc::new(StubAdapter {
            name: "x402".into(),
            result: Ok(Some(header_only("WWW-Authenticate", "x402 realm=\"x\""))),
        });
        let merged = merge_challenges(&[mpp, x402], &parts(), &cfg(), "GET /photos", None).await;
        let values: Vec<_> = merged
            .headers
            .get_all(WWW_AUTHENTICATE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert_eq!(values.len(), 1, "only x402 contributes; mpp errored");
        assert!(values[0].contains("x402"));
    }

    #[tokio::test]
    async fn missing_adapter_config_skips_that_adapter() {
        let mut cfg = cfg();
        cfg.adapter_configs.remove("mpp");
        let mpp: Arc<dyn ProtocolAdapter> = Arc::new(StubAdapter {
            name: "mpp".into(),
            // Even if this adapter would contribute, spec §3 #4 says skip when no config
            result: Ok(Some(header_only(
                "WWW-Authenticate",
                "Payment should-not-appear",
            ))),
        });
        let x402: Arc<dyn ProtocolAdapter> = Arc::new(StubAdapter {
            name: "x402".into(),
            result: Ok(Some(header_only("WWW-Authenticate", "x402 realm=\"x\""))),
        });
        let merged = merge_challenges(&[mpp, x402], &parts(), &cfg, "GET /photos", None).await;
        let values: Vec<_> = merged
            .headers
            .get_all(WWW_AUTHENTICATE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert_eq!(values.len(), 1);
        assert!(values[0].contains("x402"));
        assert!(!values[0].contains("should-not-appear"));
    }

    /// H6 regression: a hung adapter must be cut off at
    /// `PER_ADAPTER_CHALLENGE_TIMEOUT`, and the other adapter still
    /// contributes its 402 header.
    #[tokio::test]
    async fn slow_adapter_times_out_and_others_still_merge() {
        struct SlowAdapter;
        impl ProtocolAdapter for SlowAdapter {
            fn name(&self) -> &str {
                "mpp"
            }
            fn priority(&self) -> u32 {
                10
            }
            fn detect(&self, _: &Parts) -> bool {
                false
            }
            fn get_challenge<'a>(&'a self, _: &'a Parts, _: &'a Value) -> ChallengeFuture<'a> {
                // Never returns within the per-adapter timeout window.
                Box::pin(async {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    Ok(None)
                })
            }
            fn make_service(&self, inner: InnerService) -> InnerService {
                inner
            }
        }

        let slow: Arc<dyn ProtocolAdapter> = Arc::new(SlowAdapter);
        let x402: Arc<dyn ProtocolAdapter> = Arc::new(StubAdapter {
            name: "x402".into(),
            result: Ok(Some(header_only("WWW-Authenticate", "x402 realm=\"x\""))),
        });

        // Wrap the merge in a 5s outer guard so a regression (no timeout)
        // would fail the test rather than hang it.
        let merged = tokio::time::timeout(
            Duration::from_secs(5),
            merge_challenges(&[slow, x402], &parts(), &cfg(), "GET /photos", None),
        )
        .await
        .expect("per-adapter timeout must release the merge inside 5s");

        let values: Vec<_> = merged
            .headers
            .get_all(WWW_AUTHENTICATE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert_eq!(values.len(), 1, "only x402 contributes; mpp timed out");
        assert!(values[0].contains("x402"));
    }

    /// H5 regression: x402 adapter returns a body (containing accepts[]).
    /// MPP returns headers-only. Merger must surface the x402 body
    /// alongside MPP headers, and `build_402_response` must use that body
    /// rather than the RFC 9457 default.
    #[tokio::test]
    async fn x402_body_is_preserved_through_merge() {
        let mpp: Arc<dyn ProtocolAdapter> = Arc::new(StubAdapter {
            name: "mpp".into(),
            result: Ok(Some(header_only("WWW-Authenticate", "Payment realm=\"m\""))),
        });
        let x402_body = Bytes::from_static(b"{\"x402Version\":1,\"accepts\":[]}");
        let x402: Arc<dyn ProtocolAdapter> = Arc::new(StubAdapter {
            name: "x402".into(),
            result: Ok(Some(ChallengeResponse {
                headers: header_with("WWW-Authenticate", "x402 realm=\"x\""),
                body: Some(x402_body.clone()),
            })),
        });
        let merged = merge_challenges(&[mpp, x402], &parts(), &cfg(), "GET /photos", None).await;
        // Merged carries both headers AND the x402 body.
        assert_eq!(
            merged.headers.get_all(WWW_AUTHENTICATE).iter().count(),
            2,
            "both adapters' headers must flow through"
        );
        assert_eq!(
            merged.body.as_deref(),
            Some(x402_body.as_ref()),
            "x402 body must be carried through to the merged challenge"
        );

        // build_402_response wraps it in the actual axum Response with that body.
        let resp = build_402_response(merged);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            bytes.as_ref(),
            x402_body.as_ref(),
            "build_402_response must emit the x402 body, not the RFC 9457 default"
        );
    }

    /// When no adapter contributes a body (e.g. MPP-only), the response
    /// falls back to the RFC 9457 minimal problem+json — the original
    /// behaviour, preserved.
    #[tokio::test]
    async fn no_adapter_body_falls_back_to_rfc_9457() {
        let mpp: Arc<dyn ProtocolAdapter> = Arc::new(StubAdapter {
            name: "mpp".into(),
            result: Ok(Some(header_only("WWW-Authenticate", "Payment realm=\"m\""))),
        });
        let merged = merge_challenges(&[mpp], &parts(), &cfg(), "GET /photos", None).await;
        assert!(merged.body.is_none(), "no adapter set a body");
        let resp = build_402_response(merged);
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "application/problem+json",
            "default Content-Type when falling back"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["status"], 402);
        assert_eq!(body["title"], "Payment Required");
    }
}
