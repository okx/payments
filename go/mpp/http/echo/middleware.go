package echo

import (
	"errors"
	"fmt"
	"net/http"

	"github.com/labstack/echo/v4"
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

const receiptCtxKey = "mpp_payment_receipt"

// ChargeMiddleware returns an Echo middleware that enforces a one-shot charge payment.
func ChargeMiddleware(m *server.Mpp, cfg server.ChargeRouteConfig) echo.MiddlewareFunc {
	return func(next echo.HandlerFunc) echo.HandlerFunc {
		return func(c echo.Context) error {
			authHeader := c.Request().Header.Get(protocol.AuthorizationHeader)

			if authHeader == "" {
				challengeHeader, err := m.Charge(c.Request().Context(), cfg)
				if err != nil {
					return c.JSON(http.StatusInternalServerError, map[string]string{"error": "failed to generate payment challenge"})
				}
				c.Response().Header().Set(protocol.WWWAuthenticateHeader, challengeHeader)
				return c.JSON(http.StatusPaymentRequired, map[string]string{"error": "Payment required"})
			}

			challengeHeader, err := server.ChallengeHeaderFromAuth(authHeader)
			if err != nil {
				return c.JSON(http.StatusBadRequest, map[string]string{"error": fmt.Sprintf("invalid Authorization header: %s", err)})
			}

			receipt, err := m.VerifyCredential(c.Request().Context(), challengeHeader, authHeader)
			if err != nil {
				return c.JSON(paymentErrorStatus(err), map[string]string{"error": fmt.Sprintf("payment verification failed: %s", err)})
			}

			if receiptHeader, hErr := receipt.ToHeader(); hErr == nil {
				c.Response().Header().Set(protocol.PaymentReceiptHeader, receiptHeader)
			}
			c.Set(receiptCtxKey, receipt)
			return next(c)
		}
	}
}

// SessionMiddleware returns an Echo middleware that enforces a session-based payment.
func SessionMiddleware(m *server.Mpp, cfg server.SessionRouteConfig) echo.MiddlewareFunc {
	return func(next echo.HandlerFunc) echo.HandlerFunc {
		return func(c echo.Context) error {
			authHeader := c.Request().Header.Get(protocol.AuthorizationHeader)

			if authHeader == "" {
				challengeHeader, err := m.SessionChallenge(c.Request().Context(), cfg)
				if err != nil {
					return c.JSON(http.StatusInternalServerError, map[string]string{"error": "failed to generate session challenge"})
				}
				c.Response().Header().Set(protocol.WWWAuthenticateHeader, challengeHeader)
				return c.JSON(http.StatusPaymentRequired, map[string]string{"error": "Payment required"})
			}

			challengeHeader, err := server.ChallengeHeaderFromAuth(authHeader)
			if err != nil {
				return c.JSON(http.StatusBadRequest, map[string]string{"error": fmt.Sprintf("invalid Authorization header: %s", err)})
			}

			result, err := m.VerifySession(c.Request().Context(), challengeHeader, authHeader)
			if err != nil {
				return c.JSON(paymentErrorStatus(err), map[string]string{"error": fmt.Sprintf("session verification failed: %s", err)})
			}

			if receiptHeader, hErr := result.Receipt.ToHeader(); hErr == nil {
				c.Response().Header().Set(protocol.PaymentReceiptHeader, receiptHeader)
			}
			c.Set(receiptCtxKey, result.Receipt)

			if result.ManagementResponse != nil {
				return c.JSON(http.StatusOK, result.ManagementResponse)
			}
			return next(c)
		}
	}
}

// GetReceipt retrieves the payment receipt from the echo context.
func GetReceipt(c echo.Context) *protocol.Receipt {
	v := c.Get(receiptCtxKey)
	if v == nil {
		return nil
	}
	receipt, _ := v.(*protocol.Receipt)
	return receipt
}
