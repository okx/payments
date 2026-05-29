// Package errors provides error types for the MPP (Machine Payments Protocol) SDK.
//
// It includes:
//   - MppError: the main error type for all mpp operations
//   - PaymentErrorDetails: RFC 9457 Problem Details format for HTTP error responses
//   - PaymentError: interface for converting errors to Problem Details
package errors

import "fmt"

// CoreProblemTypeBase is the base URI for core payment-auth problem types.
const CoreProblemTypeBase = "https://payment-auth.org/problems/"

// SessionProblemTypeBase is the base URI for session/channel problem types.
const SessionProblemTypeBase = "https://payment-auth.org/problems/session/"

// MppErrorCode is a string type representing a specific MPP error code.
type MppErrorCode string

const (
	CodeAmountExceedsMax        MppErrorCode = "AmountExceedsMax"
	CodeInvalidAmount           MppErrorCode = "InvalidAmount"
	CodeInvalidConfig           MppErrorCode = "InvalidConfig"
	CodeHttp                    MppErrorCode = "Http"
	CodeChainIdMismatch         MppErrorCode = "ChainIdMismatch"
	CodeJson                    MppErrorCode = "Json"
	CodeHexDecode               MppErrorCode = "HexDecode"
	CodeBase64Decode            MppErrorCode = "Base64Decode"
	CodeUnsupportedPaymentMethod MppErrorCode = "UnsupportedPaymentMethod"
	CodeMissingHeader           MppErrorCode = "MissingHeader"
	CodeInvalidBase64Url        MppErrorCode = "InvalidBase64Url"
	CodeMalformedCredential     MppErrorCode = "MalformedCredential"
	CodeInvalidChallenge        MppErrorCode = "InvalidChallenge"
	CodeVerificationFailed      MppErrorCode = "VerificationFailed"
	CodePaymentExpired          MppErrorCode = "PaymentExpired"
	CodePaymentRequired         MppErrorCode = "PaymentRequired"
	CodeInvalidPayload          MppErrorCode = "InvalidPayload"
	CodeBadRequest              MppErrorCode = "BadRequest"
	CodeInsufficientBalance     MppErrorCode = "InsufficientBalance"
	CodeInvalidSignature        MppErrorCode = "InvalidSignature"
	CodeSignerMismatch          MppErrorCode = "SignerMismatch"
	CodeAmountExceedsDeposit    MppErrorCode = "AmountExceedsDeposit"
	CodeDeltaTooSmall           MppErrorCode = "DeltaTooSmall"
	CodeChannelNotFound         MppErrorCode = "ChannelNotFound"
	CodeChannelClosed           MppErrorCode = "ChannelClosed"
	CodeIo                      MppErrorCode = "Io"
	CodeInvalidUtf8             MppErrorCode = "InvalidUtf8"
	CodeSystemTime              MppErrorCode = "SystemTime"
	CodeInternal                MppErrorCode = "Internal"
)

// MppError is the main error type for all MPP operations.
type MppError struct {
	Code    MppErrorCode `json:"code"`
	Message string       `json:"message"`
	Reason  string       `json:"reason,omitempty"`
}

// Error implements the error interface.
func (e *MppError) Error() string {
	if e.Reason != "" {
		return fmt.Sprintf("%s: %s", e.Message, e.Reason)
	}
	return e.Message
}

