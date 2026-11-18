package server

import (
	"context"

	"github.com/okx/payments/go/mpp/protocol"
)

// VerifyCredentialForRoute checks only the current route's amount, then keeps
// the original credential, HMAC/expiry checks and payment pipeline unchanged.
func (m *Mpp) VerifyCredentialForRoute(ctx context.Context, cfg ChargeRouteConfig, auth string) (*protocol.Receipt, error) {
	amount, err := ParseDollarAmount(cfg.Amount, cfg.Decimals)
	if err != nil {
		return nil, err
	}
	original, err := checkRouteAmount(auth, amount)
	if err != nil {
		return nil, err
	}
	return m.VerifyCredential(ctx, original, auth)
}

// VerifySessionForRoute binds the echoed per-unit price, not the cumulative
// voucher amount. It does not change session accounting or settlement.
func (m *Mpp) VerifySessionForRoute(ctx context.Context, cfg SessionRouteConfig, auth string) (*protocol.SessionVerifyResult, error) {
	amount, err := ParseDollarAmount(cfg.Amount, cfg.Decimals)
	if err != nil {
		return nil, err
	}
	original, err := checkRouteAmount(auth, amount)
	if err != nil {
		return nil, err
	}
	return m.VerifySession(ctx, original, auth)
}

func checkRouteAmount(auth, expected string) (string, error) {
	credential, err := protocol.ParseAuthorization(auth)
	if err != nil {
		return "", err
	}
	if credential.Echo == nil {
		return "", protocol.VerificationErrorInvalidCredential("missing challenge")
	}
	var request struct {
		Amount string `json:"amount"`
	}
	if err := protocol.DeserializeRequestTyped(credential.Echo.Request, &request); err != nil {
		return "", err
	}
	if err := protocol.CheckAmount(request.Amount, expected); err != nil {
		return "", err
	}
	return ChallengeHeaderFromAuth(auth)
}
