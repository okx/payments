package gin

import (
	"errors"
	"fmt"
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/okx/payments/go/mpp/protocol"
	"github.com/okx/payments/go/mpp/server"
)

// paymentErrorStatus extracts the HTTP status from a VerificationError,
// defaulting to 402 Payment Required.
func paymentErrorStatus(err error) int {
	var ve *protocol.VerificationError
	if errors.As(err, &ve) {
		return ve.HTTPStatus()
	}
	return http.StatusPaymentRequired
}

// ChargeMiddleware returns a Gin middleware that enforces a one-shot charge payment.
func ChargeMiddleware(m *server.Mpp, cfg server.ChargeRouteConfig) gin.HandlerFunc {
	return func(c *gin.Context) {
		authHeader := c.GetHeader(protocol.AuthorizationHeader)

		if authHeader == "" {
			challengeHeader, err := m.Charge(c.Request.Context(), cfg)
			if err != nil {
				c.AbortWithStatusJSON(http.StatusInternalServerError, gin.H{
					"error": "failed to generate payment challenge",
				})
				return
			}
			c.Header(protocol.WWWAuthenticateHeader, challengeHeader)
			c.AbortWithStatus(http.StatusPaymentRequired)
			return
		}

		challengeHeader, err := server.ChallengeHeaderFromAuth(authHeader)
		if err != nil {
			c.AbortWithStatusJSON(http.StatusBadRequest, gin.H{
				"error": fmt.Sprintf("invalid Authorization header: %s", err),
			})
			return
		}

		receipt, err := m.VerifyCredential(c.Request.Context(), challengeHeader, authHeader)
		if err != nil {
			c.AbortWithStatusJSON(paymentErrorStatus(err), gin.H{
				"error": fmt.Sprintf("payment verification failed: %s", err),
			})
			return
		}

		if receiptHeader, hErr := receipt.ToHeader(); hErr == nil {
			c.Header(protocol.PaymentReceiptHeader, receiptHeader)
		}
		c.Set(server.ReceiptContextKey, receipt)
		c.Next()
	}
}

// SessionMiddleware returns a Gin middleware that enforces a session-based payment.
func SessionMiddleware(m *server.Mpp, cfg server.SessionRouteConfig) gin.HandlerFunc {
	return func(c *gin.Context) {
		authHeader := c.GetHeader(protocol.AuthorizationHeader)

		if authHeader == "" {
			challengeHeader, err := m.SessionChallenge(c.Request.Context(), cfg)
			if err != nil {
				c.AbortWithStatusJSON(http.StatusInternalServerError, gin.H{
					"error": "failed to generate session challenge",
				})
				return
			}
			c.Header(protocol.WWWAuthenticateHeader, challengeHeader)
			c.AbortWithStatus(http.StatusPaymentRequired)
			return
		}

		challengeHeader, err := server.ChallengeHeaderFromAuth(authHeader)
		if err != nil {
			c.AbortWithStatusJSON(http.StatusBadRequest, gin.H{
				"error": fmt.Sprintf("invalid Authorization header: %s", err),
			})
			return
		}

		result, err := m.VerifySession(c.Request.Context(), challengeHeader, authHeader)
		if err != nil {
			c.AbortWithStatusJSON(paymentErrorStatus(err), gin.H{
				"error": fmt.Sprintf("session verification failed: %s", err),
			})
			return
		}

		if receiptHeader, hErr := result.Receipt.ToHeader(); hErr == nil {
			c.Header(protocol.PaymentReceiptHeader, receiptHeader)
		}
		c.Set(server.ReceiptContextKey, result.Receipt)

		// Management actions (open, topUp, close) return receipt only;
		// only voucher/replay actions proceed to the inner handler.
		if result.ManagementResponse != nil {
			c.AbortWithStatusJSON(http.StatusOK, result.ManagementResponse)
			return
		}
		c.Next()
	}
}

// GetReceipt retrieves the payment receipt stored in the gin context.
func GetReceipt(c *gin.Context) *protocol.Receipt {
	v, exists := c.Get(server.ReceiptContextKey)
	if !exists {
		return nil
	}
	receipt, _ := v.(*protocol.Receipt)
	return receipt
}