// ProblemTypeSuffix returns the RFC 9457 problem type suffix for payment problems.
// Returns an empty string if this error is not a payment problem.
// For session/channel errors the suffix is relative to SessionProblemTypeBase.
// For core payment-auth errors the suffix is relative to CoreProblemTypeBase.
func (e *MppError) ProblemTypeSuffix() string {
	switch e.Code {
	case CodeMalformedCredential:
		return "malformed-credential"
	case CodeInvalidChallenge:
		return "invalid-challenge"
	case CodeVerificationFailed:
		return "verification-failed"
	case CodePaymentExpired:
		return "payment-expired"
	case CodeInvalidPayload:
		return "invalid-payload"
	case CodeBadRequest:
		return "bad-request"
	case CodeInsufficientBalance:
		return "insufficient-balance"
	case CodeInvalidSignature:
		return "invalid-signature"
	case CodeSignerMismatch:
		return "signer-mismatch"
	case CodeAmountExceedsDeposit:
		return "amount-exceeds-deposit"
	case CodeDeltaTooSmall:
		return "delta-too-small"
	case CodeChannelNotFound:
		return "channel-not-found"
	case CodeChannelClosed:
		return "channel-finalized"
	default:
		return ""
	}
}

// IsPaymentProblem returns true if this error is an RFC 9457 payment problem.
func (e *MppError) IsPaymentProblem() bool {
	return e.ProblemTypeSuffix() != ""
}

// isSessionError returns true if this error belongs to the session/channel category.
func (e *MppError) isSessionError() bool {
	switch e.Code {
	case CodeInsufficientBalance,
		CodeInvalidSignature,
		CodeSignerMismatch,
		CodeAmountExceedsDeposit,
		CodeDeltaTooSmall,
		CodeChannelNotFound,
		CodeChannelClosed:
		return true
	default:
		return false
	}
}

// ToProblemDetails converts this error to RFC 9457 Problem Details format.
// Implements the PaymentError interface.
func (e *MppError) ToProblemDetails(challengeID string) *PaymentErrorDetails {
	suffix := e.ProblemTypeSuffix()
	if suffix == "" {
		// Non-payment-problem: return a generic internal error
		d := CorePaymentError("internal-error").
			WithTitle("InternalError").
			WithStatus(500).
			WithDetail(e.Error())
		if challengeID != "" {
			d = d.WithChallengeID(challengeID)
		}
		return d
	}

	var d *PaymentErrorDetails
	if e.isSessionError() {
		d = SessionPaymentError(suffix)
	} else {
		d = CorePaymentError(suffix)
	}

	// Assign title and status per error code.
	switch e.Code {
	case CodeMalformedCredential:
		d = d.WithTitle("MalformedCredentialError").WithStatus(402)
	case CodeInvalidChallenge:
		d = d.WithTitle("InvalidChallengeError").WithStatus(402)
	case CodeVerificationFailed:
		d = d.WithTitle("VerificationFailedError").WithStatus(402)
	case CodePaymentExpired:
		d = d.WithTitle("PaymentExpiredError").WithStatus(402)
	case CodeInvalidPayload:
		d = d.WithTitle("InvalidPayloadError").WithStatus(402)
	case CodeBadRequest:
		d = d.WithTitle("BadRequestError").WithStatus(400)
	case CodeInsufficientBalance:
		d = d.WithTitle("InsufficientBalanceError").WithStatus(402)
	case CodeInvalidSignature:
		d = d.WithTitle("InvalidSignatureError").WithStatus(402)
	case CodeSignerMismatch:
		d = d.WithTitle("SignerMismatchError").WithStatus(402)
	case CodeAmountExceedsDeposit:
		d = d.WithTitle("AmountExceedsDepositError").WithStatus(402)
	case CodeDeltaTooSmall:
		d = d.WithTitle("DeltaTooSmallError").WithStatus(402)
	case CodeChannelNotFound:
		d = d.WithTitle("ChannelNotFoundError").WithStatus(410)
	case CodeChannelClosed:
		d = d.WithTitle("ChannelClosedError").WithStatus(410)
	}

	d = d.WithDetail(e.Error())

	if challengeID != "" {
		d = d.WithChallengeID(challengeID)
	}
	return d
}

// ==================== Factory Functions ====================

