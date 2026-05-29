package adapters

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"sync"

	"github.com/okx/payments/go/paymentrouter"
	x402http "github.com/okx/payments/go/x402/http"
)

// Ensure X402Adapter implements paymentrouter.ProtocolAdapter.
var _ paymentrouter.ProtocolAdapter = (*X402Adapter)(nil)

// X402Adapter implements paymentrouter.ProtocolAdapter using the x402 HTTP server.
type X402Adapter struct {
	server           *x402http.HTTPServer
	priority         int
	registeredRoutes map[string]bool // tracks routes already added via AddRoutes
	initOnce         sync.Once
	initErr          error
}

// NewX402Adapter creates a new X402Adapter. The server can be created with nil
// routes — routes are registered lazily from the paymentrouter route config JSON.
func NewX402Adapter(server *x402http.HTTPServer) *X402Adapter {
	return &X402Adapter{server: server, priority: 20, registeredRoutes: make(map[string]bool)}
}

// initialize calls the underlying x402 server's Initialize method once to
// populate the facilitator client map from the /supported endpoint.
func (a *X402Adapter) initialize(ctx context.Context) error {
	a.initOnce.Do(func() {
		a.initErr = a.server.Initialize(ctx)
	})
	return a.initErr
}

// ensureRoute registers the route with the x402 server if not already registered.
func (a *X402Adapter) ensureRoute(method, path string, cfg any) {
	endpoint := fmt.Sprintf("%s %s", method, path)
	if a.registeredRoutes[endpoint] {
		return
	}
	rc, ok := cfg.(x402http.RouteConfig)
	if !ok {
		return
	}
	a.server.AddRoutes(x402http.RoutesConfig{endpoint: rc})
	a.registeredRoutes[endpoint] = true
}

// Name returns the protocol name.
func (a *X402Adapter) Name() string { return "x402" }

// Priority returns the adapter's dispatch priority.
func (a *X402Adapter) Priority() int { return a.priority }

// Detect returns true if the request carries an x402 payment header.
func (a *X402Adapter) Detect(r *http.Request) bool {
	return r.Header.Get("PAYMENT-SIGNATURE") != "" || r.Header.Get("X-Payment") != ""
}

// GetChallenge builds a payment-required (402) challenge by delegating to the x402 server.
// It returns the response headers that should be forwarded to the caller.
func (a *X402Adapter) GetChallenge(ctx context.Context, r *http.Request, cfg any) (http.Header, error) {
	if err := a.initialize(ctx); err != nil {
		return nil, fmt.Errorf("x402: initialize: %w", err)
	}
	a.ensureRoute(r.Method, r.URL.Path, cfg)

	reqCtx := x402http.HTTPRequestContext{
		Adapter: &netHTTPAdapter{r: r},
		Path:    r.URL.Path,
		Method:  r.Method,
	}

	result := a.server.ProcessHTTPRequest(ctx, reqCtx, nil)
	if result.Type == x402http.ResultPaymentError && result.Response != nil {
		headers := make(http.Header)
		for k, v := range result.Response.Headers {
			headers.Set(k, v)
		}
		return headers, nil
	}

	return http.Header{}, nil
}

// Handle processes an incoming payment request.
// On verified payment it copies settlement headers to w and returns nil.
// On no-payment-required it returns nil without touching w.
// On any error it writes an error response to w and returns a non-nil error.
func (a *X402Adapter) Handle(w http.ResponseWriter, r *http.Request, cfg any) error {
	if err := a.initialize(r.Context()); err != nil {
		return fmt.Errorf("x402: initialize: %w", err)
	}
	a.ensureRoute(r.Method, r.URL.Path, cfg)

	reqCtx := x402http.HTTPRequestContext{
		Adapter:       &netHTTPAdapter{r: r},
		Path:          r.URL.Path,
		Method:        r.Method,
		PaymentHeader: r.Header.Get("PAYMENT-SIGNATURE"),
	}

	result := a.server.ProcessHTTPRequest(r.Context(), reqCtx, nil)

	switch result.Type {
	case x402http.ResultPaymentVerified:
		if result.Response != nil {
			for k, v := range result.Response.Headers {
				w.Header().Set(k, v)
			}
		}
		// Settle payment after successful verification
		if result.PaymentPayload != nil && result.PaymentRequirements != nil {
			settleResult := a.server.ProcessSettlement(r.Context(), *result.PaymentPayload, *result.PaymentRequirements, nil)
			if settleResult != nil {
				if !settleResult.Success {
					if settleResult.Response != nil {
						for k, v := range settleResult.Response.Headers {
							w.Header().Set(k, v)
						}
						w.WriteHeader(settleResult.Response.Status)
						if settleResult.Response.Body != nil {
							body, _ := json.Marshal(settleResult.Response.Body)
							w.Write(body) //nolint:errcheck
						}
					}
					return fmt.Errorf("x402: settlement failed: %s", settleResult.ErrorReason)
				}
				for k, v := range settleResult.Headers {
					w.Header().Set(k, v)
				}
			}
		}
		return nil

	case x402http.ResultNoPaymentRequired:
		return nil

	default:
		// ResultPaymentError or unexpected result
		var bodyStr string
		if result.Response != nil {
			for k, v := range result.Response.Headers {
				w.Header().Set(k, v)
			}
			w.WriteHeader(result.Response.Status)
			if result.Response.Body != nil {
				body, err := json.Marshal(result.Response.Body)
				if err == nil {
					bodyStr = string(body)
					_, _ = w.Write(body)
				}
			}
		} else {
			w.WriteHeader(http.StatusPaymentRequired)
		}
		return fmt.Errorf("x402: payment error (result type: %s, body: %s)", result.Type, bodyStr)
	}
}

// ============================================================================
// netHTTPAdapter — adapts *http.Request to x402http.HTTPAdapter
// ============================================================================

type netHTTPAdapter struct {
	r *http.Request
}

func (a *netHTTPAdapter) GetHeader(name string) string { return a.r.Header.Get(name) }
func (a *netHTTPAdapter) GetMethod() string            { return a.r.Method }
func (a *netHTTPAdapter) GetPath() string              { return a.r.URL.Path }

func (a *netHTTPAdapter) GetURL() string {
	scheme := "http"
	if a.r.TLS != nil || a.r.Header.Get("X-Forwarded-Proto") == "https" {
		scheme = "https"
	}
	host := a.r.Host
	if host == "" {
		host = a.r.URL.Host
	}
	return scheme + "://" + host + a.r.RequestURI
}

func (a *netHTTPAdapter) GetAcceptHeader() string { return a.r.Header.Get("Accept") }
func (a *netHTTPAdapter) GetUserAgent() string    { return a.r.Header.Get("User-Agent") }
