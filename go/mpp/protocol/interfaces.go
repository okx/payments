package protocol

import (
	"context"
	"encoding/json"
	"net/http"
	"fmt"
)

// ─────────────────────────────────────────────────────────────────────────────
// ErrorCode
// ─────────────────────────────────────────────────────────────────────────────

// ErrorCode is a machine-readable verification error identifier.
type ErrorCode string

const (
	ErrorCodeExpired              ErrorCode = "expired"
	ErrorCodeInvalidAmount        ErrorCode = "invalid-amount"
	ErrorCodeInvalidRecipient     ErrorCode = "invalid-recipient"
	ErrorCodeTransactionFailed    ErrorCode = "transaction-failed"
	ErrorCodeNotFound             ErrorCode = "not-found"
	ErrorCodeInvalidCredential    ErrorCode = "invalid-credential"
	ErrorCodeNetworkError         ErrorCode = "network-error"
	ErrorCodeChainIdMismatch      ErrorCode = "chain-id-mismatch"
	ErrorCodeCredentialMismatch   ErrorCode = "credential-mismatch"
	ErrorCodeChannelNotFound      ErrorCode = "channel-not-found"
	ErrorCodeChannelClosed        ErrorCode = "channel-closed"
	ErrorCodeInsufficientBalance  ErrorCode = "insufficient-balance"
	ErrorCodeInvalidPayload       ErrorCode = "invalid-payload"
	ErrorCodeInvalidSignature     ErrorCode = "invalid-signature"
	ErrorCodeAmountExceedsDeposit ErrorCode = "amount-exceeds-deposit"
	ErrorCodeDeltaTooSmall        ErrorCode = "delta-too-small"
)

// specNamespace is the URI prefix for error codes defined in the MPP spec.
const specNamespace = "https://mpp.okx.com/errors/"

// String returns the raw error code string.
func (c ErrorCode) String() string {
	return string(c)
}

// SpecCode returns the error code prefixed with the MPP spec namespace URI.
func (c ErrorCode) SpecCode() string {
	return fmt.Sprintf("%s%s", specNamespace, string(c))
}

// ─────────────────────────────────────────────────────────────────────────────
// VerificationError
// ─────────────────────────────────────────────────────────────────────────────

// VerificationError is returned by verifier implementations to signal a
// structured payment verification failure.
type VerificationError struct {
	Message   string    `json:"message"`
	Code      ErrorCode `json:"code,omitempty"`
	Retryable bool      `json:"retryable"`
}

// Error implements the error interface.
func (e *VerificationError) Error() string {
	if e.Code != "" {
		return fmt.Sprintf("verification error [%s]: %s", e.Code, e.Message)
	}
	return fmt.Sprintf("verification error: %s", e.Message)
}

// NewVerificationError creates a VerificationError with only a message.
func NewVerificationError(message string) *VerificationError {
	return &VerificationError{Message: message}
}

// NewVerificationErrorWithCode creates a VerificationError with a message and
// an error code.
func NewVerificationErrorWithCode(message string, code ErrorCode) *VerificationError {
	return &VerificationError{Message: message, Code: code}
}

// HTTPStatus returns the appropriate HTTP status code per MPP error spec.
//   - 400 Bad Request for payload/format errors
//   - 410 Gone for channel-not-found/channel-closed
//   - 402 Payment Required for all other payment errors (default)
func (e *VerificationError) HTTPStatus() int {
	switch e.Code {
	case ErrorCodeInvalidPayload:
		return http.StatusBadRequest
	case ErrorCodeChannelNotFound, ErrorCodeChannelClosed:
		return http.StatusGone
	default:
		return http.StatusPaymentRequired
	}
}

// WithRetryable marks the error as retryable and returns the receiver.
func (e *VerificationError) WithRetryable() *VerificationError {
	e.Retryable = true
	return e
}

// ─────────────────────────────────────────────────────────────────────────────
// Named constructors for each error code
// ─────────────────────────────────────────────────────────────────────────────

// VerificationErrorExpired creates a VerificationError with code ErrorCodeExpired.
func VerificationErrorExpired(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeExpired)
}

// VerificationErrorInvalidAmount creates a VerificationError with code ErrorCodeInvalidAmount.
func VerificationErrorInvalidAmount(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeInvalidAmount)
}

// VerificationErrorInvalidRecipient creates a VerificationError with code ErrorCodeInvalidRecipient.
func VerificationErrorInvalidRecipient(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeInvalidRecipient)
}

// VerificationErrorTransactionFailed creates a VerificationError with code ErrorCodeTransactionFailed.
func VerificationErrorTransactionFailed(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeTransactionFailed)
}

// VerificationErrorNotFound creates a VerificationError with code ErrorCodeNotFound.
func VerificationErrorNotFound(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeNotFound)
}

// VerificationErrorInvalidCredential creates a VerificationError with code ErrorCodeInvalidCredential.
func VerificationErrorInvalidCredential(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeInvalidCredential)
}

// VerificationErrorNetworkError creates a retryable VerificationError with code ErrorCodeNetworkError.
func VerificationErrorNetworkError(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeNetworkError).WithRetryable()
}

// VerificationErrorChainIdMismatch creates a VerificationError with code ErrorCodeChainIdMismatch.
func VerificationErrorChainIdMismatch(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeChainIdMismatch)
}