// NewMalformedCredentialError creates a malformed credential error.
func NewMalformedCredentialError(reason string) *MppError {
	msg := "Credential is malformed"
	if reason != "" {
		msg = fmt.Sprintf("Credential is malformed: %s.", reason)
	} else {
		msg = "Credential is malformed."
	}
	return &MppError{Code: CodeMalformedCredential, Message: msg, Reason: reason}
}

// NewInvalidChallengeError creates an invalid challenge error with id and reason.
func NewInvalidChallengeError(id, reason string) *MppError {
	var msg string
	switch {
	case id != "" && reason != "":
		msg = fmt.Sprintf("Challenge \"%s\" is invalid: %s.", id, reason)
	case id != "":
		msg = fmt.Sprintf("Challenge \"%s\" is invalid.", id)
	case reason != "":
		msg = fmt.Sprintf("Challenge is invalid: %s.", reason)
	default:
		msg = "Challenge is invalid."
	}
	return &MppError{Code: CodeInvalidChallenge, Message: msg, Reason: reason}
}

// NewVerificationFailedError creates a verification failed error.
func NewVerificationFailedError(reason string) *MppError {
	var msg string
	if reason != "" {
		msg = fmt.Sprintf("Payment verification failed: %s.", reason)
	} else {
		msg = "Payment verification failed."
	}
	return &MppError{Code: CodeVerificationFailed, Message: msg, Reason: reason}
}

// NewPaymentExpiredError creates a payment expired error with an expiration timestamp.
func NewPaymentExpiredError(expires string) *MppError {
	var msg string
	if expires != "" {
		msg = fmt.Sprintf("Payment expired at %s.", expires)
	} else {
		msg = "Payment has expired."
	}
	return &MppError{Code: CodePaymentExpired, Message: msg, Reason: expires}
}

// NewPaymentRequiredError creates a payment required error with realm and description.
func NewPaymentRequiredError(realm, description string) *MppError {
	msg := "Payment is required"
	if realm != "" {
		msg += fmt.Sprintf(" for \"%s\"", realm)
	}
	if description != "" {
		msg += fmt.Sprintf(" (%s)", description)
	}
	msg += "."
	return &MppError{Code: CodePaymentRequired, Message: msg, Reason: description}
}

// NewInvalidPayloadError creates an invalid payload error.
func NewInvalidPayloadError(reason string) *MppError {
	var msg string
	if reason != "" {
		msg = fmt.Sprintf("Credential payload is invalid: %s.", reason)
	} else {
		msg = "Credential payload is invalid."
	}
	return &MppError{Code: CodeInvalidPayload, Message: msg, Reason: reason}
}

// NewBadRequestError creates a bad request error.
func NewBadRequestError(reason string) *MppError {
	var msg string
	if reason != "" {
		msg = fmt.Sprintf("Bad request: %s.", reason)
	} else {
		msg = "Bad request."
	}
	return &MppError{Code: CodeBadRequest, Message: msg, Reason: reason}
}

// NewInsufficientBalanceError creates an insufficient balance error.
func NewInsufficientBalanceError(reason string) *MppError {
	var msg string
	if reason != "" {
		msg = fmt.Sprintf("Insufficient balance: %s.", reason)
	} else {
		msg = "Insufficient balance."
	}
	return &MppError{Code: CodeInsufficientBalance, Message: msg, Reason: reason}
}

// NewInvalidSignatureError creates an invalid signature error.
func NewInvalidSignatureError(reason string) *MppError {
	var msg string
	if reason != "" {
		msg = fmt.Sprintf("Invalid signature: %s.", reason)
	} else {
		msg = "Invalid signature."
	}
	return &MppError{Code: CodeInvalidSignature, Message: msg, Reason: reason}
}

// NewSignerMismatchError creates a signer mismatch error.
func NewSignerMismatchError(reason string) *MppError {
	var msg string
	if reason != "" {
		msg = fmt.Sprintf("Signer mismatch: %s.", reason)
	} else {
		msg = "Signer is not authorized for this channel."
	}
	return &MppError{Code: CodeSignerMismatch, Message: msg, Reason: reason}
}

