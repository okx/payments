//! Convenience extensions for [`mpp::protocol::core::PaymentCredential`].
//!
//! Upstream `PaymentCredential` exposes `challenge.request: Base64UrlJson<T>`
//! whose `.decode()` returns a generic error. Merchants typically want to
//! decode the request body into a concrete intent struct (e.g.
//! [`SessionRequest`](mpp::protocol::intents::SessionRequest) or
//! [`ChargeRequest`](mpp::protocol::intents::ChargeRequest)) and route by
//! `?` on `SaApiError`.
//!
//! ```ignore
//! use mpp_evm::CredentialExt;
//! use mpp::protocol::intents::SessionRequest;
//!
//! let request: SessionRequest = credential.decode_request()?;
//! ```

use mpp::protocol::core::PaymentCredential;
use serde::de::DeserializeOwned;

use crate::error::SaApiError;

/// Extension methods for [`PaymentCredential`].
pub trait CredentialExt {
    /// Decode `challenge.request` (base64url-JCS-encoded JSON) into a typed
    /// intent struct. Errors are normalised to [`SaApiError`] (code 70000) so
    /// the caller can chain via `?` against other SDK calls.
    fn decode_request<R: DeserializeOwned>(&self) -> Result<R, SaApiError>;
}

impl CredentialExt for PaymentCredential {
    fn decode_request<R: DeserializeOwned>(&self) -> Result<R, SaApiError> {
        self.challenge
            .request
            .decode()
            .map_err(|e| SaApiError::new(70000, format!("decode challenge.request: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp::protocol::core::{Base64UrlJson, ChallengeEcho};
    use mpp::protocol::intents::SessionRequest;
    use serde_json::json;

    fn fixture_credential(request_body: serde_json::Value) -> PaymentCredential {
        PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-1".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "session".into(),
                request: Base64UrlJson::from_value(&request_body).unwrap(),
                expires: None,
                digest: None,
                opaque: None,
            },
            source: None,
            payload: json!({}),
        }
    }

    #[test]
    fn decode_request_returns_typed_session_request() {
        let body = json!({
            "amount": "100",
            "currency": "0x74b7F16337b8972027F6196A17a631aC6dE26d22",
            "recipient": "0xb483abdb92f8061e9a3a082a4aaaa6b88c381308",
        });
        let credential = fixture_credential(body);
        let request: SessionRequest = credential.decode_request().unwrap();
        assert_eq!(request.amount, "100");
    }

    #[test]
    fn decode_request_maps_error_to_70000() {
        // Garbage that won't deserialize into SessionRequest.
        let body = json!({ "this": "is wrong" });
        let credential = fixture_credential(body);
        let err = credential
            .decode_request::<SessionRequest>()
            .expect_err("expected decode error");
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("decode challenge.request"));
    }
}
