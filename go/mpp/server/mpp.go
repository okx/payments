package server

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/okx/payments/go/mpp/evm"
	"github.com/okx/payments/go/mpp/protocol"
)

// Mpp is the high-level payment coordinator that wires together an EVM
// configuration with a charge verifier and a session verifier.
type Mpp struct {
	cfg             EVMConfig
	chargeVerifier  protocol.ChargeVerifier
	sessionVerifier protocol.SessionVerifier
}

// NewMpp creates a new Mpp instance. Both verifiers may be nil if only one
// intent type is required.
func NewMpp(cfg EVMConfig, charge protocol.ChargeVerifier, session protocol.SessionVerifier) *Mpp {
	return &Mpp{
		cfg:             cfg,
		chargeVerifier:  charge,
		sessionVerifier: session,
	}
}

// defaultRealm returns the default challenge realm ("mpp").
const defaultRealm = "mpp"

// ─────────────────────────────────────────────────────────────────────────────
// Charge
// ─────────────────────────────────────────────────────────────────────────────

// Charge creates a WWW-Authenticate challenge header for a one-shot charge
// payment. The amount is expressed in decimal notation; decimals specifies the
// token's decimal places.
func (m *Mpp) Charge(ctx context.Context, cfg ChargeRouteConfig) (string, error) {
	if m.chargeVerifier == nil {
		return "", fmt.Errorf("Mpp.Charge: charge verifier not configured")
	}

	baseAmount, err := ParseDollarAmount(cfg.Amount, cfg.Decimals)
	if err != nil {
		return "", fmt.Errorf("Mpp.Charge: %w", err)
	}

	realm := m.cfg.Realm
	if realm == "" {
		realm = defaultRealm
	}

	req := protocol.ChargeRequest{
		Amount:   baseAmount,
		Currency: cfg.Currency,
	}
	if cfg.Description != "" {
		req.Description = &cfg.Description
	}
	if cfg.ExternalID != "" {
		req.ExternalID = &cfg.ExternalID
	}
	if m.cfg.Recipient != "" {
		req.Recipient = &m.cfg.Recipient
	}
	{
		var details evm.EVMMethodDetails
		// Pull feePayer from charge method if it exposes ChallengeMethodDetails.
		type detailer interface{ ChallengeMethodDetails() *evm.EVMMethodDetails }
		if d, ok := m.chargeVerifier.(detailer); ok {
			if md := d.ChallengeMethodDetails(); md != nil {
				details = *md
			}
		}
		if cfg.Splits != nil {
			details.Splits = cfg.Splits
		}
		if m.cfg.ChainID != 0 {
			chainID := m.cfg.ChainID
			details.ChainID = &chainID
		}
		data, err := json.Marshal(details)
		if err != nil {
			return "", fmt.Errorf("Mpp.Charge: marshal method_details: %w", err)
		}
		req.MethodDetails = data
	}

	copts := ChargeOptions{
		EVMConfig:   m.cfg,
		Description: cfg.Description,
		Realm:       realm,
	}

	return m.ChargeWithOptions(ctx, req, copts)
}

// ChargeWithOptions is the lower-level charge challenge creation method. It
// serializes req into the challenge's request field and returns a formatted
// WWW-Authenticate header.
func (m *Mpp) ChargeWithOptions(ctx context.Context, req protocol.ChargeRequest, opts ChargeOptions) (string, error) {
	if m.chargeVerifier == nil {
		return "", fmt.Errorf("Mpp.ChargeWithOptions: charge verifier not configured")
	}

	requestB64, err := protocol.SerializeRequest(req)
	if err != nil {
		return "", fmt.Errorf("Mpp.ChargeWithOptions: serialize request: %w", err)
	}

	realm := opts.Realm
	if realm == "" {
		realm = defaultRealm
	}

	secretKey := opts.EVMConfig.SecretKey
	if secretKey == "" {
		secretKey = m.cfg.SecretKey
	}

	expires := uint64(time.Now().Add(evm.DefaultExpiresMinutes * time.Minute).Unix())

	c := protocol.NewPaymentChallenge(realm, m.chargeVerifier.Method(), protocol.IntentCharge, requestB64)
	c.WithExpires(expires)
	c.WithSecretKey(secretKey)
	if opts.Description != "" {
		c.WithDescription(opts.Description)
	}

	header, err := c.ToHeader()
	if err != nil {
		return "", fmt.Errorf("Mpp.ChargeWithOptions: format header: %w", err)
	}
	return header, nil
}

