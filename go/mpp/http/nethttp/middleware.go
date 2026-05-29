package nethttp

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"

	"github.com/okx/payments/go/mpp/protocol"
	"github.com/okx/payments/go/mpp/server"
)

func paymentErrorStatus(err error) int {
	var ve *protocol.VerificationError
	if errors.As(err, &ve) {
		return ve.HTTPStatus()
	}
	return http.StatusPaymentRequired
}

type contextKey string

const receiptCtxKey contextKey = "mpp_payment_receipt"

// ChargeMiddleware returns net/http middleware that enforces a one-shot charge payment.
func ChargeMiddleware(m *server.Mpp, cfg server.ChargeRouteConfig) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			authHeader := r.Header.Get(protocol.AuthorizationHeader)

			if authHeader == "" {
				challengeHeader, err := m.Charge(r.Context(), cfg)
				if err != nil {
					writeJSON(w, http.StatusInternalServerError, map[string]string{"error": "failed to generate payment challenge"})
					return
				}
				w.Header().Set(protocol.WWWAuthenticateHeader, challengeHeader)
				writeJSON(w, http.StatusPaymentRequired, map[string]string{"error": "Payment required"})
				return
			}

			challengeHeader, err := server.ChallengeHeaderFromAuth(authHeader)
			if err != nil {
				writeJSON(w, http.StatusBadRequest, map[string]string{"error": fmt.Sprintf("invalid Authorization header: %s", err)})
				return
			}

			receipt, err := m.VerifyCredential(r.Context(), challengeHeader, authHeader)
			if err != nil {
				writeJSON(w, paymentErrorStatus(err), map[string]string{"error": fmt.Sprintf("payment verification failed: %s", err)})
				return
			}

			if receiptHeader, hErr := receipt.ToHeader(); hErr == nil {
				w.Header().Set(protocol.PaymentReceiptHeader, receiptHeader)
			}
			ctx := context.WithValue(r.Context(), receiptCtxKey, receipt)
			next.ServeHTTP(w, r.WithContext(ctx))
		})
	}
}

// SessionMiddleware returns net/http middleware that enforces a session-based payment.
func SessionMiddleware(m *server.Mpp, cfg server.SessionRouteConfig) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			authHeader := r.Header.Get(protocol.AuthorizationHeader)

			if authHeader == "" {
				challengeHeader, err := m.SessionChallenge(r.Context(), cfg)
				if err != nil {
					writeJSON(w, http.StatusInternalServerError, map[string]string{"error": "failed to generate session challenge"})
					return
				}
				w.Header().Set(protocol.WWWAuthenticateHeader, challengeHeader)
				writeJSON(w, http.StatusPaymentRequired, map[string]string{"error": "Payment required"})
				return
			}

			challengeHeader, err := server.ChallengeHeaderFromAuth(authHeader)
			if err != nil {
				writeJSON(w, http.StatusBadRequest, map[string]string{"error": fmt.Sprintf("invalid Authorization header: %s", err)})
				return
			}

			result, err := m.VerifySession(r.Context(), challengeHeader, authHeader)
			if err != nil {
				writeJSON(w, paymentErrorStatus(err), map[string]string{"error": fmt.Sprintf("session verification failed: %s", err)})
				return
			}

			if receiptHeader, hErr := result.Receipt.ToHeader(); hErr == nil {
				w.Header().Set(protocol.PaymentReceiptHeader, receiptHeader)
			}

			if result.ManagementResponse != nil {
				writeJSON(w, http.StatusOK, result.ManagementResponse)
				return
			}
			ctx := context.WithValue(r.Context(), receiptCtxKey, result.Receipt)
			next.ServeHTTP(w, r.WithContext(ctx))
		})
	}
}

// GetReceipt retrieves the payment receipt from the request context.
func GetReceipt(r *http.Request) *protocol.Receipt {
	v := r.Context().Value(receiptCtxKey)
	if v == nil {
		return nil
	}
	receipt, _ := v.(*protocol.Receipt)
	return receipt
}

func writeJSON(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(body) //nolint:errcheck
}
