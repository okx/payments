package adapters

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"

	"github.com/okx/payments/go/mpp/protocol"
	"github.com/okx/payments/go/mpp/server"
	"github.com/okx/payments/go/paymentrouter"
)

// Ensure MppAdapter implements the ProtocolAdapter interface.
var _ paymentrouter.ProtocolAdapter = (*MppAdapter)(nil)

// MppRouteConfig holds per-route configuration for the MppAdapter.
type MppRouteConfig struct {
	Intent           string `json:"intent"`
	Amount           string `json:"amount"`
	Currency         string `json:"currency"`
	Decimals         uint32 `json:"decimals"`
	Description      string `json:"description,omitempty"`
	ExternalID       string `json:"externalId,omitempty"`
	Realm            string `json:"realm,omitempty"`
	UnitType         string `json:"unitType,omitempty"`
	SuggestedDeposit string `json:"suggestedDeposit,omitempty"`
}

// MppAdapter implements paymentrouter.ProtocolAdapter for the MPP payment protocol.
type MppAdapter struct {
	mpp      *server.Mpp
	priority int
}

// NewMppAdapter creates a new MppAdapter wrapping the provided Mpp server instance.
func NewMppAdapter(mpp *server.Mpp) *MppAdapter {
	return &MppAdapter{mpp: mpp, priority: 10}
}

// Name returns the protocol name identifier.
func (a *MppAdapter) Name() string {
	return "mpp"
}

// Priority returns the adapter's priority for protocol selection ordering.
func (a *MppAdapter) Priority() int {
	return a.priority
}

// Detect reports whether the request carries a PaymentAuth Authorization header.
func (a *MppAdapter) Detect(r *http.Request) bool {
	authHeader := r.Header.Get(protocol.AuthorizationHeader)
	if authHeader == "" {
		return false
	}
	idx := strings.IndexByte(authHeader, ' ')
	var scheme string
	if idx < 0 {
		scheme = authHeader
	} else {
		scheme = authHeader[:idx]
	}
	return scheme == protocol.PaymentScheme
}

// GetChallenge issues a WWW-Authenticate challenge header based on the route config.
// Returns nil headers (without error) when cfg is nil or has no intent set.
func (a *MppAdapter) GetChallenge(ctx context.Context, r *http.Request, cfg any) (http.Header, error) {
	if cfg == nil {
		return nil, nil
	}

	routeCfg, ok := cfg.(MppRouteConfig)
	if !ok {
		return nil, fmt.Errorf("mpp adapter: invalid config type %T", cfg)
	}

	if routeCfg.Intent == "" {
		return nil, nil
	}

	var challengeHeader string
	var err error

	switch routeCfg.Intent {
	case protocol.IntentCharge:
		challengeHeader, err = a.mpp.Charge(ctx, server.ChargeRouteConfig{
			Amount:      routeCfg.Amount,
			Currency:    routeCfg.Currency,
			Decimals:    routeCfg.Decimals,
			Description: routeCfg.Description,
			ExternalID:  routeCfg.ExternalID,
		})
	case protocol.IntentSession:
		challengeHeader, err = a.mpp.SessionChallenge(ctx, server.SessionRouteConfig{
			Amount:           routeCfg.Amount,
			Currency:         routeCfg.Currency,
			Decimals:         routeCfg.Decimals,
			Description:      routeCfg.Description,
			ExternalID:       routeCfg.ExternalID,
			UnitType:         routeCfg.UnitType,
			SuggestedDeposit: routeCfg.SuggestedDeposit,
		})
	default:
		return nil, fmt.Errorf("mpp: unknown intent %q", routeCfg.Intent)
	}

	if err != nil {
		return nil, fmt.Errorf("mpp: generate challenge: %w", err)
	}

	h := make(http.Header)
	h.Set(protocol.WWWAuthenticateHeader, challengeHeader)
	return h, nil
}

// Handle verifies the payment credential in the Authorization header and writes
// the Payment-Receipt header on success. Returns an error on failure after
// writing an error JSON body to w.
func (a *MppAdapter) Handle(w http.ResponseWriter, r *http.Request, cfg any) error {
	authHeader := r.Header.Get(protocol.AuthorizationHeader)
	if authHeader == "" {
		writeErrorJSON(w, http.StatusUnauthorized, "missing Authorization header")
		return fmt.Errorf("mpp: missing Authorization header")
	}

	challengeHeader, err := server.ChallengeHeaderFromAuth(authHeader)
	if err != nil {
		writeErrorJSON(w, http.StatusBadRequest, "invalid Authorization header")
		return fmt.Errorf("mpp: reconstruct challenge: %w", err)
	}

	// Determine intent from the reconstructed challenge.
	challenge, err := protocol.PaymentChallengeFromHeader(challengeHeader)
	if err != nil {
		writeErrorJSON(w, http.StatusBadRequest, "invalid challenge header")
		return fmt.Errorf("mpp: parse challenge: %w", err)
	}

	switch challenge.Intent {
	case protocol.IntentCharge:
		receipt, err := a.mpp.VerifyCredential(r.Context(), challengeHeader, authHeader)
		if err != nil {
			writeErrorJSON(w, http.StatusUnauthorized, "payment verification failed")
			return fmt.Errorf("mpp: verify charge: %w", err)
		}
		if receiptHeader, hErr := receipt.ToHeader(); hErr == nil {
			w.Header().Set(protocol.PaymentReceiptHeader, receiptHeader)
		}

	case protocol.IntentSession:
		result, err := a.mpp.VerifySession(r.Context(), challengeHeader, authHeader)
		if err != nil {
			writeErrorJSON(w, http.StatusUnauthorized, "payment verification failed")
			return fmt.Errorf("mpp: verify session: %w", err)
		}
		if receiptHeader, hErr := result.Receipt.ToHeader(); hErr == nil {
			w.Header().Set(protocol.PaymentReceiptHeader, receiptHeader)
		}
		if result.ManagementResponse != nil {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			json.NewEncoder(w).Encode(result.ManagementResponse) //nolint:errcheck
			return nil
		}

	default:
		writeErrorJSON(w, http.StatusBadRequest, fmt.Sprintf("unknown intent %q", challenge.Intent))
		return fmt.Errorf("mpp: unknown intent %q", challenge.Intent)
	}

	return nil
}

// writeErrorJSON writes a JSON error body with the given status code.
func writeErrorJSON(w http.ResponseWriter, status int, msg string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(map[string]string{"error": msg}) //nolint:errcheck
}
