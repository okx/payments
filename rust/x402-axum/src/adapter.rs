//! Axum Request/Response adapter.
//!
//! Mirrors: `@x402/http/express/src/adapter.ts` (ExpressAdapter)
//!
//! Extracts payment-related information from Axum HTTP requests.

use axum::http::Request;

/// Extract the payment-signature header from a request.
/// Checks both "payment-signature" (v2) and "x-payment" (v1 compat).
pub fn extract_payment_header<B>(req: &Request<B>) -> Option<String> {
    req.headers()
        .get("payment-signature")
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

/// Build the route key in "METHOD /path" format for route matching.
pub fn route_key<B>(req: &Request<B>) -> String {
    format!("{} {}", extract_method(req), extract_path(req))
}
