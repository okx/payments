//! OKX HMAC-SHA256 signing for Facilitator API authentication.
//!
//! New module (no TS counterpart). OKX requires every Facilitator API request
//! to include OK-ACCESS-KEY/SIGN/TIMESTAMP/PASSPHRASE headers.
//!
//! Signing rule (standard OKX API):
//! ```text
//! message = timestamp + METHOD + requestPath + body
//! signature = Base64(HMAC-SHA256(secret_key, message))
//! ```

use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderValue};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Compute the OKX HMAC-SHA256 signature.
///
/// # Arguments
/// - `secret_key` - The seller's secret key
/// - `timestamp` - ISO 8601 timestamp string
/// - `method` - HTTP method in uppercase (e.g., "GET", "POST")
/// - `request_path` - Request path (e.g., "/api/v6/pay/x402/verify")
/// - `body` - Request body string (empty string for GET requests)
///
/// # Returns
/// Base64-encoded HMAC-SHA256 signature
pub(crate) fn sign_request(
    secret_key: &str,
    timestamp: &str,
    method: &str,
    request_path: &str,
    body: &str,
) -> String {
    let message = format!("{}{}{}{}", timestamp, method.to_uppercase(), request_path, body);

    let mut mac =
        HmacSha256::new_from_slice(secret_key.as_bytes()).expect("HMAC accepts any key length");
    mac.update(message.as_bytes());
    let result = mac.finalize().into_bytes();

    STANDARD.encode(result)
}

/// Build the complete set of OKX authentication headers.
///
/// # Arguments
/// - `api_key` - The seller's API key
/// - `secret_key` - The seller's secret key
/// - `passphrase` - The seller's passphrase
/// - `method` - HTTP method (e.g., "GET", "POST")
/// - `request_path` - Request path (e.g., "/api/v6/pay/x402/verify")
/// - `body` - Request body string (empty string for GET requests)
///
/// # Returns
/// HeaderMap with OK-ACCESS-KEY, OK-ACCESS-SIGN, OK-ACCESS-TIMESTAMP, OK-ACCESS-PASSPHRASE
pub fn build_auth_headers(
    api_key: &str,
    secret_key: &str,
    passphrase: &str,
    method: &str,
    request_path: &str,
    body: &str,
) -> Result<HeaderMap, crate::error::X402Error> {
    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let signature = sign_request(secret_key, &timestamp, method, request_path, body);

    let mut headers = HeaderMap::new();
    headers.insert(
        "OK-ACCESS-KEY",
        HeaderValue::from_str(api_key)
            .map_err(|e| crate::error::X402Error::Config(format!("invalid api_key: {}", e)))?,
    );
    headers.insert(
        "OK-ACCESS-SIGN",
        HeaderValue::from_str(&signature)
            .map_err(|e| crate::error::X402Error::Config(format!("invalid signature: {}", e)))?,
    );
    headers.insert(
        "OK-ACCESS-TIMESTAMP",
        HeaderValue::from_str(&timestamp)
            .map_err(|e| crate::error::X402Error::Config(format!("invalid timestamp: {}", e)))?,
    );
    headers.insert(
        "OK-ACCESS-PASSPHRASE",
        HeaderValue::from_str(passphrase)
            .map_err(|e| crate::error::X402Error::Config(format!("invalid passphrase: {}", e)))?,
    );
    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_request_deterministic() {
        let sig = sign_request(
            "test-secret",
            "2026-03-30T10:00:00.000Z",
            "POST",
            "/api/v6/pay/x402/verify",
            r#"{"x402Version":2}"#,
        );
        // Signature should be a valid base64 string
        assert!(!sig.is_empty());
        assert!(STANDARD.decode(&sig).is_ok());

        // Same input should produce the same signature
        let sig2 = sign_request(
            "test-secret",
            "2026-03-30T10:00:00.000Z",
            "POST",
            "/api/v6/pay/x402/verify",
            r#"{"x402Version":2}"#,
        );
        assert_eq!(sig, sig2);
    }

    #[test]
    fn test_build_auth_headers() {
        let headers = build_auth_headers(
            "my-api-key",
            "my-secret",
            "my-passphrase",
            "POST",
            "/api/v6/pay/x402/verify",
            "{}",
        )
        .unwrap();
        assert_eq!(headers.get("OK-ACCESS-KEY").unwrap(), "my-api-key");
        assert!(headers.get("OK-ACCESS-SIGN").is_some());
        assert!(headers.get("OK-ACCESS-TIMESTAMP").is_some());
        assert_eq!(
            headers.get("OK-ACCESS-PASSPHRASE").unwrap(),
            "my-passphrase"
        );
    }
}