// NewAmountExceedsDepositError creates an amount-exceeds-deposit error.
func NewAmountExceedsDepositError(reason string) *MppError {
	var msg string
	if reason != "" {
		msg = fmt.Sprintf("Amount exceeds deposit: %s.", reason)
	} else {
		msg = "Voucher amount exceeds channel deposit."
	}
	return &MppError{Code: CodeAmountExceedsDeposit, Message: msg, Reason: reason}
}

// NewDeltaTooSmallError creates a delta-too-small error.
func NewDeltaTooSmallError(reason string) *MppError {
	var msg string
	if reason != "" {
		msg = fmt.Sprintf("Delta too small: %s.", reason)
	} else {
		msg = "Amount increase below minimum voucher delta."
	}
	return &MppError{Code: CodeDeltaTooSmall, Message: msg, Reason: reason}
}

// NewChannelNotFoundError creates a channel-not-found error.
func NewChannelNotFoundError(reason string) *MppError {
	var msg string
	if reason != "" {
		msg = fmt.Sprintf("Channel not found: %s.", reason)
	} else {
		msg = "No channel with this ID exists."
	}
	return &MppError{Code: CodeChannelNotFound, Message: msg, Reason: reason}
}

// NewChannelClosedError creates a channel-closed/finalized error.
func NewChannelClosedError(reason string) *MppError {
	var msg string
	if reason != "" {
		msg = fmt.Sprintf("Channel closed: %s.", reason)
	} else {
		msg = "Channel is closed."
	}
	return &MppError{Code: CodeChannelClosed, Message: msg, Reason: reason}
}

// ==================== PaymentErrorDetails ====================

// PaymentErrorDetails is the RFC 9457 Problem Details structure for payment errors.
type PaymentErrorDetails struct {
	ProblemType string `json:"type"`
	Title       string `json:"title"`
	Status      int    `json:"status"`
	Detail      string `json:"detail"`
	ChallengeID string `json:"challengeId,omitempty"`
}

// NewPaymentErrorDetails creates a new PaymentErrorDetails with the given problem type URI.
func NewPaymentErrorDetails(typeURI string) *PaymentErrorDetails {
	return &PaymentErrorDetails{
		ProblemType: typeURI,
		Status:      402,
	}
}

// CorePaymentError creates a PaymentErrorDetails for a core payment-auth problem.
// The full type URI is CoreProblemTypeBase + suffix.
func CorePaymentError(suffix string) *PaymentErrorDetails {
	return NewPaymentErrorDetails(CoreProblemTypeBase + suffix)
}

// SessionPaymentError creates a PaymentErrorDetails for a session/channel problem.
// The full type URI is SessionProblemTypeBase + suffix.
func SessionPaymentError(suffix string) *PaymentErrorDetails {
	return NewPaymentErrorDetails(SessionProblemTypeBase + suffix)
}

// WithTitle sets the title and returns the receiver.
func (p *PaymentErrorDetails) WithTitle(title string) *PaymentErrorDetails {
	p.Title = title
	return p
}

// WithStatus sets the HTTP status code and returns the receiver.
func (p *PaymentErrorDetails) WithStatus(status int) *PaymentErrorDetails {
	p.Status = status
	return p
}

// WithDetail sets the detail message and returns the receiver.
func (p *PaymentErrorDetails) WithDetail(detail string) *PaymentErrorDetails {
	p.Detail = detail
	return p
}

// WithChallengeID sets the challenge ID and returns the receiver.
func (p *PaymentErrorDetails) WithChallengeID(challengeID string) *PaymentErrorDetails {
	p.ChallengeID = challengeID
	return p
}

// ==================== PaymentError Interface ====================

// PaymentError is implemented by errors that can be converted to RFC 9457 Problem Details.
type PaymentError interface {
	ToProblemDetails(challengeID string) *PaymentErrorDetails
}
