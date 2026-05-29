package saclient

import (
	"encoding/json"

	"github.com/okx/payments/go/mpp/protocol"
)

// SAResponse is the standard SA API response envelope.
type SAResponse struct {
	Code int             `json:"code"`
	Msg  string          `json:"msg"`
	Data json.RawMessage `json:"data"`
}

// ─────────────────────────────────────────────────────────────────────────────
// Generic credential envelope
// ─────────────────────────────────────────────────────────────────────────────

// CredentialRequest is the envelope for all SA API endpoints.
// Challenge and Source are optional (nil/empty for merchant-initiated flows).
// P is the typed payload specific to each endpoint.
type CredentialRequest[P any] struct {
	Challenge *protocol.ChallengeEcho `json:"challenge,omitempty"`
	Payload   P                       `json:"payload"`
	Source    string                  `json:"source,omitempty"`
}

// ─────────────────────────────────────────────────────────────────────────────
// Charge payload types
// ─────────────────────────────────────────────────────────────────────────────

// Eip3009Authorization is a signed EIP-3009 transferWithAuthorization.
// For charge, signature is inside the authorization object.
type Eip3009Authorization struct {
	Type        string                 `json:"type"` // "eip-3009"
	From        string                 `json:"from"`
	To          string                 `json:"to"`
	Value       string                 `json:"value"`
	ValidAfter  string                 `json:"validAfter"`
	ValidBefore string                 `json:"validBefore"`
	Nonce       string                 `json:"nonce"`
	Signature   string                 `json:"signature,omitempty"`
	Splits      []Eip3009Authorization `json:"splits,omitempty"`
}

// ChargeTransactionPayload is the payload for POST /charge/settle (transaction mode).
type ChargeTransactionPayload struct {
	Type          string                `json:"type"` // "transaction"
	Authorization Eip3009Authorization  `json:"authorization"`
}

// ChargeHashPayload is the payload for POST /charge/verifyHash (hash mode).
type ChargeHashPayload struct {
	Type string `json:"type"` // "hash"
	Hash string `json:"hash"`
}

// ─────────────────────────────────────────────────────────────────────────────
// Session payload types (client-facing)
// ─────────────────────────────────────────────────────────────────────────────

// SessionOpenPayload is the payload for POST /session/open.
type SessionOpenPayload struct {
	Action           string                `json:"action"`                     // "open"
	Type             string                `json:"type"`                       // "transaction" or "hash"
	ChannelID        string                `json:"channelId"`
	Authorization    *Eip3009Authorization `json:"authorization,omitempty"`    // transaction mode
	Signature        string                `json:"signature,omitempty"`        // transaction mode: EIP-3009 sig
	Hash             string                `json:"hash,omitempty"`             // hash mode: tx hash
	Salt             string                `json:"salt"`
	AuthorizedSigner string                `json:"authorizedSigner,omitempty"`
}

// SessionTopUpPayload is the payload for POST /session/topUp.
type SessionTopUpPayload struct {
	Action            string                `json:"action"`                     // "topUp"
	Type              string                `json:"type"`                       // "transaction" or "hash"
	ChannelID         string                `json:"channelId"`
	Authorization     *Eip3009Authorization `json:"authorization,omitempty"`    // transaction mode
	Signature         string                `json:"signature,omitempty"`        // transaction mode
	Hash              string                `json:"hash,omitempty"`             // hash mode
	AdditionalDeposit string                `json:"additionalDeposit"`
	TopUpSalt         string                `json:"topUpSalt,omitempty"`
}

// ─────────────────────────────────────────────────────────────────────────────
// Session payload types (merchant-facing)
// ─────────────────────────────────────────────────────────────────────────────

// SessionSettlePayload is the payload for POST /session/settle.
type SessionSettlePayload struct {
	Action           string `json:"action,omitempty"` // "settle"
	ChannelID        string `json:"channelId"`
	CumulativeAmount string `json:"cumulativeAmount"`
	VoucherSignature string `json:"voucherSignature"`
	PayeeSignature   string `json:"payeeSignature"`
	Nonce            string `json:"nonce"`
	Deadline         string `json:"deadline"`
}

