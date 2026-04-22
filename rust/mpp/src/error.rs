//! SA API error types and MPP RFC 9457 conversion.
//!
//! Source: SA API error codes (8000, 70000-70014) from [Pay] MPP EVM API 方案.
//! HTTP status + problem type aligned with the 错误码对齐方案 and mpp-rs
//! `PaymentErrorDetails` conventions.

use mpp::PaymentErrorDetails;

/// Error returned by SA API.
#[derive(Debug, Clone, thiserror::Error)]
#[error("SA API error {code}: {msg}")]
pub struct SaApiError {
    pub code: u32,
    pub msg: String,
}

/// Where in the MPP problem namespace a given error lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Namespace {
    Core,
    Session,
}

impl SaApiError {
    pub fn new(code: u32, msg: impl Into<String>) -> Self {
        Self {
            code,
            msg: msg.into(),
        }
    }

    /// Convert SA API error to MPP RFC 9457 Problem Details.
    pub fn to_problem_details(&self, challenge_id: Option<&str>) -> PaymentErrorDetails {
        let (namespace, suffix, http_status, title) = map(self.code);

        let mut details = match namespace {
            Namespace::Core => PaymentErrorDetails::core(suffix),
            Namespace::Session => PaymentErrorDetails::session(suffix),
        };
        details.title = title.into();
        details.status = http_status;
        details.detail = self.msg.clone();
        details.challenge_id = challenge_id.map(|s| s.into());
        details
    }
}

/// SA API code → (namespace, suffix, HTTP status, RFC 9457 title).
fn map(code: u32) -> (Namespace, &'static str, u16, &'static str) {
    use Namespace::*;
    match code {
        // Internal SA service failure — surface as 500.
        8000 => (Core, "service-error", 500, "ServiceError"),

        // Request-level faults (400-class).
        70000 => (Core, "bad-request", 400, "BadRequestError"),
        70005 => (Core, "invalid-split", 400, "InvalidSplitError"),
        70006 => (Core, "invalid-split", 400, "InvalidSplitError"),
        70011 => (
            Session,
            "invalid-escrow-config",
            400,
            "InvalidEscrowConfigError",
        ),

        // Chain / currency support.
        70001 => (Core, "unsupported-chain", 422, "UnsupportedChainError"),

        // Access control.
        70002 => (Core, "payer-blocked", 403, "PayerBlockedError"),

        // Credential semantic errors — still 402 (Payment Required).
        70003 => (
            Core,
            "malformed-credential",
            402,
            "MalformedCredentialError",
        ),
        70004 => (Session, "invalid-signature", 402, "InvalidSignatureError"),

        // Challenge lifecycle — unauthenticated / expired.
        70009 => (Core, "payment-expired", 401, "PaymentExpiredError"),

        // On-chain verification (transaction not yet confirmed).
        70007 => (Core, "tx-not-confirmed", 402, "TxNotConfirmedError"),

        // Channel state errors.
        70008 => (Session, "channel-finalized", 410, "ChannelFinalizedError"),
        70010 => (Session, "channel-not-found", 404, "ChannelNotFoundError"),
        70012 => (
            Session,
            "amount-exceeds-deposit",
            402,
            "AmountExceedsDepositError",
        ),
        70013 => (Session, "delta-too-small", 402, "DeltaTooSmallError"),
        70014 => (Session, "channel-closing", 409, "ChannelClosingError"),

        // Unknown — fall back to a generic verification failure.
        _ => (Core, "verification-failed", 402, "VerificationFailedError"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(code: u32) -> String {
        SaApiError::new(code, "msg")
            .to_problem_details(None)
            .problem_type
    }

    fn status(code: u32) -> u16 {
        SaApiError::new(code, "msg").to_problem_details(None).status
    }

    #[test]
    fn service_error_is_500() {
        assert_eq!(status(8000), 500);
        assert_eq!(uri(8000), "https://paymentauth.org/problems/service-error");
    }

    #[test]
    fn bad_request_is_400_core_namespace() {
        assert_eq!(status(70000), 400);
        assert!(uri(70000).ends_with("/problems/bad-request"));
        assert!(!uri(70000).contains("/session/"));
    }

    #[test]
    fn invalid_split_codes_share_suffix() {
        assert_eq!(uri(70005), uri(70006));
        assert_eq!(status(70005), 400);
    }

    #[test]
    fn unsupported_chain_422() {
        assert_eq!(status(70001), 422);
    }

    #[test]
    fn payer_blocked_403() {
        assert_eq!(status(70002), 403);
    }

    #[test]
    fn invalid_signature_goes_to_session_namespace() {
        let u = uri(70004);
        assert!(u.contains("/problems/session/"), "got {u}");
        assert!(u.ends_with("/invalid-signature"));
    }

    #[test]
    fn payment_expired_401() {
        assert_eq!(status(70009), 401);
    }

    #[test]
    fn channel_finalized_410() {
        assert_eq!(status(70008), 410);
        assert!(uri(70008).contains("/session/channel-finalized"));
    }

    #[test]
    fn channel_not_found_404() {
        assert_eq!(status(70010), 404);
    }

    #[test]
    fn channel_closing_409() {
        assert_eq!(status(70014), 409);
        assert!(uri(70014).ends_with("/channel-closing"));
    }

    #[test]
    fn amount_exceeds_deposit_402_session() {
        assert_eq!(status(70012), 402);
        assert!(uri(70012).contains("/session/amount-exceeds-deposit"));
    }

    #[test]
    fn unknown_code_falls_back_to_verification_failed() {
        assert_eq!(status(99999), 402);
        assert!(uri(99999).ends_with("/verification-failed"));
    }

    #[test]
    fn challenge_id_is_passed_through() {
        let d = SaApiError::new(70004, "bad sig").to_problem_details(Some("chal-1"));
        assert_eq!(d.challenge_id.as_deref(), Some("chal-1"));
        assert_eq!(d.detail, "bad sig");
    }

    #[test]
    fn all_16_documented_codes_are_mapped() {
        // 8000 + 70000..=70014 = 16 codes, none should fall through to default.
        for code in [8000u32].into_iter().chain(70000..=70014) {
            let u = uri(code);
            assert!(
                !u.ends_with("/verification-failed"),
                "code {code} fell through to default mapping"
            );
        }
    }
}