// ─────────────────────────────────────────────────────────────────────────────
// VerifyCredential
// ─────────────────────────────────────────────────────────────────────────────

// VerifyCredential verifies a charge credential presented in authHeader against
// the challenge in challengeHeader. On success it returns the payment receipt.
func (m *Mpp) VerifyCredential(ctx context.Context, challengeHeader string, authHeader string) (*protocol.Receipt, error) {
	if m.chargeVerifier == nil {
		return nil, fmt.Errorf("Mpp.VerifyCredential: charge verifier not configured")
	}

	challenge, err := protocol.PaymentChallengeFromHeader(challengeHeader)
	if err != nil {
		return nil, fmt.Errorf("Mpp.VerifyCredential: parse challenge: %w", err)
	}

	// Recompute the expected challenge ID using our secret key so clients
	// cannot forge a challenge ID.
	expectedID := protocol.ComputeChallengeID(
		m.cfg.SecretKey,
		challenge.Realm,
		challenge.Method,
		challenge.Intent,
		challenge.Request,
		challenge.Expires,
		challenge.Digest,
		challenge.Opaque,
	)
	challenge.ID = expectedID
	challenge.SecretKey = m.cfg.SecretKey

	if err := challenge.ValidateForCharge(); err != nil {
		return nil, fmt.Errorf("Mpp.VerifyCredential: validate: %w", err)
	}

	cred, err := protocol.ParseAuthorization(authHeader)
	if err != nil {
		return nil, fmt.Errorf("Mpp.VerifyCredential: parse authorization: %w", err)
	}

	if err := challenge.Verify(cred); err != nil {
		return nil, fmt.Errorf("Mpp.VerifyCredential: verify: %w", err)
	}

	var req protocol.ChargeRequest
	if err := protocol.RequestFromChallengeTyped(challenge, &req); err != nil {
		return nil, fmt.Errorf("Mpp.VerifyCredential: decode request: %w", err)
	}

	req = m.chargeVerifier.PrepareRequest(req, cred)

	return m.chargeVerifier.Verify(ctx, cred, &req)
}

// ─────────────────────────────────────────────────────────────────────────────
// SessionChallenge
// ─────────────────────────────────────────────────────────────────────────────

// SessionChallenge creates a WWW-Authenticate challenge header for a
// session-based payment. The amount is expressed in decimal notation; decimals
// specifies the token's decimal places.
func (m *Mpp) SessionChallenge(ctx context.Context, cfg SessionRouteConfig) (string, error) {
	if m.sessionVerifier == nil {
		return "", fmt.Errorf("Mpp.SessionChallenge: session verifier not configured")
	}

	baseAmount, err := ParseDollarAmount(cfg.Amount, cfg.Decimals)
	if err != nil {
		return "", fmt.Errorf("Mpp.SessionChallenge: %w", err)
	}

	realm := m.cfg.Realm
	if realm == "" {
		realm = defaultRealm
	}

	req := protocol.SessionRequest{
		Amount:   baseAmount,
		Currency: cfg.Currency,
	}
	if m.cfg.Recipient != "" {
		req.Recipient = &m.cfg.Recipient
	}
	if cfg.Description != "" {
		req.Description = &cfg.Description
	}
	if cfg.ExternalID != "" {
		req.ExternalID = &cfg.ExternalID
	}
	if cfg.UnitType != "" {
		req.UnitType = &cfg.UnitType
	}
	if cfg.SuggestedDeposit != "" {
		req.SuggestedDeposit = &cfg.SuggestedDeposit
	}

	sopts := SessionChallengeOptions{
		EVMConfig:   m.cfg,
		Description: cfg.Description,
		Realm:       realm,
	}

	return m.SessionChallengeWithDetails(ctx, req, sopts)
}

