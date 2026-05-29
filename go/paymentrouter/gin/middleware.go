package gin

import (
	"net/http"

	"github.com/gin-gonic/gin"
	pr "github.com/okx/payments/go/paymentrouter"
)

// PaymentGate holds protocol adapters and produces per-route Gin middleware
// via the For method.
type PaymentGate struct {
	protocols []pr.ProtocolAdapter
	onError   func(err error, phase, protocol string)
}

// New creates a PaymentGate with the given protocol adapters.
func New(protocols []pr.ProtocolAdapter, opts ...Option) *PaymentGate {
	g := &PaymentGate{protocols: protocols}
	for _, o := range opts {
		o(g)
	}
	return g
}

// Option configures a PaymentGate.
type Option func(*PaymentGate)

// WithOnError sets the error callback.
func WithOnError(fn func(err error, phase, protocol string)) Option {
	return func(g *PaymentGate) { g.onError = fn }
}

// For returns a Gin middleware scoped to a single route's payment config.
//
//	r.GET("/api/onetime", paid.For(cfg), handler)
func (g *PaymentGate) For(cfg pr.RouteConfig) gin.HandlerFunc {
	return func(c *gin.Context) {
		detected := pr.Detect(g.protocols, c.Request)
		if detected != nil {
			adapterCfg, ok := cfg[detected.Name()]
			if !ok {
				adapterCfg = struct{}{}
			}
			err := detected.Handle(c.Writer, c.Request, adapterCfg)
			if err != nil {
				if g.onError != nil {
					g.onError(err, pr.PhaseHandle, detected.Name())
				}
				c.Abort()
				return
			}
			if c.Writer.Written() {
				c.Abort()
				return
			}
			c.Next()
			return
		}

		merged := pr.MergeChallenges(c.Request.Context(), g.protocols, c.Request, cfg,
			func(err error, proto string) {
				if g.onError != nil {
					g.onError(err, pr.PhaseChallenge, proto)
				}
			},
		)
		for name, values := range merged {
			for _, v := range values {
				c.Writer.Header().Add(name, v)
			}
		}
		c.JSON(http.StatusPaymentRequired, gin.H{"error": "Payment required"})
		c.Abort()
	}
}
