package nethttp

import (
	"encoding/json"
	"net/http"

	pr "github.com/okx/payments/go/paymentrouter"
)

// PaymentGate holds protocol adapters and produces per-route net/http middleware
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

// responseRecorder wraps http.ResponseWriter to track whether a status has been written.
type responseRecorder struct {
	http.ResponseWriter
	written bool
}

func (r *responseRecorder) WriteHeader(code int) {
	r.written = true
	r.ResponseWriter.WriteHeader(code)
}

func (r *responseRecorder) Write(b []byte) (int, error) {
	r.written = true
	return r.ResponseWriter.Write(b)
}

// For returns a net/http middleware func scoped to a single route's payment config.
//
//	mux.Handle("GET /api/onetime", paid.For(cfg)(handler))
func (g *PaymentGate) For(cfg pr.RouteConfig) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			detected := pr.Detect(g.protocols, r)
			if detected != nil {
				adapterCfg, ok := cfg[detected.Name()]
				if !ok {
					adapterCfg = struct{}{}
				}
				rec := &responseRecorder{ResponseWriter: w}
				err := detected.Handle(rec, r, adapterCfg)
				if err != nil {
					if g.onError != nil {
						g.onError(err, pr.PhaseHandle, detected.Name())
					}
					return
				}
				if rec.written {
					return
				}
				next.ServeHTTP(w, r)
				return
			}

			merged := pr.MergeChallenges(r.Context(), g.protocols, r, cfg,
				func(err error, proto string) {
					if g.onError != nil {
						g.onError(err, pr.PhaseChallenge, proto)
					}
				},
			)
			for name, values := range merged {
				for _, v := range values {
					w.Header().Add(name, v)
				}
			}
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusPaymentRequired)
			json.NewEncoder(w).Encode(map[string]string{"error": "Payment required"}) //nolint:errcheck
		})
	}
}
