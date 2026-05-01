//! Axum Request/Response adapter.
//!
//! Mirrors: `@x402/http/express/src/adapter.ts` (ExpressAdapter)
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

/// Build the route key in "METHOD /path" format for route matching.
pub fn route_key<B>(req: &Request<B>) -> String {
    format!("{} {}", extract_method(req), extract_path(req))
}

/// Set settlement overrides on the response for partial settlement.
///
/// Route handlers call this to settle less than the authorized maximum
/// (e.g., authorize $1.00, actually charge $0.30 for usage-based billing).
/// The middleware reads and removes this header before settlement.
///
/// Mirrors TS: `setSettlementOverrides(res, overrides)`
/// Mirrors Go: `SetSettlementOverrides(c, overrides)`
pub fn set_settlement_overrides<B>(res: &mut Response<B>, overrides: &SettlementOverrides) {
    if let Ok(value) = serde_json::to_string(overrides) {
        if let Ok(header_value) = value.parse() {
            res.headers_mut()
                .insert(SETTLEMENT_OVERRIDES_HEADER, header_value);
        }
    }
}