// VerificationErrorCredentialMismatch creates a VerificationError with code ErrorCodeCredentialMismatch.
func VerificationErrorCredentialMismatch(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeCredentialMismatch)
}

// VerificationErrorChannelNotFound creates a VerificationError with code ErrorCodeChannelNotFound.
func VerificationErrorChannelNotFound(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeChannelNotFound)
}

// VerificationErrorChannelClosed creates a VerificationError with code ErrorCodeChannelClosed.
func VerificationErrorChannelClosed(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeChannelClosed)
}

// VerificationErrorInsufficientBalance creates a VerificationError with code ErrorCodeInsufficientBalance.
func VerificationErrorInsufficientBalance(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeInsufficientBalance)
}

// VerificationErrorInvalidPayload creates a VerificationError with code ErrorCodeInvalidPayload.
func VerificationErrorInvalidPayload(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeInvalidPayload)
}

// VerificationErrorInvalidSignature creates a VerificationError with code ErrorCodeInvalidSignature.
func VerificationErrorInvalidSignature(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeInvalidSignature)
}

// VerificationErrorAmountExceedsDeposit creates a VerificationError with code ErrorCodeAmountExceedsDeposit.
func VerificationErrorAmountExceedsDeposit(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeAmountExceedsDeposit)
}

// VerificationErrorDeltaTooSmall creates a VerificationError with code ErrorCodeDeltaTooSmall.
func VerificationErrorDeltaTooSmall(msg string) *VerificationError {
	return NewVerificationErrorWithCode(msg, ErrorCodeDeltaTooSmall)
}

// VerificationErrorPending creates a retryable VerificationError without a
// specific error code, indicating the operation is still in progress.
func VerificationErrorPending(msg string) *VerificationError {
	return NewVerificationError(msg).WithRetryable()
}

// ─────────────────────────────────────────────────────────────────────────────
// Short error constructors with built-in messages
// ─────────────────────────────────────────────────────────────────────────────

func ErrCredential(msg string) *VerificationError {
	return VerificationErrorInvalidCredential(msg)
}

func ErrPayload(msg string) *VerificationError {
	return VerificationErrorInvalidPayload(msg)
}

func ErrSig(msg string) *VerificationError {
	return VerificationErrorInvalidSignature(msg)
}

func ErrAmount(msg string) *VerificationError {
	return VerificationErrorInvalidAmount(msg)
}

func ErrNetwork(msg string) *VerificationError {
	return VerificationErrorNetworkError(msg)
}

func ErrChannelNotFound(channelID string) *VerificationError {
	return VerificationErrorChannelNotFound(fmt.Sprintf("channel %q not found", channelID))
}

func ErrChannelClosed() *VerificationError {
	return VerificationErrorChannelClosed("channel is already closed")
}

func ErrExceedsDeposit(amount, deposit string) *VerificationError {
	return VerificationErrorAmountExceedsDeposit(
		fmt.Sprintf("amount %s exceeds deposit %s", amount, deposit),
	)
}

func ErrDeltaTooSmall(delta, min string) *VerificationError {
	return VerificationErrorDeltaTooSmall(
		fmt.Sprintf("delta %s below minimum %s", delta, min),
	)
}

func ErrInsufficientBalance(available, requested string) *VerificationError {
	return VerificationErrorInsufficientBalance(
		fmt.Sprintf("available %s, requested %s", available, requested),
	)
}

func ErrChainMismatch(expected, got uint64) *VerificationError {
	return VerificationErrorChainIdMismatch(
		fmt.Sprintf("expected chain %d, got %d", expected, got),
	)
}

// ─────────────────────────────────────────────────────────────────────────────
// Verifier interfaces
// ─────────────────────────────────────────────────────────────────────────────

// ChargeVerifier verifies a one-shot charge payment against a credential.
type ChargeVerifier interface {
	// Method returns the lowercase payment method name handled by this verifier.
	Method() string

	// PrepareRequest applies any method-specific transformations to the request
	// before verification (e.g. unit conversion, default injection).
	PrepareRequest(request ChargeRequest, cred *PaymentCredential) ChargeRequest

	// Verify validates the credential and request, then returns the receipt.
	// On failure it returns a *VerificationError (which also satisfies error).
	Verify(ctx context.Context, cred *PaymentCredential, request *ChargeRequest) (*Receipt, error)
}

// SessionVerifier verifies a session-based payment against a credential.
type SessionVerifier interface {
	// Method returns the lowercase payment method name handled by this verifier.
	Method() string

	// VerifySession validates the credential and session request, then returns
	// the receipt. On failure it returns a *VerificationError.
	VerifySession(ctx context.Context, cred *PaymentCredential, request *SessionRequest) (*Receipt, error)

	// ChallengeMethodDetails returns the method-specific details to embed in the
	// WWW-Authenticate challenge, or nil if none are required.
	ChallengeMethodDetails() json.RawMessage

	// Respond is called after successful verification. If it returns a non-nil
	// value, the request is a management action (open, topUp, close) and the
	// caller should return the value as the response body instead of proceeding
	// with normal request handling. If nil, the caller serves the resource.
	Respond(cred *PaymentCredential, receipt *Receipt) any
}
