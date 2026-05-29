//! Axum Request/Response adapter.
//!
//! Extracts payment-related information from Axum HTTP requests.

use axum::http::{Request, Response};
use x402_core::http::{SettlementOverrides, PAYMENT_SIGNATURE_HEADER, SETTLEMENT_OVERRIDES_HEADER};

/// Extract the payment-signature header from a request.
/// Checks both "payment-signature" (v2) and "x-payment" (v1 compat).
pub fn extract_payment_header<B>(req: &Request<B>) -> Option<String> {
    req.headers()
        .get(PAYMENT_SIGNATURE_HEADER)
        .or_else(|| req.headers().get("x-payment"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Extract the HTTP method as uppercase string.
pub fn extract_method<B>(req: &Request<B>) -> String {
    req.method().as_str().to_uppercase()
}

/// Extract the request path.
pub fn extract_path<B>(req: &Request<B>) -> String {
    req.uri().path().to_string()
}

/// Extract a full request URL (`scheme://host/path?query`).
///
/// Falls back to `req.uri().to_string()` (path + query only) when host
/// headers are missing — preserves the legacy single-source behavior so
/// badly-formed requests don't crash.
///
/// Header precedence:
/// - scheme: `X-Forwarded-Proto` → `req.uri().scheme_str()` → `"http"`
/// - host:   `X-Forwarded-Host` → `Host` → (none → fallback)
///
/// `X-Forwarded-*` are honored because most production deployments sit
/// behind a reverse proxy / load balancer and `req.uri()` only carries
/// path+query in axum's HTTP/1.1 server.
pub fn extract_url<B>(req: &Request<B>) -> String {
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or_else(|| req.uri().path());

    let header_str = |name: &str| -> Option<&str> {
        req.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };

    let host = header_str("x-forwarded-host").or_else(|| header_str("host"));
    let Some(host) = host else {
        return req.uri().to_string();
    };

    let scheme = header_str("x-forwarded-proto")
        .or_else(|| req.uri().scheme_str())
        .unwrap_or("http");

    format!("{scheme}://{host}{path_and_query}")
}

/// Build the route key in "METHOD /path" format for route matching.
pub fn route_key<B>(req: &Request<B>) -> String {
    format!("{} {}", extract_method(req), extract_path(req))
}

/// Set settlement overrides on the response for partial settlement.
///
/// Route handlers call this to settle less than the authorized maximum
/// (e.g., authorize $1.00, actually charge $0.30 for usage-based billing).
/// The middleware reads and removes this header before settlement.
pub fn set_settlement_overrides<B>(res: &mut Response<B>, overrides: &SettlementOverrides) {
    if let Ok(value) = serde_json::to_string(overrides) {
        if let Ok(header_value) = value.parse() {
            res.headers_mut()
                .insert(SETTLEMENT_OVERRIDES_HEADER, header_value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;

    fn req(uri: &str, headers: &[(&str, &str)]) -> Request<Body> {
        let mut builder = Request::builder().uri(uri);
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[test]
    fn extract_url_uses_host_header_when_no_forwarded_headers() {
        let r = req("/weather?city=tokyo", &[("host", "api.shop.com")]);
        assert_eq!(extract_url(&r), "http://api.shop.com/weather?city=tokyo");
    }

    #[test]
    fn extract_url_prefers_x_forwarded_proto_over_default_scheme() {
        let r = req(
            "/weather",
            &[
                ("host", "api.shop.com"),
                ("x-forwarded-proto", "https"),
            ],
        );
        assert_eq!(extract_url(&r), "https://api.shop.com/weather");
    }

    #[test]
    fn extract_url_prefers_x_forwarded_host_over_host() {
        // Reverse-proxy scenario: Host carries the internal target, the
        // real public hostname is in X-Forwarded-Host.
        let r = req(
            "/weather",
            &[
                ("host", "internal-svc:8080"),
                ("x-forwarded-host", "api.shop.com"),
                ("x-forwarded-proto", "https"),
            ],
        );
        assert_eq!(extract_url(&r), "https://api.shop.com/weather");
    }

    #[test]
    fn extract_url_falls_back_to_uri_when_host_missing() {
        // Malformed HTTP/1.0 request with no Host header — preserve legacy
        // behavior of returning path+query as-is rather than panicking.
        let r = req("/weather?city=tokyo", &[]);
        assert_eq!(extract_url(&r), "/weather?city=tokyo");
    }

    #[test]
    fn extract_url_ignores_empty_forwarded_headers() {
        // Empty X-Forwarded-Proto should not produce a "://host/path" with
        // an empty scheme; fall through to default "http".
        let r = req(
            "/weather",
            &[
                ("host", "api.shop.com"),
                ("x-forwarded-proto", ""),
            ],
        );
        assert_eq!(extract_url(&r), "http://api.shop.com/weather");
    }

    #[test]
    fn extract_url_keeps_query_string() {
        let r = req(
            "/weather?city=tokyo&unit=c",
            &[("host", "api.shop.com")],
        );
        assert_eq!(
            extract_url(&r),
            "http://api.shop.com/weather?city=tokyo&unit=c"
        );
    }
}
