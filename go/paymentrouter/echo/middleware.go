package echo

import (
	"net/http"

	"github.com/labstack/echo/v4"
	pr "github.com/okx/payments/go/paymentrouter"
)

// PaymentGate holds protocol adapters and produces per-route Echo middleware
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

// For returns an Echo middleware scoped to a single route's payment config.
//
//	e.GET("/api/onetime", handler, paid.For(cfg))
func (g *PaymentGate) For(cfg pr.RouteConfig) echo.MiddlewareFunc {
	return func(next echo.HandlerFunc) echo.HandlerFunc {
		return func(c echo.Context) error {
			detected := pr.Detect(g.protocols, c.Request())
			if detected != nil {
				adapterCfg, ok := cfg[detected.Name()]
				if !ok {
					adapterCfg = struct{}{}
				}
				err := detected.Handle(c.Response(), c.Request(), adapterCfg)
				if err != nil {
					if g.onError != nil {
						g.onError(err, pr.PhaseHandle, detected.Name())
					}
					return nil
				}
				if c.Response().Committed {
					return nil
				}
				return next(c)
			}

			merged := pr.MergeChallenges(c.Request().Context(), g.protocols, c.Request(), cfg,
				func(err error, proto string) {
					if g.onError != nil {
						g.onError(err, pr.PhaseChallenge, proto)
					}
				},
			)
			for name, values := range merged {
				for _, v := range values {
					c.Response().Header().Add(name, v)
				}
			}
			return c.JSON(http.StatusPaymentRequired, map[string]string{"error": "Payment required"})
		}
	}
}
