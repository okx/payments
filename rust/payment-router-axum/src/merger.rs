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
use futures::future::join_all;
use http::{HeaderMap, Response, StatusCode, header::CONTENT_TYPE, request::Parts};

use crate::adapter::ProtocolAdapter;
use crate::types::{ErrorContext, ErrorHandler, ErrorPhase, UnifiedRouteConfig};

/// Run `get_challenge` on all adapters in parallel, merge resulting headers.
///
/// Returns a merged `HeaderMap` with (possibly multiple) `WWW-Authenticate`
/// entries, one per successful adapter. Failures are reported via `on_error`.
pub(crate) async fn merge_challenges(
    adapters: &[Arc<dyn ProtocolAdapter>],
    parts: &Parts,
    route_cfg: &UnifiedRouteConfig,
    route_key: &str,
    on_error: Option<&Arc<ErrorHandler>>,
) -> HeaderMap {
    // Build futures that each carry the adapter name for error reporting.
    let futures = adapters.iter().map(|adapter| async move {
        let name = adapter.name().to_string();
        // If this adapter has no config for this route, skip it entirely (spec §3 #4).
        let cfg = match route_cfg.adapter_configs.get(&name) {
            Some(v) => v,
            None => return (name, Ok(None)),
        };
        let result = adapter.get_challenge(parts, cfg).await;
        (name, result)
    });

    let results = join_all(futures).await;

    let mut merged = HeaderMap::new();
    for (protocol, result) in results {
        match result {
            Ok(Some(headers)) => {
                for (name, value) in headers.iter() {
                    // append = multi-row semantics (spec §9: no comma concat).
                    merged.append(name.clone(), value.clone());
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
    merged
}

/// Assemble the 402 response from merged challenge headers.
///
/// Body is RFC 9457 problem+json minimal form. Content-Type set explicitly.
pub(crate) fn build_402_response(headers: HeaderMap) -> Response<Body> {
    let body = serde_json::json!({
        "type": "about:blank",
        "title": "Payment Required",
        "status": 402,
    });
    let body_bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    let mut resp = Response::builder()
        .status(StatusCode::PAYMENT_REQUIRED)
        .header(CONTENT_TYPE, "application/problem+json")
        .body(Body::from(body_bytes))
        .unwrap_or_else(|_| {
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
    use crate::adapter::{ChallengeFuture, InnerService, ProtocolAdapter};
    use http::{HeaderValue, Request, header::WWW_AUTHENTICATE};
    use serde_json::Value;

    struct StubAdapter {
        name: String,
        result: Result<Option<HeaderMap>, String>,
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
            result: Ok(Some(header_with("WWW-Authenticate", "Payment realm=\"m\""))),
        });
        let x402: Arc<dyn ProtocolAdapter> = Arc::new(StubAdapter {
            name: "x402".into(),
            result: Ok(Some(header_with("WWW-Authenticate", "x402 realm=\"x\""))),
        });
        let merged = merge_challenges(&[mpp, x402], &parts(), &cfg(), "GET /photos", None).await;
        let values: Vec<_> = merged
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
            result: Ok(Some(header_with("WWW-Authenticate", "x402 realm=\"x\""))),
        });
        let merged = merge_challenges(&[mpp, x402], &parts(), &cfg(), "GET /photos", None).await;
        let values: Vec<_> = merged
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
            result: Ok(Some(header_with("WWW-Authenticate", "Payment should-not-appear"))),
        });
        let x402: Arc<dyn ProtocolAdapter> = Arc::new(StubAdapter {
            name: "x402".into(),
            result: Ok(Some(header_with("WWW-Authenticate", "x402 realm=\"x\""))),
        });
        let merged = merge_challenges(&[mpp, x402], &parts(), &cfg, "GET /photos", None).await;
        let values: Vec<_> = merged
            .get_all(WWW_AUTHENTICATE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert_eq!(values.len(), 1);
        assert!(values[0].contains("x402"));
        assert!(!values[0].contains("should-not-appear"));
    }
}