// SessionChallengeWithDetails is the lower-level session challenge creation
// method. It serializes req into the challenge's request field and returns a
// formatted WWW-Authenticate header. Method-specific details from the session
// verifier are embedded in the challenge.
func (m *Mpp) SessionChallengeWithDetails(ctx context.Context, req protocol.SessionRequest, opts SessionChallengeOptions) (string, error) {
	if m.sessionVerifier == nil {
		return "", fmt.Errorf("Mpp.SessionChallengeWithDetails: session verifier not configured")
	}

	// Embed method-specific details from the verifier if available.
	if details := m.sessionVerifier.ChallengeMethodDetails(); details != nil {
		req.MethodDetails = details
	}

	requestB64, err := protocol.SerializeRequest(req)
	if err != nil {
		return "", fmt.Errorf("Mpp.SessionChallengeWithDetails: serialize request: %w", err)
	}

	realm := opts.Realm
	if realm == "" {
		realm = defaultRealm
	}

	secretKey := opts.EVMConfig.SecretKey
	if secretKey == "" {
		secretKey = m.cfg.SecretKey
	}

	expires := uint64(time.Now().Add(evm.DefaultExpiresMinutes * time.Minute).Unix())

	c := protocol.NewPaymentChallenge(realm, m.sessionVerifier.Method(), protocol.IntentSession, requestB64)
	c.WithExpires(expires)
	c.WithSecretKey(secretKey)
	if opts.Description != "" {
		c.WithDescription(opts.Description)
	}

	header, err := c.ToHeader()
	if err != nil {
		return "", fmt.Errorf("Mpp.SessionChallengeWithDetails: format header: %w", err)
	}
	return header, nil
}

// ─────────────────────────────────────────────────────────────────────────────
// VerifySession
// ─────────────────────────────────────────────────────────────────────────────

// VerifySession verifies a session credential presented in authHeader against
// the challenge in challengeHeader. On success it returns the payment receipt.
func (m *Mpp) VerifySession(ctx context.Context, challengeHeader string, authHeader string) (*protocol.SessionVerifyResult, error) {
	if m.sessionVerifier == nil {
		return nil, fmt.Errorf("Mpp.VerifySession: session verifier not configured")
	}

	challenge, err := protocol.PaymentChallengeFromHeader(challengeHeader)
	if err != nil {
		return nil, fmt.Errorf("Mpp.VerifySession: parse challenge: %w", err)
	}

	// Recompute the expected challenge ID using our secret key.
	expectedID := protocol.ComputeChallengeID(
		m.cfg.SecretKey,
		challenge.Realm,
		challenge.Method,
		challenge.Intent,
		challenge.Request,
		challenge.Expires,
		challenge.Digest,
		challenge.Opaque,
	)
	challenge.ID = expectedID
	challenge.SecretKey = m.cfg.SecretKey

	if err := challenge.ValidateForSession(); err != nil {
		return nil, fmt.Errorf("Mpp.VerifySession: validate: %w", err)
	}

	cred, err := protocol.ParseAuthorization(authHeader)
	if err != nil {
		return nil, fmt.Errorf("Mpp.VerifySession: parse authorization: %w", err)
	}

	if err := challenge.Verify(cred); err != nil {
		return nil, fmt.Errorf("Mpp.VerifySession: verify: %w", err)
	}

	var req protocol.SessionRequest
	if err := protocol.RequestFromChallengeTyped(challenge, &req); err != nil {
		return nil, fmt.Errorf("Mpp.VerifySession: decode request: %w", err)
	}

	receipt, verifyErr := m.sessionVerifier.VerifySession(ctx, cred, &req)
	if verifyErr != nil {
		return nil, verifyErr
	}

	mgmt := m.sessionVerifier.Respond(cred, receipt)
	return &protocol.SessionVerifyResult{
		Receipt:            receipt,
		ManagementResponse: mgmt,
	}, nil
}