// SessionClosePayload is the payload for POST /session/close.
type SessionClosePayload struct {
	Action           string `json:"action,omitempty"` // "close"
	ChannelID        string `json:"channelId"`
	CumulativeAmount string `json:"cumulativeAmount"`
	VoucherSignature string `json:"voucherSignature"`
	PayeeSignature   string `json:"payeeSignature"`
	Nonce            string `json:"nonce"`
	Deadline         string `json:"deadline"`
}

// ─────────────────────────────────────────────────────────────────────────────
// Typed request aliases
// ─────────────────────────────────────────────────────────────────────────────

type ChargeSettleRequest = CredentialRequest[ChargeTransactionPayload]
type ChargeVerifyHashRequest = CredentialRequest[ChargeHashPayload]
type SessionOpenRequest = CredentialRequest[SessionOpenPayload]
type SessionTopUpRequest = CredentialRequest[SessionTopUpPayload]
type SessionSettleRequest = CredentialRequest[SessionSettlePayload]
type SessionCloseRequest = CredentialRequest[SessionClosePayload]

// ─────────────────────────────────────────────────────────────────────────────
// Response types
// ─────────────────────────────────────────────────────────────────────────────

// ChargeReceipt is the response data for charge operations.
type ChargeReceipt struct {
	Method      string `json:"method"`
	Reference   string `json:"reference"`
	Status      string `json:"status"`
	Timestamp   string `json:"timestamp"`
	ChainID     uint64 `json:"chainId"`
	ChallengeID string `json:"challengeId"`
	ExternalID  string `json:"externalId"`
}

// SessionReceipt is the response data for session mutating operations.
type SessionReceipt struct {
	Method    string `json:"method"`
	Intent    string `json:"intent"`
	Status    string `json:"status"`
	Timestamp string `json:"timestamp"`
	ChannelID string `json:"channelId"`
	ChainID   uint64 `json:"chainId"`
	Reference string `json:"reference"`
	Deposit   string `json:"deposit"`
}

// SessionStatus is the response data for GET /api/v6/pay/mpp/session/status.
type SessionStatus struct {
	ChannelID        string `json:"channelId"`
	Payer            string `json:"payer"`
	Payee            string `json:"payee"`
	Token            string `json:"token"`
	Deposit          string `json:"deposit"`
	CumulativeAmount string `json:"cumulativeAmount"`
	SettledOnChain   string `json:"settledOnChain"`
	RemainingBalance string `json:"remainingBalance"`
	SessionStatus    string `json:"sessionStatus"` // OPEN, CLOSING, CLOSED
}

// ─────────────────────────────────────────────────────────────────────────────
// SA error codes
// ─────────────────────────────────────────────────────────────────────────────

// SAErrorCode represents numeric SA API business error codes.
type SAErrorCode int

const (
	SACodeSuccess              SAErrorCode = 0
	SACodeUnsupportedChain     SAErrorCode = 70001
	SACodePayerBlocked         SAErrorCode = 70002
	SACodeInvalidCredential    SAErrorCode = 70003
	SACodeInvalidSignature     SAErrorCode = 70004
	SACodeInsufficientBalance  SAErrorCode = 70005
	SACodeAmountExceedsDeposit SAErrorCode = 70006
	SACodeTxNotConfirmed       SAErrorCode = 70007
	SACodeChannelNotFound      SAErrorCode = 70008
	SACodeChannelClosed        SAErrorCode = 70009
	SACodeDeltaTooSmall        SAErrorCode = 70010
	SACodeGracePeriodTooShort  SAErrorCode = 70011
	SACodeSignerMismatch       SAErrorCode = 70012
	SACodeDeltaBelowMinimum    SAErrorCode = 70013
	SACodeChannelClosing       SAErrorCode = 70014
	SACodeInternalError        SAErrorCode = 8000
)
