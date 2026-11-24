package protocol

import (
	"encoding/json"
	"fmt"
	"math/big"
)

// ─────────────────────────────────────────────────────────────────────────────
// ChargeRequest
// ─────────────────────────────────────────────────────────────────────────────

// ChargeRequest represents a payment request for a charge intent.
type ChargeRequest struct {
	Amount        string          `json:"amount"`
	Currency      string          `json:"currency"`
	Decimals      *uint8          `json:"decimals,omitempty"`
	Recipient     *string         `json:"recipient,omitempty"`
	Description   *string         `json:"description,omitempty"`
	ExternalID    *string         `json:"externalId,omitempty"`
	MethodDetails json.RawMessage `json:"methodDetails,omitempty"`
}

// WithBaseUnits converts the Amount field from a decimal representation to base
// units by applying the Decimals multiplier via ParseUnits, then clears Decimals.
// If Decimals is nil the receiver is returned unchanged.
func (r *ChargeRequest) WithBaseUnits() (*ChargeRequest, error) {
	if r.Decimals == nil {
		return r, nil
	}
	converted, err := ParseUnits(r.Amount, *r.Decimals)
	if err != nil {
		return nil, fmt.Errorf("ChargeRequest.WithBaseUnits: %w", err)
	}
	r.Amount = converted
	r.Decimals = nil
	return r, nil
}

// ParseAmountBigInt parses the Amount field as a *big.Int.
func (r *ChargeRequest) ParseAmountBigInt() (*big.Int, error) {
	n := new(big.Int)
	if _, ok := n.SetString(r.Amount, 10); !ok {
		return nil, fmt.Errorf("ChargeRequest.ParseAmountBigInt: invalid amount %q", r.Amount)
	}
	return n, nil
}

// ValidateMaxAmount returns an error if the request Amount exceeds max.
// Both values are parsed as *big.Int for comparison.
func (r *ChargeRequest) ValidateMaxAmount(max string) error {
	amount, err := r.ParseAmountBigInt()
	if err != nil {
		return fmt.Errorf("ChargeRequest.ValidateMaxAmount: %w", err)
	}
	maxInt := new(big.Int)
	if _, ok := maxInt.SetString(max, 10); !ok {
		return fmt.Errorf("ChargeRequest.ValidateMaxAmount: invalid max amount %q", max)
	}
	if amount.Cmp(maxInt) > 0 {
		return fmt.Errorf("ChargeRequest.ValidateMaxAmount: amount %s exceeds max %s", r.Amount, max)
	}
	return nil
}

// ─────────────────────────────────────────────────────────────────────────────
// SessionRequest
// ─────────────────────────────────────────────────────────────────────────────

// SessionRequest represents a payment request for a session intent.
type SessionRequest struct {
	Amount           string          `json:"amount"`
	UnitType         *string         `json:"unitType,omitempty"`
	Currency         string          `json:"currency"`
	Decimals         *uint8          `json:"decimals,omitempty"`
	Recipient        *string         `json:"recipient,omitempty"`
	SuggestedDeposit *string         `json:"suggestedDeposit,omitempty"`
	Description      *string         `json:"description,omitempty"`
	ExternalID       *string         `json:"externalId,omitempty"`
	MethodDetails    json.RawMessage `json:"methodDetails,omitempty"`
}

// WithBaseUnits converts the Amount field from a decimal representation to base
// units by applying the Decimals multiplier via ParseUnits, then clears Decimals.
// If Decimals is nil the receiver is returned unchanged.
func (r *SessionRequest) WithBaseUnits() (*SessionRequest, error) {
	if r.Decimals == nil {
		return r, nil
	}
	converted, err := ParseUnits(r.Amount, *r.Decimals)
	if err != nil {
		return nil, fmt.Errorf("SessionRequest.WithBaseUnits: %w", err)
	}
	r.Amount = converted
	r.Decimals = nil
	return r, nil
}

// parseAmountBigInt parses the Amount field as a *big.Int.
func (r *SessionRequest) parseAmountBigInt() (*big.Int, error) {
	n := new(big.Int)
	if _, ok := n.SetString(r.Amount, 10); !ok {
		return nil, fmt.Errorf("SessionRequest: invalid amount %q", r.Amount)
	}
	return n, nil
}

// ValidateMaxAmount returns an error if the request Amount exceeds max.
// Both values are parsed as *big.Int for comparison.
func (r *SessionRequest) ValidateMaxAmount(max string) error {
	amount, err := r.parseAmountBigInt()
	if err != nil {
		return fmt.Errorf("SessionRequest.ValidateMaxAmount: %w", err)
	}
	maxInt := new(big.Int)
	if _, ok := maxInt.SetString(max, 10); !ok {
		return fmt.Errorf("SessionRequest.ValidateMaxAmount: invalid max amount %q", max)
	}
	if amount.Cmp(maxInt) > 0 {
		return fmt.Errorf("SessionRequest.ValidateMaxAmount: amount %s exceeds max %s", r.Amount, max)
	}
	return nil
}

// ─────────────────────────────────────────────────────────────────────────────
// Serialization helpers
// ─────────────────────────────────────────────────────────────────────────────

// SerializeRequest JSON-marshals v and returns the base64url-encoded string.
func SerializeRequest(v interface{}) (string, error) {
	data, err := json.Marshal(v)
	if err != nil {
		return "", fmt.Errorf("SerializeRequest: marshal: %w", err)
	}
	return Base64URLEncode(data), nil
}

// DeserializeRequest base64url-decodes encoded and returns the raw JSON bytes.
func DeserializeRequest(encoded string) (json.RawMessage, error) {
	data, err := Base64URLDecode(encoded)
	if err != nil {
		return nil, fmt.Errorf("DeserializeRequest: %w", err)
	}
	return json.RawMessage(data), nil
}

// DeserializeRequestTyped base64url-decodes encoded and unmarshals the JSON
// into v.
func DeserializeRequestTyped(encoded string, v interface{}) error {
	data, err := Base64URLDecode(encoded)
	if err != nil {
		return fmt.Errorf("DeserializeRequestTyped: %w", err)
	}
	if err := json.Unmarshal(data, v); err != nil {
		return fmt.Errorf("DeserializeRequestTyped: unmarshal: %w", err)
	}
	return nil
}

// RequestFromChallenge decodes the Request field of a PaymentChallenge and
// returns it as raw JSON bytes.
func RequestFromChallenge(c *PaymentChallenge) (json.RawMessage, error) {
	raw, err := DeserializeRequest(c.Request)
	if err != nil {
		return nil, fmt.Errorf("RequestFromChallenge: %w", err)
	}
	return raw, nil
}

// RequestFromChallengeTyped decodes the Request field of a PaymentChallenge
// and unmarshals it into v.
func RequestFromChallengeTyped(c *PaymentChallenge, v interface{}) error {
	if err := DeserializeRequestTyped(c.Request, v); err != nil {
		return fmt.Errorf("RequestFromChallengeTyped: %w", err)
	}
	return nil
}
