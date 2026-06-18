package server

import (
	"fmt"

	"github.com/okx/payments/go/mpp/evm"
	"github.com/okx/payments/go/mpp/protocol"
)

// ReceiptContextKey is the context key under which the verified payment
// receipt is stored after successful credential or session verification.
// All framework adapters use this same key.
const ReceiptContextKey = "mpp_payment_receipt"

// ChallengeHeaderFromAuth reconstructs a WWW-Authenticate header value from
// the ChallengeEcho embedded in an Authorization header.
func ChallengeHeaderFromAuth(authHeader string) (string, error) {
	cred, err := protocol.ParseAuthorization(authHeader)
	if err != nil {
		return "", fmt.Errorf("ChallengeHeaderFromAuth: %w", err)
	}
	if cred.Echo == nil {
		return "", fmt.Errorf("ChallengeHeaderFromAuth: echo is nil")
	}

	echo := cred.Echo
	c := &protocol.PaymentChallenge{
		ID:          echo.ID,
		Realm:       echo.Realm,
		Method:      echo.Method,
		Intent:      echo.Intent,
		Request:     echo.Request,
		Expires:     echo.ParseExpiresUnix(),
		Description: echo.Description,
		Digest:      echo.Digest,
		Opaque:      echo.Opaque,
	}

	header, err := c.ToHeader()
	if err != nil {
		return "", fmt.Errorf("ChallengeHeaderFromAuth: format challenge: %w", err)
	}
	return header, nil
}

// EVMConfig holds the static, instance-level configuration shared by all
// routes on a given Mpp instance. Set once at construction.
type EVMConfig struct {
	// ChainID is the expected EVM chain identifier (e.g. 196 for X Layer).
	ChainID uint64

	// Recipient is the expected on-chain payment recipient address (hex, with or
	// without 0x prefix).
	Recipient string

	// SecretKey is the HMAC secret used to sign and verify challenge IDs.
	// If empty an empty key is used (deterministic but not authenticated).
	SecretKey string

	// Realm is the protection realm in the WWW-Authenticate header.
	// Defaults to "mpp" if empty.
	Realm string
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-route config structs (replace EVMOption builder pattern)
// ─────────────────────────────────────────────────────────────────────────────

// ChargeRouteConfig holds per-route parameters for a charge challenge.
type ChargeRouteConfig struct {
	// Amount in human-readable decimal (e.g. "0.01").
	Amount string

	// Currency is the ERC-20 token contract address.
	Currency string

	// Decimals is the token's decimal places (e.g. 6 for USDC).
	Decimals uint32

	// Description is an optional human-readable payment description.
	Description string

	// ExternalID is an optional caller-supplied identifier.
	ExternalID string

	// Splits specifies secondary recipients. The primary recipient receives
	// totalAmount minus the sum of all splits. Max 10 entries.
	Splits []evm.Split

	// ResourceURL optionally tags this charge with the endpoint URL it protects
	// (e.g. "https://api.shop.com/photo"), forwarded through challenge.request so
	// the SA API can aggregate transaction volume / revenue per endpoint. Charge
	// mode only. Leave empty to omit.
	ResourceURL string
}

// SessionRouteConfig holds per-route parameters for a session challenge.
type SessionRouteConfig struct {
	// Amount in human-readable decimal (e.g. "0.001").
	Amount string

	// Currency is the ERC-20 token contract address.
	Currency string

	// Decimals is the token's decimal places.
	Decimals uint32

	// Description is an optional human-readable payment description.
	Description string

	// ExternalID is an optional caller-supplied identifier.
	ExternalID string

	// UnitType describes the billing unit (e.g. "request", "second", "byte").
	UnitType string

	// SuggestedDeposit is a hint for the client's initial deposit (base units).
	SuggestedDeposit string
}

// ─────────────────────────────────────────────────────────────────────────────
// High-level option structs for lower-level challenge creation
// ─────────────────────────────────────────────────────────────────────────────

// ChargeOptions bundles the configuration and optional parameters for creating
// a charge payment challenge.
type ChargeOptions struct {
	// EVMConfig holds the verifier's chain configuration.
	EVMConfig EVMConfig

	// Description is an optional human-readable payment description embedded in
	// the WWW-Authenticate challenge.
	Description string

	// Realm overrides the default realm value in the WWW-Authenticate header.
	Realm string
}

// SessionChallengeOptions bundles the configuration and optional parameters for
// creating a session payment challenge.
type SessionChallengeOptions struct {
	// EVMConfig holds the verifier's chain configuration.
	EVMConfig EVMConfig

	// Description is an optional human-readable payment description embedded in
	// the WWW-Authenticate challenge.
	Description string

	// Realm overrides the default realm value in the WWW-Authenticate header.
	Realm string
}
