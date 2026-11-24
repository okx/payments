package errors

import (
	"strings"
	"testing"
)

// ─────────────────────────────────────────────────────────────────────────────
// MppErrorCode constants
// ─────────────────────────────────────────────────────────────────────────────

func TestMppErrorCodeConstants(t *testing.T) {
	cases := []struct {
		code MppErrorCode
		want string
	}{
		{CodeAmountExceedsMax, "AmountExceedsMax"},
		{CodeInvalidAmount, "InvalidAmount"},
		{CodeInvalidConfig, "InvalidConfig"},
		{CodeHttp, "Http"},
		{CodeChainIdMismatch, "ChainIdMismatch"},
		{CodeJson, "Json"},
		{CodeHexDecode, "HexDecode"},
		{CodeBase64Decode, "Base64Decode"},
		{CodeUnsupportedPaymentMethod, "UnsupportedPaymentMethod"},
		{CodeMissingHeader, "MissingHeader"},
		{CodeInvalidBase64Url, "InvalidBase64Url"},
		{CodeMalformedCredential, "MalformedCredential"},
		{CodeInvalidChallenge, "InvalidChallenge"},
		{CodeVerificationFailed, "VerificationFailed"},
		{CodePaymentExpired, "PaymentExpired"},
		{CodePaymentRequired, "PaymentRequired"},
		{CodeInvalidPayload, "InvalidPayload"},
		{CodeBadRequest, "BadRequest"},
		{CodeInsufficientBalance, "InsufficientBalance"},
		{CodeInvalidSignature, "InvalidSignature"},
		{CodeSignerMismatch, "SignerMismatch"},
		{CodeAmountExceedsDeposit, "AmountExceedsDeposit"},
		{CodeDeltaTooSmall, "DeltaTooSmall"},
		{CodeChannelNotFound, "ChannelNotFound"},
		{CodeChannelClosed, "ChannelClosed"},
		{CodeIo, "Io"},
		{CodeInvalidUtf8, "InvalidUtf8"},
		{CodeSystemTime, "SystemTime"},
		{CodeInternal, "Internal"},
	}
	for _, tc := range cases {
		t.Run(tc.want, func(t *testing.T) {
			if string(tc.code) != tc.want {
				t.Errorf("got %q, want %q", string(tc.code), tc.want)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// MppError.Error()
// ─────────────────────────────────────────────────────────────────────────────

func TestMppErrorError(t *testing.T) {
	t.Run("with reason", func(t *testing.T) {
		e := &MppError{Code: CodeBadRequest, Message: "Bad request", Reason: "missing field"}
		want := "Bad request: missing field"
		if e.Error() != want {
			t.Errorf("got %q, want %q", e.Error(), want)
		}
	})
	t.Run("without reason", func(t *testing.T) {
		e := &MppError{Code: CodeBadRequest, Message: "Bad request"}
		if e.Error() != "Bad request" {
			t.Errorf("got %q, want %q", e.Error(), "Bad request")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// ProblemTypeSuffix
// ─────────────────────────────────────────────────────────────────────────────

func TestProblemTypeSuffix(t *testing.T) {
	cases := []struct {
		code MppErrorCode
		want string
	}{
		{CodeMalformedCredential, "malformed-credential"},
		{CodeInvalidChallenge, "invalid-challenge"},
		{CodeVerificationFailed, "verification-failed"},
		{CodePaymentExpired, "payment-expired"},
		{CodeInvalidPayload, "invalid-payload"},
		{CodeBadRequest, "bad-request"},
		{CodeInsufficientBalance, "insufficient-balance"},
		{CodeInvalidSignature, "invalid-signature"},
		{CodeSignerMismatch, "signer-mismatch"},
		{CodeAmountExceedsDeposit, "amount-exceeds-deposit"},
		{CodeDeltaTooSmall, "delta-too-small"},
		{CodeChannelNotFound, "channel-not-found"},
		{CodeChannelClosed, "channel-finalized"},
		// non-payment codes return ""
		{CodePaymentRequired, ""},
		{CodeInternal, ""},
		{CodeBadRequest, "bad-request"}, // already tested above; verify no clash
	}
	for _, tc := range cases {
		t.Run(string(tc.code), func(t *testing.T) {
			e := &MppError{Code: tc.code}
			got := e.ProblemTypeSuffix()
			if got != tc.want {
				t.Errorf("ProblemTypeSuffix() for %s = %q, want %q", tc.code, got, tc.want)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// IsPaymentProblem
// ─────────────────────────────────────────────────────────────────────────────

func TestIsPaymentProblem(t *testing.T) {
	trueCodesRaw := []MppErrorCode{
		CodeMalformedCredential, CodeInvalidChallenge, CodeVerificationFailed,
		CodePaymentExpired, CodeInvalidPayload, CodeBadRequest,
		CodeInsufficientBalance, CodeInvalidSignature, CodeSignerMismatch,
		CodeAmountExceedsDeposit, CodeDeltaTooSmall, CodeChannelNotFound,
		CodeChannelClosed,
	}
	for _, code := range trueCodesRaw {
		e := &MppError{Code: code}
		if !e.IsPaymentProblem() {
			t.Errorf("IsPaymentProblem() for %s should be true", code)
		}
	}

	falseCodes := []MppErrorCode{CodePaymentRequired, CodeInternal}
	for _, code := range falseCodes {
		e := &MppError{Code: code}
		if e.IsPaymentProblem() {
			t.Errorf("IsPaymentProblem() for %s should be false", code)
		}
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ToProblemDetails
// ─────────────────────────────────────────────────────────────────────────────

func TestToProblemDetails(t *testing.T) {
	t.Run("core payment error uses CoreProblemTypeBase", func(t *testing.T) {
		e := &MppError{Code: CodeVerificationFailed, Message: "fail"}
		d := e.ToProblemDetails("")
		if !strings.HasPrefix(d.ProblemType, CoreProblemTypeBase) {
			t.Errorf("type %q should start with CoreProblemTypeBase", d.ProblemType)
		}
		if d.Status != 402 {
			t.Errorf("status = %d, want 402", d.Status)
		}
	})

	t.Run("session error uses SessionProblemTypeBase", func(t *testing.T) {
		e := &MppError{Code: CodeInsufficientBalance, Message: "nope"}
		d := e.ToProblemDetails("")
		if !strings.HasPrefix(d.ProblemType, SessionProblemTypeBase) {
			t.Errorf("type %q should start with SessionProblemTypeBase", d.ProblemType)
		}
		if d.Status != 402 {
			t.Errorf("status = %d, want 402", d.Status)
		}
	})

	t.Run("bad request status 400", func(t *testing.T) {
		e := &MppError{Code: CodeBadRequest, Message: "bad"}
		d := e.ToProblemDetails("")
		if d.Status != 400 {
			t.Errorf("status = %d, want 400", d.Status)
		}
	})

	t.Run("channel not found status 410", func(t *testing.T) {
		e := &MppError{Code: CodeChannelNotFound, Message: "nf"}
		d := e.ToProblemDetails("")
		if d.Status != 410 {
			t.Errorf("status = %d, want 410", d.Status)
		}
	})

	t.Run("channel closed status 410 and suffix channel-finalized", func(t *testing.T) {
		e := &MppError{Code: CodeChannelClosed, Message: "closed"}
		d := e.ToProblemDetails("")
		if d.Status != 410 {
			t.Errorf("status = %d, want 410", d.Status)
		}
		if !strings.HasSuffix(d.ProblemType, "channel-finalized") {
			t.Errorf("type %q should end with channel-finalized", d.ProblemType)
		}
	})

	t.Run("non-payment error returns 500 internal-error", func(t *testing.T) {
		e := &MppError{Code: CodeInternal, Message: "oops"}
		d := e.ToProblemDetails("")
		if d.Status != 500 {
			t.Errorf("status = %d, want 500", d.Status)
		}
		if !strings.Contains(d.ProblemType, "internal-error") {
			t.Errorf("type %q should contain internal-error", d.ProblemType)
		}
	})

	t.Run("challenge ID set when non-empty", func(t *testing.T) {
		e := &MppError{Code: CodeVerificationFailed, Message: "fail"}
		d := e.ToProblemDetails("chal-abc")
		if d.ChallengeID != "chal-abc" {
			t.Errorf("challengeId = %q, want %q", d.ChallengeID, "chal-abc")
		}
	})

	t.Run("challenge ID empty when not provided", func(t *testing.T) {
		e := &MppError{Code: CodeVerificationFailed, Message: "fail"}
		d := e.ToProblemDetails("")
		if d.ChallengeID != "" {
			t.Errorf("challengeId = %q, want empty", d.ChallengeID)
		}
	})
}

func TestToProblemDetailsNoChallengeID(t *testing.T) {
	e := &MppError{Code: CodeMalformedCredential, Message: "cred bad"}
	d := e.ToProblemDetails("")
	if d.ChallengeID != "" {
		t.Errorf("ChallengeID should be empty when empty string passed, got %q", d.ChallengeID)
	}
	if d.Title != "MalformedCredentialError" {
		t.Errorf("Title = %q, want MalformedCredentialError", d.Title)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Factory functions
// ─────────────────────────────────────────────────────────────────────────────

func TestFactoryFunctions(t *testing.T) {
	t.Run("NewMalformedCredentialError with reason", func(t *testing.T) {
		e := NewMalformedCredentialError("bad field")
		if e.Code != CodeMalformedCredential {
			t.Errorf("code = %q", e.Code)
		}
		if e.Reason != "bad field" {
			t.Errorf("reason = %q", e.Reason)
		}
		if !strings.Contains(e.Message, "bad field") {
			t.Errorf("message %q should contain reason", e.Message)
		}
	})
	t.Run("NewMalformedCredentialError without reason", func(t *testing.T) {
		e := NewMalformedCredentialError("")
		if e.Message != "Credential is malformed." {
			t.Errorf("message = %q", e.Message)
		}
	})

	t.Run("NewInvalidChallengeError both", func(t *testing.T) {
		e := NewInvalidChallengeError("abc", "expired")
		if e.Code != CodeInvalidChallenge {
			t.Errorf("code = %q", e.Code)
		}
		if !strings.Contains(e.Message, "abc") || !strings.Contains(e.Message, "expired") {
			t.Errorf("message %q should contain id and reason", e.Message)
		}
	})
	t.Run("NewInvalidChallengeError id only", func(t *testing.T) {
		e := NewInvalidChallengeError("abc", "")
		if !strings.Contains(e.Message, "abc") {
			t.Errorf("message %q should contain id", e.Message)
		}
	})
	t.Run("NewInvalidChallengeError reason only", func(t *testing.T) {
		e := NewInvalidChallengeError("", "some reason")
		if !strings.Contains(e.Message, "some reason") {
			t.Errorf("message %q should contain reason", e.Message)
		}
	})
	t.Run("NewInvalidChallengeError neither", func(t *testing.T) {
		e := NewInvalidChallengeError("", "")
		if e.Message != "Challenge is invalid." {
			t.Errorf("message = %q", e.Message)
		}
	})

	t.Run("NewVerificationFailedError", func(t *testing.T) {
		e := NewVerificationFailedError("sig mismatch")
		if e.Code != CodeVerificationFailed {
			t.Errorf("code = %q", e.Code)
		}
		if !strings.Contains(e.Message, "sig mismatch") {
			t.Errorf("message %q should contain reason", e.Message)
		}
	})

	t.Run("NewPaymentExpiredError with expires", func(t *testing.T) {
		e := NewPaymentExpiredError("2024-01-01T00:00:00Z")
		if e.Code != CodePaymentExpired {
			t.Errorf("code = %q", e.Code)
		}
		if !strings.Contains(e.Message, "2024-01-01T00:00:00Z") {
			t.Errorf("message %q should contain expires", e.Message)
		}
	})
	t.Run("NewPaymentExpiredError without expires", func(t *testing.T) {
		e := NewPaymentExpiredError("")
		if e.Message != "Payment has expired." {
			t.Errorf("message = %q", e.Message)
		}
	})

	t.Run("NewPaymentRequiredError with realm and desc", func(t *testing.T) {
		e := NewPaymentRequiredError("myrealm", "pay now")
		if e.Code != CodePaymentRequired {
			t.Errorf("code = %q", e.Code)
		}
		if !strings.Contains(e.Message, "myrealm") || !strings.Contains(e.Message, "pay now") {
			t.Errorf("message %q should contain realm and desc", e.Message)
		}
	})
	t.Run("NewPaymentRequiredError empty", func(t *testing.T) {
		e := NewPaymentRequiredError("", "")
		if e.Message != "Payment is required." {
			t.Errorf("message = %q", e.Message)
		}
	})

	t.Run("NewInvalidPayloadError", func(t *testing.T) {
		e := NewInvalidPayloadError("corrupt")
		if e.Code != CodeInvalidPayload {
			t.Errorf("code = %q", e.Code)
		}
	})
	t.Run("NewInvalidPayloadError empty", func(t *testing.T) {
		e := NewInvalidPayloadError("")
		if e.Message != "Credential payload is invalid." {
			t.Errorf("message = %q", e.Message)
		}
	})

	t.Run("NewBadRequestError", func(t *testing.T) {
		e := NewBadRequestError("missing id")
		if e.Code != CodeBadRequest {
			t.Errorf("code = %q", e.Code)
		}
	})

	t.Run("NewInsufficientBalanceError", func(t *testing.T) {
		e := NewInsufficientBalanceError("no funds")
		if e.Code != CodeInsufficientBalance {
			t.Errorf("code = %q", e.Code)
		}
	})

	t.Run("NewInvalidSignatureError", func(t *testing.T) {
		e := NewInvalidSignatureError("bad sig")
		if e.Code != CodeInvalidSignature {
			t.Errorf("code = %q", e.Code)
		}
	})

	t.Run("NewSignerMismatchError with reason", func(t *testing.T) {
		e := NewSignerMismatchError("wrong signer")
		if e.Code != CodeSignerMismatch {
			t.Errorf("code = %q", e.Code)
		}
		if !strings.Contains(e.Message, "wrong signer") {
			t.Errorf("message %q should contain reason", e.Message)
		}
	})
	t.Run("NewSignerMismatchError without reason", func(t *testing.T) {
		e := NewSignerMismatchError("")
		if !strings.Contains(e.Message, "authorized") {
			t.Errorf("message %q should mention authorized", e.Message)
		}
	})

	t.Run("NewAmountExceedsDepositError", func(t *testing.T) {
		e := NewAmountExceedsDepositError("too much")
		if e.Code != CodeAmountExceedsDeposit {
			t.Errorf("code = %q", e.Code)
		}
	})

	t.Run("NewDeltaTooSmallError", func(t *testing.T) {
		e := NewDeltaTooSmallError("tiny")
		if e.Code != CodeDeltaTooSmall {
			t.Errorf("code = %q", e.Code)
		}
	})

	t.Run("NewChannelNotFoundError with reason", func(t *testing.T) {
		e := NewChannelNotFoundError("id-xyz")
		if e.Code != CodeChannelNotFound {
			t.Errorf("code = %q", e.Code)
		}
		if !strings.Contains(e.Message, "id-xyz") {
			t.Errorf("message %q should contain reason", e.Message)
		}
	})
	t.Run("NewChannelNotFoundError without reason", func(t *testing.T) {
		e := NewChannelNotFoundError("")
		if !strings.Contains(e.Message, "No channel") {
			t.Errorf("message %q should say No channel", e.Message)
		}
	})

	t.Run("NewChannelClosedError", func(t *testing.T) {
		e := NewChannelClosedError("final")
		if e.Code != CodeChannelClosed {
			t.Errorf("code = %q", e.Code)
		}
	})
	t.Run("NewChannelClosedError without reason", func(t *testing.T) {
		e := NewChannelClosedError("")
		if !strings.Contains(e.Message, "closed") {
			t.Errorf("message %q should mention closed", e.Message)
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// PaymentErrorDetails builder chain
// ─────────────────────────────────────────────────────────────────────────────

func TestPaymentErrorDetailsBuilder(t *testing.T) {
	d := NewPaymentErrorDetails("https://example.com/err").
		WithTitle("MyError").
		WithStatus(403).
		WithDetail("something went wrong").
		WithChallengeID("chal-001")

	if d.ProblemType != "https://example.com/err" {
		t.Errorf("ProblemType = %q", d.ProblemType)
	}
	if d.Title != "MyError" {
		t.Errorf("Title = %q", d.Title)
	}
	if d.Status != 403 {
		t.Errorf("Status = %d", d.Status)
	}
	if d.Detail != "something went wrong" {
		t.Errorf("Detail = %q", d.Detail)
	}
	if d.ChallengeID != "chal-001" {
		t.Errorf("ChallengeID = %q", d.ChallengeID)
	}
}

func TestNewPaymentErrorDetailsDefaultStatus(t *testing.T) {
	d := NewPaymentErrorDetails("https://example.com/err")
	if d.Status != 402 {
		t.Errorf("default Status = %d, want 402", d.Status)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// CorePaymentError / SessionPaymentError
// ─────────────────────────────────────────────────────────────────────────────

func TestCorePaymentError(t *testing.T) {
	d := CorePaymentError("my-suffix")
	if d.ProblemType != CoreProblemTypeBase+"my-suffix" {
		t.Errorf("ProblemType = %q, want %q", d.ProblemType, CoreProblemTypeBase+"my-suffix")
	}
}

func TestSessionPaymentError(t *testing.T) {
	d := SessionPaymentError("my-suffix")
	if d.ProblemType != SessionProblemTypeBase+"my-suffix" {
		t.Errorf("ProblemType = %q, want %q", d.ProblemType, SessionProblemTypeBase+"my-suffix")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// PaymentError interface compliance
// ─────────────────────────────────────────────────────────────────────────────

func TestPaymentErrorInterfaceCompliance(t *testing.T) {
	// compile-time check that *MppError implements PaymentError
	var _ PaymentError = (*MppError)(nil)
}
