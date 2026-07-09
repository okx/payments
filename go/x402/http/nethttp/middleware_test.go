package nethttp

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/okx/payments/go/x402"
	x402http "github.com/okx/payments/go/x402/http"
	"github.com/okx/payments/go/x402/subscription"
	"github.com/okx/payments/go/x402/types"
)

// ============================================================================
// Mock Implementations
// ============================================================================

// mockSchemeServer implements x402.SchemeNetworkServer for testing.
type mockSchemeServer struct {
	scheme string
}

func (m *mockSchemeServer) Scheme() string {
	return m.scheme
}

func (m *mockSchemeServer) ParsePrice(price x402.Price, network x402.Network) (x402.AssetAmount, error) {
	return x402.AssetAmount{
		Asset:  "USDC",
		Amount: "1000000",
	}, nil
}

func (m *mockSchemeServer) EnhancePaymentRequirements(ctx context.Context, base types.PaymentRequirements, supported types.SupportedKind, extensions []string) (types.PaymentRequirements, error) {
	return base, nil
}

// mockFacilitatorClient implements x402.FacilitatorClient for testing.
type mockFacilitatorClient struct {
	verifyFunc          func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.VerifyResponse, error)
	verifySignatureFunc func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.VerifyResponse, error)
	settleFunc          func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.SettleResponse, error)
	supportedFunc       func(ctx context.Context) (x402.SupportedResponse, error)
}

func (m *mockFacilitatorClient) Verify(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.VerifyResponse, error) {
	if m.verifyFunc != nil {
		return m.verifyFunc(ctx, payloadBytes, requirementsBytes)
	}
	return &x402.VerifyResponse{IsValid: true, Payer: "0xmock"}, nil
}

func (m *mockFacilitatorClient) VerifySignature(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.VerifyResponse, error) {
	if m.verifySignatureFunc != nil {
		return m.verifySignatureFunc(ctx, payloadBytes, requirementsBytes)
	}
	return &x402.VerifyResponse{IsValid: true, Payer: "0xmock"}, nil
}

func (m *mockFacilitatorClient) Settle(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.SettleResponse, error) {
	if m.settleFunc != nil {
		return m.settleFunc(ctx, payloadBytes, requirementsBytes)
	}
	return &x402.SettleResponse{Success: true, Transaction: "0xtx", Network: "eip155:1", Payer: "0xmock"}, nil
}

func (m *mockFacilitatorClient) GetSupported(ctx context.Context) (x402.SupportedResponse, error) {
	if m.supportedFunc != nil {
		return m.supportedFunc(ctx)
	}
	return x402.SupportedResponse{
		Kinds: []x402.SupportedKind{
			{X402Version: 2, Scheme: "exact", Network: "eip155:1"},
		},
		Extensions: []string{},
		Signers:    make(map[string][]string),
	}, nil
}

func (m *mockFacilitatorClient) Identifier() string {
	return "mock"
}

// ============================================================================
// Test Helpers
// ============================================================================

// createPaymentHeader creates a base64-encoded payment header for testing.
//
//nolint:unparam // payTo is always "0xtest" in current tests but keeping param for flexibility
func createPaymentHeader(payTo string) string {
	payload := x402.PaymentPayload{
		X402Version: 2,
		Payload:     map[string]any{"sig": "test"},
		Accepted: x402.PaymentRequirements{
			Scheme:            "exact",
			Network:           "eip155:1",
			Asset:             "USDC",
			Amount:            "1000000",
			PayTo:             payTo,
			MaxTimeoutSeconds: 300,
			Extra: map[string]any{
				"resourceUrl": "http://example.com/api",
			},
		},
	}

	payloadJSON, _ := json.Marshal(payload)
	return base64.StdEncoding.EncodeToString(payloadJSON)
}

// defaultSupportedFunc returns a standard supported response function for tests.
func defaultSupportedFunc() func(ctx context.Context) (x402.SupportedResponse, error) {
	return func(ctx context.Context) (x402.SupportedResponse, error) {
		return x402.SupportedResponse{
			Kinds: []x402.SupportedKind{
				{X402Version: 2, Scheme: "exact", Network: "eip155:1"},
			},
			Extensions: []string{},
			Signers:    make(map[string][]string),
		}, nil
	}
}

// ============================================================================
// NetHTTPAdapter Tests
// ============================================================================

func TestNetHTTPAdapter_GetHeader(t *testing.T) {
	req := httptest.NewRequest("GET", "/test", nil)
	req.Header.Set("X-Custom-Header", "test-value")
	req.Header.Set("payment-signature", "sig-data")

	adapter := NewNetHTTPAdapter(req)

	if adapter.GetHeader("X-Custom-Header") != "test-value" {
		t.Error("Expected X-Custom-Header to be 'test-value'")
	}

	if adapter.GetHeader("payment-signature") != "sig-data" {
		t.Error("Expected payment-signature header")
	}
}

func TestNetHTTPAdapter_GetMethod(t *testing.T) {
	tests := []struct {
		method   string
		expected string
	}{
		{"GET", "GET"},
		{"POST", "POST"},
		{"PUT", "PUT"},
		{"DELETE", "DELETE"},
	}

	for _, tt := range tests {
		t.Run(tt.method, func(t *testing.T) {
			req := httptest.NewRequest(tt.method, "/test", nil)
			adapter := NewNetHTTPAdapter(req)

			if adapter.GetMethod() != tt.expected {
				t.Errorf("Expected method %s, got %s", tt.expected, adapter.GetMethod())
			}
		})
	}
}

func TestNetHTTPAdapter_GetPath(t *testing.T) {
	req := httptest.NewRequest("GET", "/api/users/123", nil)
	adapter := NewNetHTTPAdapter(req)

	if adapter.GetPath() != "/api/users/123" {
		t.Errorf("Expected path '/api/users/123', got '%s'", adapter.GetPath())
	}
}

func TestNetHTTPAdapter_GetURL(t *testing.T) {
	tests := []struct {
		name     string
		target   string
		expected string
	}{
		{
			name:     "with query params",
			target:   "/api/test?id=1",
			expected: "http://example.com/api/test?id=1",
		},
		{
			name:     "without query params",
			target:   "/api/test",
			expected: "http://example.com/api/test",
		},
		{
			name:     "with multiple query params",
			target:   "/api/test?id=1&foo=bar",
			expected: "http://example.com/api/test?id=1&foo=bar",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req := httptest.NewRequest("GET", tt.target, nil)
			req.Host = "example.com"
			adapter := NewNetHTTPAdapter(req)

			if adapter.GetURL() != tt.expected {
				t.Errorf("Expected URL '%s', got '%s'", tt.expected, adapter.GetURL())
			}
		})
	}
}

func TestNetHTTPAdapter_GetAcceptHeader(t *testing.T) {
	req := httptest.NewRequest("GET", "/test", nil)
	req.Header.Set("Accept", "text/html")

	adapter := NewNetHTTPAdapter(req)

	if adapter.GetAcceptHeader() != "text/html" {
		t.Errorf("Expected Accept header 'text/html', got '%s'", adapter.GetAcceptHeader())
	}
}

func TestNetHTTPAdapter_GetUserAgent(t *testing.T) {
	req := httptest.NewRequest("GET", "/test", nil)
	req.Header.Set("User-Agent", "Mozilla/5.0")

	adapter := NewNetHTTPAdapter(req)

	if adapter.GetUserAgent() != "Mozilla/5.0" {
		t.Errorf("Expected User-Agent 'Mozilla/5.0', got '%s'", adapter.GetUserAgent())
	}
}

// ============================================================================
// PaymentMiddleware Tests
// ============================================================================

func TestPaymentMiddleware_CallsNextWhenNoPaymentRequired(t *testing.T) {
	routes := x402http.RoutesConfig{
		"GET /api": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	nextCalled := false
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		nextCalled = true
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"message": "success"})
	})

	middleware := PaymentMiddlewareFromConfig(routes, WithSyncFacilitatorOnStart(false))
	wrapped := middleware(handler)

	req := httptest.NewRequest("GET", "/public", nil)
	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if !nextCalled {
		t.Error("Expected next handler to be called for non-protected route")
	}
	if w.Code != http.StatusOK {
		t.Errorf("Expected status 200, got %d", w.Code)
	}
}

func TestPaymentMiddleware_Returns402JSONForPaymentError(t *testing.T) {
	mockClient := &mockFacilitatorClient{supportedFunc: defaultSupportedFunc()}
	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"GET /api": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
			Description: "API access",
		},
	}

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"data": "protected"})
	})

	middleware := PaymentMiddlewareFromConfig(routes,
		WithFacilitatorClient(mockClient),
		WithScheme("eip155:1", mockServer),
		WithSyncFacilitatorOnStart(true),
		WithTimeout(5*time.Second),
	)
	wrapped := middleware(handler)

	req := httptest.NewRequest("GET", "/api", nil)
	req.Header.Set("Accept", "application/json")

	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("Expected status 402, got %d", w.Code)
	}

	if w.Header().Get("PAYMENT-REQUIRED") == "" {
		t.Error("Expected PAYMENT-REQUIRED header")
	}
}

func TestPaymentMiddleware_Returns402HTMLForBrowserRequest(t *testing.T) {
	mockClient := &mockFacilitatorClient{supportedFunc: defaultSupportedFunc()}
	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"*": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$5.00",
					Network: "eip155:1",
				},
			},
			Description: "Premium content",
		},
	}

	paywallConfig := &x402http.PaywallConfig{
		AppName: "Test App",
	}

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"data": "protected"})
	})

	middleware := PaymentMiddlewareFromConfig(routes,
		WithFacilitatorClient(mockClient),
		WithScheme("eip155:1", mockServer),
		WithPaywallConfig(paywallConfig),
		WithSyncFacilitatorOnStart(true),
		WithTimeout(5*time.Second),
	)
	wrapped := middleware(handler)

	req := httptest.NewRequest("GET", "/content", nil)
	req.Header.Set("Accept", "text/html")
	req.Header.Set("User-Agent", "Mozilla/5.0")

	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("Expected status 402, got %d", w.Code)
	}

	contentType := w.Header().Get("Content-Type")
	if !bytes.Contains([]byte(contentType), []byte("text/html")) {
		t.Errorf("Expected Content-Type to contain 'text/html', got '%s'", contentType)
	}

	body := w.Body.String()
	if !bytes.Contains([]byte(body), []byte("Payment Required")) {
		t.Error("Expected 'Payment Required' in HTML body")
	}
	if !bytes.Contains([]byte(body), []byte("Test App")) {
		t.Error("Expected app name in HTML body")
	}
}

func TestPaymentMiddleware_SettlesAndReturnsResponseForVerifiedPayment(t *testing.T) {
	settleCalled := false

	mockClient := &mockFacilitatorClient{
		verifyFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.VerifyResponse, error) {
			return &x402.VerifyResponse{IsValid: true, Payer: "0xpayer"}, nil
		},
		settleFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.SettleResponse, error) {
			settleCalled = true
			return &x402.SettleResponse{
				Success:     true,
				Transaction: "0xtx",
				Network:     "eip155:1",
				Payer:       "0xpayer",
			}, nil
		},
		supportedFunc: defaultSupportedFunc(),
	}

	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"POST /api": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"data": "protected-data"})
	})

	middleware := PaymentMiddlewareFromConfig(routes,
		WithFacilitatorClient(mockClient),
		WithScheme("eip155:1", mockServer),
		WithSyncFacilitatorOnStart(true),
		WithTimeout(5*time.Second),
	)
	wrapped := middleware(handler)

	req := httptest.NewRequest("POST", "/api", nil)
	req.Header.Set("PAYMENT-SIGNATURE", createPaymentHeader("0xtest"))
	req.Host = "example.com"

	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Errorf("Expected status 200, got %d. Body: %s", w.Code, w.Body.String())
	}

	if !settleCalled {
		t.Error("Expected settlement to be called")
	}

	if w.Header().Get("PAYMENT-RESPONSE") == "" {
		t.Error("Expected PAYMENT-RESPONSE header")
	}
}

func TestPaymentMiddleware_SkipsSettlementWhenHandlerReturns400OrHigher(t *testing.T) {
	settleCalled := false

	mockClient := &mockFacilitatorClient{
		verifyFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.VerifyResponse, error) {
			return &x402.VerifyResponse{IsValid: true, Payer: "0xpayer"}, nil
		},
		settleFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.SettleResponse, error) {
			settleCalled = true
			return &x402.SettleResponse{Success: true, Transaction: "0xtx"}, nil
		},
		supportedFunc: defaultSupportedFunc(),
	}

	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"POST /api": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusInternalServerError)
		_ = json.NewEncoder(w).Encode(map[string]string{"error": "internal error"})
	})

	middleware := PaymentMiddlewareFromConfig(routes,
		WithFacilitatorClient(mockClient),
		WithScheme("eip155:1", mockServer),
		WithSyncFacilitatorOnStart(true),
		WithTimeout(5*time.Second),
	)
	wrapped := middleware(handler)

	req := httptest.NewRequest("POST", "/api", nil)
	req.Header.Set("PAYMENT-SIGNATURE", createPaymentHeader("0xtest"))
	req.Host = "example.com"

	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusInternalServerError {
		t.Errorf("Expected status 500, got %d", w.Code)
	}

	if settleCalled {
		t.Error("Settlement should NOT be called when handler returns >= 400")
	}
}

func TestPaymentMiddleware_Returns402WhenSettlementFails(t *testing.T) {
	mockClient := &mockFacilitatorClient{
		verifyFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.VerifyResponse, error) {
			return &x402.VerifyResponse{IsValid: true, Payer: "0xpayer"}, nil
		},
		settleFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.SettleResponse, error) {
			return &x402.SettleResponse{
				Success:     false,
				ErrorReason: "Insufficient funds",
			}, nil
		},
		supportedFunc: defaultSupportedFunc(),
	}

	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"POST /api": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"data": "protected-data"})
	})

	middleware := PaymentMiddlewareFromConfig(routes,
		WithFacilitatorClient(mockClient),
		WithScheme("eip155:1", mockServer),
		WithSyncFacilitatorOnStart(true),
		WithTimeout(5*time.Second),
	)
	wrapped := middleware(handler)

	req := httptest.NewRequest("POST", "/api", nil)
	req.Header.Set("PAYMENT-SIGNATURE", createPaymentHeader("0xtest"))
	req.Host = "example.com"

	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("Expected status 402, got %d", w.Code)
	}

	// Empty body by default on settlement failure
	var response map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &response); err != nil {
		t.Fatalf("Failed to parse response: %v", err)
	}
	if len(response) != 0 {
		t.Errorf("Expected empty body {}, got %v", response)
	}

	// PAYMENT-RESPONSE header must be included on settlement failure
	if w.Header().Get("PAYMENT-RESPONSE") == "" {
		t.Error("Expected PAYMENT-RESPONSE header on settlement failure")
	}
}

func TestPaymentMiddleware_CustomErrorHandler(t *testing.T) {
	customHandlerCalled := false

	mockClient := &mockFacilitatorClient{
		verifyFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.VerifyResponse, error) {
			return &x402.VerifyResponse{IsValid: true, Payer: "0xpayer"}, nil
		},
		settleFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.SettleResponse, error) {
			return &x402.SettleResponse{
				Success:     false,
				ErrorReason: "Settlement rejected",
			}, nil
		},
		supportedFunc: defaultSupportedFunc(),
	}

	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"POST /api": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	customErrorHandler := func(w http.ResponseWriter, r *http.Request, err error) {
		customHandlerCalled = true
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusPaymentRequired)
		_ = json.NewEncoder(w).Encode(map[string]string{
			"custom_error": err.Error(),
		})
	}

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"data": "protected-data"})
	})

	middleware := PaymentMiddlewareFromConfig(routes,
		WithFacilitatorClient(mockClient),
		WithScheme("eip155:1", mockServer),
		WithErrorHandler(customErrorHandler),
		WithSyncFacilitatorOnStart(true),
		WithTimeout(5*time.Second),
	)
	wrapped := middleware(handler)

	req := httptest.NewRequest("POST", "/api", nil)
	req.Header.Set("PAYMENT-SIGNATURE", createPaymentHeader("0xtest"))
	req.Host = "example.com"

	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if !customHandlerCalled {
		t.Error("Expected custom error handler to be called")
	}

	var response map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &response); err != nil {
		t.Fatalf("Failed to parse response: %v", err)
	}

	if response["custom_error"] == nil {
		t.Error("Expected custom_error in response")
	}
}

func TestPaymentMiddleware_CustomSettlementHandler(t *testing.T) {
	settlementHandlerCalled := false
	var capturedSettleResponse *x402.SettleResponse

	mockClient := &mockFacilitatorClient{
		verifyFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.VerifyResponse, error) {
			return &x402.VerifyResponse{IsValid: true, Payer: "0xpayer"}, nil
		},
		settleFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.SettleResponse, error) {
			return &x402.SettleResponse{
				Success:     true,
				Transaction: "0xtx123",
				Network:     "eip155:1",
				Payer:       "0xpayer",
			}, nil
		},
		supportedFunc: defaultSupportedFunc(),
	}

	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"POST /api": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	customSettlementHandler := func(w http.ResponseWriter, r *http.Request, settleResponse *x402.SettleResponse) {
		settlementHandlerCalled = true
		capturedSettleResponse = settleResponse
		w.Header().Set("X-Transaction-ID", settleResponse.Transaction)
	}

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"data": "protected-data"})
	})

	middleware := PaymentMiddlewareFromConfig(routes,
		WithFacilitatorClient(mockClient),
		WithScheme("eip155:1", mockServer),
		WithSettlementHandler(customSettlementHandler),
		WithSyncFacilitatorOnStart(true),
		WithTimeout(5*time.Second),
	)
	wrapped := middleware(handler)

	req := httptest.NewRequest("POST", "/api", nil)
	req.Header.Set("PAYMENT-SIGNATURE", createPaymentHeader("0xtest"))
	req.Host = "example.com"

	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Errorf("Expected status 200, got %d", w.Code)
	}

	if !settlementHandlerCalled {
		t.Error("Expected custom settlement handler to be called")
	}

	if capturedSettleResponse == nil {
		t.Fatal("Expected settle response to be captured")
	}

	if capturedSettleResponse.Transaction != "0xtx123" {
		t.Errorf("Expected transaction '0xtx123', got '%s'", capturedSettleResponse.Transaction)
	}

	if w.Header().Get("X-Transaction-ID") != "0xtx123" {
		t.Error("Expected custom X-Transaction-ID header")
	}
}

func TestPaymentMiddleware_WithTimeout(t *testing.T) {
	mockClient := &mockFacilitatorClient{supportedFunc: defaultSupportedFunc()}
	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"*": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	timeout := 10 * time.Second

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"message": "success"})
	})

	middleware := PaymentMiddlewareFromConfig(routes,
		WithFacilitatorClient(mockClient),
		WithScheme("eip155:1", mockServer),
		WithTimeout(timeout),
		WithSyncFacilitatorOnStart(true),
	)
	wrapped := middleware(handler)

	req := httptest.NewRequest("GET", "/test", nil)
	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("Expected status 402, got %d", w.Code)
	}
}

// ============================================================================
// X402Payment (Builder Pattern) Tests
// ============================================================================

func TestX402Payment_CreatesWorkingMiddleware(t *testing.T) {
	mockClient := &mockFacilitatorClient{supportedFunc: defaultSupportedFunc()}
	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"GET /api": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	protectedHandler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"data": "protected"})
	})

	middleware := X402Payment(Config{
		Routes:      routes,
		Facilitator: mockClient,
		Schemes: []SchemeConfig{
			{Network: "eip155:1", Server: mockServer},
		},
		SyncFacilitatorOnStart: true,
		Timeout:                5 * time.Second,
	})
	wrapped := middleware(protectedHandler)

	// Test non-protected route passes through
	req := httptest.NewRequest("GET", "/public", nil)
	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Errorf("Expected status 200 for public route, got %d", w.Code)
	}

	// Test protected route requires payment
	req = httptest.NewRequest("GET", "/api", nil)
	w = httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("Expected status 402 for protected route, got %d", w.Code)
	}
}

func TestX402Payment_RegistersMultipleFacilitators(t *testing.T) {
	mockClient1 := &mockFacilitatorClient{supportedFunc: defaultSupportedFunc()}
	mockClient2 := &mockFacilitatorClient{supportedFunc: defaultSupportedFunc()}
	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"*": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"message": "success"})
	})

	middleware := X402Payment(Config{
		Routes:       routes,
		Facilitators: []x402.FacilitatorClient{mockClient1, mockClient2},
		Schemes: []SchemeConfig{
			{Network: "eip155:1", Server: mockServer},
		},
		SyncFacilitatorOnStart: true,
	})
	wrapped := middleware(handler)

	req := httptest.NewRequest("GET", "/test", nil)
	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("Expected status 402, got %d", w.Code)
	}
}

func TestX402Payment_RegistersMultipleSchemes(t *testing.T) {
	mockServer1 := &mockSchemeServer{scheme: "exact"}
	mockServer2 := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"*": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"message": "success"})
	})

	middleware := X402Payment(Config{
		Routes: routes,
		Schemes: []SchemeConfig{
			{Network: "eip155:1", Server: mockServer1},
			{Network: "eip155:8453", Server: mockServer2},
		},
		SyncFacilitatorOnStart: false,
	})
	wrapped := middleware(handler)

	req := httptest.NewRequest("GET", "/test", nil)
	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("Expected status 402, got %d", w.Code)
	}
}

// ============================================================================
// Context Helper Tests
// ============================================================================

func TestPayloadFromContext_ReturnsPayload(t *testing.T) {
	payload := &types.PaymentPayload{
		X402Version: 2,
		Payload:     map[string]any{"sig": "test"},
	}

	ctx := withPayload(context.Background(), payload)
	got, ok := PayloadFromContext(ctx)

	if !ok {
		t.Fatal("Expected payload to be found in context")
	}
	if got.X402Version != 2 {
		t.Errorf("Expected X402Version 2, got %d", got.X402Version)
	}
}

func TestPayloadFromContext_ReturnsFalseWhenMissing(t *testing.T) {
	_, ok := PayloadFromContext(context.Background())
	if ok {
		t.Error("Expected payload not to be found in empty context")
	}
}

func TestRequirementsFromContext_ReturnsRequirements(t *testing.T) {
	reqs := &types.PaymentRequirements{
		Scheme:  "exact",
		Network: "eip155:1",
	}

	ctx := withRequirements(context.Background(), reqs)
	got, ok := RequirementsFromContext(ctx)

	if !ok {
		t.Fatal("Expected requirements to be found in context")
	}
	if got.Scheme != "exact" {
		t.Errorf("Expected scheme 'exact', got '%s'", got.Scheme)
	}
}

func TestRequirementsFromContext_ReturnsFalseWhenMissing(t *testing.T) {
	_, ok := RequirementsFromContext(context.Background())
	if ok {
		t.Error("Expected requirements not to be found in empty context")
	}
}

// ============================================================================
// responseCapture Tests
// ============================================================================

func TestResponseCapture_CapturesStatusCode(t *testing.T) {
	capture := &responseCapture{
		ResponseWriter: httptest.NewRecorder(),
		body:           &bytes.Buffer{},
		statusCode:     http.StatusOK,
	}

	capture.WriteHeader(http.StatusCreated)

	if capture.statusCode != http.StatusCreated {
		t.Errorf("Expected status 201, got %d", capture.statusCode)
	}
}

func TestResponseCapture_CapturesBody(t *testing.T) {
	capture := &responseCapture{
		ResponseWriter: httptest.NewRecorder(),
		body:           &bytes.Buffer{},
		statusCode:     http.StatusOK,
	}

	data := []byte(`{"message":"test"}`)
	n, err := capture.Write(data)

	if err != nil {
		t.Fatalf("Write failed: %v", err)
	}
	if n != len(data) {
		t.Errorf("Expected to write %d bytes, wrote %d", len(data), n)
	}
	if capture.body.String() != `{"message":"test"}` {
		t.Errorf("Expected body '%s', got '%s'", `{"message":"test"}`, capture.body.String())
	}
}

func TestResponseCapture_WriteHeaderOnlyOnce(t *testing.T) {
	capture := &responseCapture{
		ResponseWriter: httptest.NewRecorder(),
		body:           &bytes.Buffer{},
		statusCode:     http.StatusOK,
	}

	capture.WriteHeader(http.StatusCreated)
	capture.WriteHeader(http.StatusAccepted) // Should be ignored

	if capture.statusCode != http.StatusCreated {
		t.Errorf("Expected status 201 (first call), got %d", capture.statusCode)
	}
}

func TestPaymentMiddleware_PayloadAvailableInDownstreamHandler(t *testing.T) {
	var capturedPayload *types.PaymentPayload

	mockClient := &mockFacilitatorClient{
		verifyFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.VerifyResponse, error) {
			return &x402.VerifyResponse{IsValid: true, Payer: "0xpayer"}, nil
		},
		settleFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.SettleResponse, error) {
			return &x402.SettleResponse{
				Success:     true,
				Transaction: "0xtx",
				Network:     "eip155:1",
				Payer:       "0xpayer",
			}, nil
		},
		supportedFunc: defaultSupportedFunc(),
	}

	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"POST /api": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		payload, ok := PayloadFromContext(r.Context())
		if ok {
			capturedPayload = payload
		}
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"ok":true}`))
	})

	middleware := PaymentMiddlewareFromConfig(routes,
		WithFacilitatorClient(mockClient),
		WithScheme("eip155:1", mockServer),
		WithSyncFacilitatorOnStart(true),
		WithTimeout(5*time.Second),
	)
	wrapped := middleware(handler)

	req := httptest.NewRequest("POST", "/api", nil)
	req.Header.Set("PAYMENT-SIGNATURE", createPaymentHeader("0xtest"))
	req.Host = "example.com"

	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Errorf("Expected status 200, got %d. Body: %s", w.Code, w.Body.String())
	}

	if capturedPayload == nil {
		t.Fatal("Expected payment payload to be available in downstream handler context")
	}

	if capturedPayload.X402Version != 2 {
		t.Errorf("Expected X402Version 2, got %d", capturedPayload.X402Version)
	}
}

// ============================================================================
// PaymentMiddlewareFromHTTPServer Tests
// ============================================================================

func TestPaymentMiddlewareFromHTTPServer_Returns402ForProtectedRoute(t *testing.T) {
	mockClient := &mockFacilitatorClient{supportedFunc: defaultSupportedFunc()}
	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"GET /api": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	// Create resource server and wrap as HTTPServer (same pattern as user would)
	resourceServer := x402.Newx402ResourceServer(
		x402.WithFacilitatorClient(mockClient),
	)
	resourceServer.Register("eip155:1", mockServer)

	httpServer := x402http.Wrappedx402HTTPResourceServer(routes, resourceServer)

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"data": "protected"})
	})

	middleware := PaymentMiddlewareFromHTTPServer(httpServer,
		WithSyncFacilitatorOnStart(true),
		WithTimeout(5*time.Second),
	)
	wrapped := middleware(handler)

	// Protected route should require payment
	req := httptest.NewRequest("GET", "/api", nil)
	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("Expected status 402 for protected route, got %d", w.Code)
	}

	// Non-protected route should pass through
	req = httptest.NewRequest("GET", "/public", nil)
	w = httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Errorf("Expected status 200 for public route, got %d", w.Code)
	}
}

// ============================================================================
// Period Subscription Settlement Tests
// ============================================================================

func boolPtr(b bool) *bool { return &b }

// captureSubscriptionFacilitator records the syncSettle flag the middleware
// passes down when settling a period subscribe, and returns an already-active
// subscription so settlement does not poll.
type captureSubscriptionFacilitator struct {
	createSyncSettle *bool
}

func (c *captureSubscriptionFacilitator) CreateSubscription(_ context.Context, req *subscription.CreateSubscriptionRequest) (*subscription.CreateSubscriptionResponse, error) {
	v := req.SyncSettle
	c.createSyncSettle = &v
	return &subscription.CreateSubscriptionResponse{SubID: "0xnew", State: subscription.StateActive}, nil
}
func (c *captureSubscriptionFacilitator) Charge(_ context.Context, _ *subscription.ChargeRequest) (*subscription.ChargeResponse, error) {
	return &subscription.ChargeResponse{}, nil
}
func (c *captureSubscriptionFacilitator) ChangeSubscription(_ context.Context, _ *subscription.ChangeSubscriptionRequest) (*subscription.ChangeResponse, error) {
	return &subscription.ChangeResponse{NewSubID: "0xnew", State: subscription.StateActive}, nil
}
func (c *captureSubscriptionFacilitator) CancelSubscription(_ context.Context, req *subscription.CancelSubscriptionRequest) (*subscription.TxResultResponse, error) {
	return &subscription.TxResultResponse{SubID: req.SubID}, nil
}
func (c *captureSubscriptionFacilitator) CancelPendingChange(_ context.Context, req *subscription.CancelPendingChangeRequest) (*subscription.TxResultResponse, error) {
	return &subscription.TxResultResponse{SubID: req.SubID}, nil
}
func (c *captureSubscriptionFacilitator) FinalizeExpired(_ context.Context, req *subscription.FinalizeExpiredRequest) (*subscription.TxResultResponse, error) {
	return &subscription.TxResultResponse{SubID: req.SubID}, nil
}
func (c *captureSubscriptionFacilitator) GetSubscription(_ context.Context, _ string) (*subscription.SubscriptionStatus, error) {
	return nil, nil
}
func (c *captureSubscriptionFacilitator) GetCharges(_ context.Context, _ string, _, _ int) (*subscription.ChargesResponse, error) {
	return &subscription.ChargesResponse{}, nil
}
func (c *captureSubscriptionFacilitator) GetPendingChange(_ context.Context, _ string) (*subscription.PendingPlanChange, error) {
	return nil, nil
}

// periodSubscribeHeader builds a base64 PAYMENT-SIGNATURE for a period subscribe
// whose signed terms match the pro plan advertised by periodSubscribeRoute.
func periodSubscribeHeader(t *testing.T) string {
	t.Helper()
	terms := subscription.SubscriptionTerms{
		Merchant:             "0xmerchant",
		Token:                "0xtok",
		AmountPerPeriod:      "5000",
		PeriodSec:            2592000,
		MaxPeriods:           12,
		PlanID:               "pro",
		PlanTier:             2,
		PeriodMode:           subscription.PeriodModeFixed,
		InitialChargePeriods: 1,
		InitialChargeAmount:  "5000",
		StartAt:              0,
		ChangeFromSubID:      "0x0000000000000000000000000000000000000000000000000000000000000000",
	}
	inner := subscription.SubscriptionPayloadInner{
		Terms:                 terms,
		TermsSignature:        "0xtermsig",
		PermitSingleSignature: "0xpermitsig",
	}
	innerJSON, err := json.Marshal(inner)
	if err != nil {
		t.Fatalf("marshal inner: %v", err)
	}
	var innerMap map[string]any
	if err := json.Unmarshal(innerJSON, &innerMap); err != nil {
		t.Fatalf("decode inner map: %v", err)
	}

	payload := x402.PaymentPayload{
		X402Version: 2,
		Payload:     innerMap,
		Accepted: x402.PaymentRequirements{
			Scheme:            subscription.SchemePeriod,
			Network:           "eip155:196",
			Asset:             "0xtok",
			PayTo:             "0xmerchant",
			Amount:            "5000",
			MaxTimeoutSeconds: 300,
		},
	}
	payloadJSON, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	return base64.StdEncoding.EncodeToString(payloadJSON)
}

// periodSubscribeRoutes advertises a single pro plan on POST /subscribe with the
// given per-route SyncSettle setting.
func periodSubscribeRoutes(syncSettle *bool) x402http.RoutesConfig {
	plan := subscription.SubscriptionPlan{
		ID:                   "pro",
		Tier:                 2,
		Network:              "eip155:196",
		PayTo:                "0xmerchant",
		AmountPerPeriod:      "5000",
		PeriodSec:            2592000,
		PeriodMode:           subscription.PeriodModeFixed,
		MaxPeriods:           12,
		InitialChargePeriods: 1,
		InitialChargeAmount:  "5000",
	}
	return x402http.RoutesConfig{
		"POST /subscribe": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  subscription.SchemePeriod,
					Price:   map[string]any{"amount": "5000", "asset": "0xtok"},
					Network: "eip155:196",
					PayTo:   "0xmerchant",
					Extra:   plan.BuildExtra(),
				},
			},
			SyncSettle: syncSettle,
		},
	}
}

// TestPeriodSubscribeSettlesWithRouteSyncSettle verifies the period subscribe
// path derives the settlement mode from the route's SyncSettle: a nil pointer
// stays synchronous (the prior hardcoded behavior), and an explicit value is
// honored.
func TestPeriodSubscribeSettlesWithRouteSyncSettle(t *testing.T) {
	cases := []struct {
		name     string
		route    *bool
		expected bool
	}{
		{"nil defaults to synchronous", nil, true},
		{"explicit sync", boolPtr(true), true},
		{"explicit async", boolPtr(false), false},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			facilitator := &captureSubscriptionFacilitator{}
			support := subscription.NewSupport(facilitator, subscription.AccessWindowSecs)

			periodScheme := subscription.NewPeriodScheme().
				WithFacilitator("0xfacilitator").
				WithSubscriptionContract("0xsubscription").
				WithPermit2Contract("0xpermit2")

			nextCalled := false
			handler := http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				nextCalled = true
				w.WriteHeader(http.StatusOK)
				_ = json.NewEncoder(w).Encode(map[string]string{"data": "granted"})
			})

			middleware := X402Payment(Config{
				Routes:       periodSubscribeRoutes(tc.route),
				Schemes:      []SchemeConfig{{Network: "eip155:196", Server: periodScheme}},
				Subscription: support,
				Timeout:      5 * time.Second,
			})
			wrapped := middleware(handler)

			req := httptest.NewRequest("POST", "/subscribe", nil)
			req.Header.Set("PAYMENT-SIGNATURE", periodSubscribeHeader(t))
			req.Host = "example.com"

			w := httptest.NewRecorder()
			wrapped.ServeHTTP(w, req)

			if w.Code != http.StatusOK {
				t.Fatalf("expected status 200, got %d. Body: %s", w.Code, w.Body.String())
			}
			if !nextCalled {
				t.Error("expected the protected handler to be served after settlement")
			}
			if facilitator.createSyncSettle == nil {
				t.Fatal("expected CreateSubscription to be called")
			}
			if *facilitator.createSyncSettle != tc.expected {
				t.Errorf("expected syncSettle %v, got %v", tc.expected, *facilitator.createSyncSettle)
			}
		})
	}
}

// ============================================================================
// Exempt-payer signature bypass
// ============================================================================

// exemptPaymentHeader builds a base64 PAYMENT-SIGNATURE whose authorization.from
// is the given address, carrying the given scheme.
func exemptPaymentHeader(from, scheme string) string {
	payload := x402.PaymentPayload{
		X402Version: 2,
		Payload: map[string]any{
			"signature":     "0xsig",
			"authorization": map[string]any{"from": from},
		},
		Accepted: x402.PaymentRequirements{
			Scheme:            scheme,
			Network:           "eip155:1",
			Asset:             "USDC",
			Amount:            "1000000",
			PayTo:             "0xtest",
			MaxTimeoutSeconds: 300,
		},
	}
	b, _ := json.Marshal(payload)
	return base64.StdEncoding.EncodeToString(b)
}

// permit2PaymentHeader builds a header whose payer is under permit2Authorization.
func permit2PaymentHeader(from string) string {
	payload := x402.PaymentPayload{
		X402Version: 2,
		Payload: map[string]any{
			"signature":            "0xsig",
			"permit2Authorization": map[string]any{"from": from},
		},
		Accepted: x402.PaymentRequirements{
			Scheme:            "exact",
			Network:           "eip155:1",
			Asset:             "USDC",
			Amount:            "1000000",
			PayTo:             "0xtest",
			MaxTimeoutSeconds: 300,
		},
	}
	b, _ := json.Marshal(payload)
	return base64.StdEncoding.EncodeToString(b)
}

type exemptCounters struct {
	verify    bool
	verifySig bool
	settle    bool
}

// schemeSupportedFunc reports facilitator support for a single scheme.
func schemeSupportedFunc(scheme string) func(ctx context.Context) (x402.SupportedResponse, error) {
	return func(ctx context.Context) (x402.SupportedResponse, error) {
		return x402.SupportedResponse{
			Kinds:      []x402.SupportedKind{{X402Version: 2, Scheme: scheme, Network: "eip155:1"}},
			Extensions: []string{},
			Signers:    make(map[string][]string),
		}, nil
	}
}

// buildExemptMiddleware wires a middleware for the given scheme and exempt list
// with a mock facilitator that records which calls it receives.
func buildExemptMiddleware(t *testing.T, scheme string, exempt []string, sigValid bool, verifyErr error) (func(http.Handler) http.Handler, *exemptCounters) {
	t.Helper()
	calls := &exemptCounters{}
	mockClient := &mockFacilitatorClient{
		verifyFunc: func(context.Context, []byte, []byte) (*x402.VerifyResponse, error) {
			calls.verify = true
			if verifyErr != nil {
				return nil, verifyErr
			}
			return &x402.VerifyResponse{IsValid: true, Payer: "0xpayer"}, nil
		},
		verifySignatureFunc: func(context.Context, []byte, []byte) (*x402.VerifyResponse, error) {
			calls.verifySig = true
			return &x402.VerifyResponse{IsValid: sigValid, Payer: "0xpayer"}, nil
		},
		settleFunc: func(context.Context, []byte, []byte) (*x402.SettleResponse, error) {
			calls.settle = true
			return &x402.SettleResponse{Success: true, Transaction: "0xtx", Network: "eip155:1", Payer: "0xpayer"}, nil
		},
		supportedFunc: schemeSupportedFunc(scheme),
	}

	routes := x402http.RoutesConfig{
		"GET /resource": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{Scheme: scheme, PayTo: "0xtest", Price: "$1.00", Network: "eip155:1"},
			},
		},
	}

	opts := []MiddlewareOption{
		WithFacilitatorClient(mockClient),
		WithScheme("eip155:1", &mockSchemeServer{scheme: scheme}),
		WithSyncFacilitatorOnStart(true),
		WithTimeout(5 * time.Second),
	}
	if len(exempt) > 0 {
		opts = append(opts, WithExemptPayers(exempt))
	}

	return PaymentMiddlewareFromConfig(routes, opts...), calls
}

func serveExempt(mw func(http.Handler) http.Handler, from, scheme string) *httptest.ResponseRecorder {
	handler := http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"data": "resource"})
	})
	req := httptest.NewRequest("GET", "/resource", nil)
	req.Header.Set("PAYMENT-SIGNATURE", exemptPaymentHeader(from, scheme))
	req.Host = "example.com"
	w := httptest.NewRecorder()
	mw(handler).ServeHTTP(w, req)
	return w
}

// An exempt payer with a valid signature is served without a verify or a settle
// call, even when the balance check would otherwise fail (empty review wallet).
func TestExempt_ServedWithoutVerifyOrSettle(t *testing.T) {
	mw, calls := buildExemptMiddleware(t, "exact", []string{"0xReviewer"}, true, fmt.Errorf("insufficient_funds"))
	w := serveExempt(mw, "0xReviewer", "exact")

	if w.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", w.Code, w.Body.String())
	}
	if !calls.verifySig {
		t.Error("expected verify-signature to be called")
	}
	if calls.verify {
		t.Error("expected verify NOT to be called on the exempt path")
	}
	if calls.settle {
		t.Error("expected settle NOT to be called on the exempt path")
	}
}

// A non-exempt payer takes the normal verify + settle path and never hits
// verify-signature.
func TestExempt_NonExemptTakesNormalPath(t *testing.T) {
	mw, calls := buildExemptMiddleware(t, "exact", []string{"0xReviewer"}, true, nil)
	w := serveExempt(mw, "0xSomeoneElse", "exact")

	if w.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", w.Code, w.Body.String())
	}
	if calls.verifySig {
		t.Error("expected verify-signature NOT to be called for a non-exempt payer")
	}
	if !calls.verify || !calls.settle {
		t.Error("expected normal verify + settle for a non-exempt payer")
	}
}

// A listed payer whose signature is invalid is not bypassed; the request falls
// through to the normal paid flow. A forged header therefore cannot self-exempt.
func TestExempt_InvalidSignatureFallsThrough(t *testing.T) {
	mw, calls := buildExemptMiddleware(t, "exact", []string{"0xReviewer"}, false, nil)
	w := serveExempt(mw, "0xReviewer", "exact")

	if !calls.verifySig {
		t.Error("expected verify-signature to be attempted")
	}
	if !calls.verify {
		t.Error("expected fall-through to normal verify when the signature is invalid")
	}
	if w.Code != http.StatusOK {
		t.Fatalf("expected 200 after fall-through, got %d: %s", w.Code, w.Body.String())
	}
}

// An empty exempt list leaves the normal paid flow untouched.
func TestExempt_EmptyListDisablesBypass(t *testing.T) {
	mw, calls := buildExemptMiddleware(t, "exact", nil, true, nil)
	w := serveExempt(mw, "0xReviewer", "exact")

	if calls.verifySig {
		t.Error("expected verify-signature NOT to be called when the list is empty")
	}
	if !calls.verify || !calls.settle {
		t.Error("expected normal verify + settle when the list is empty")
	}
	if w.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", w.Code)
	}
}

// A payer carried under permit2Authorization.from is matched too.
func TestExempt_Permit2Authorization(t *testing.T) {
	mw, calls := buildExemptMiddleware(t, "exact", []string{"0xReviewer"}, true, nil)
	handler := http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"data": "resource"})
	})
	req := httptest.NewRequest("GET", "/resource", nil)
	req.Header.Set("PAYMENT-SIGNATURE", permit2PaymentHeader("0xReviewer"))
	req.Host = "example.com"
	w := httptest.NewRecorder()
	mw(handler).ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", w.Code, w.Body.String())
	}
	if !calls.verifySig || calls.settle {
		t.Errorf("expected exempt bypass for permit2Authorization, got verifySig=%v settle=%v", calls.verifySig, calls.settle)
	}
}

// The address match is case-insensitive.
func TestExempt_CaseInsensitiveMatch(t *testing.T) {
	mw, calls := buildExemptMiddleware(t, "exact", []string{"0xABCdef"}, true, nil)
	w := serveExempt(mw, "0xabcDEF", "exact")

	if w.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", w.Code)
	}
	if !calls.verifySig {
		t.Error("expected verify-signature to be called for a case-insensitive match")
	}
	if calls.settle {
		t.Error("expected no settle for an exempt payer")
	}
}

// The aggr_deferred scheme is in scope for the bypass.
func TestExempt_AggrDeferredSchemeBypassed(t *testing.T) {
	mw, calls := buildExemptMiddleware(t, "aggr_deferred", []string{"0xReviewer"}, true, nil)
	w := serveExempt(mw, "0xReviewer", "aggr_deferred")

	if w.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", w.Code, w.Body.String())
	}
	if !calls.verifySig {
		t.Error("expected verify-signature to be called for aggr_deferred")
	}
	if calls.verify || calls.settle {
		t.Error("expected no verify/settle on the aggr_deferred exempt path")
	}
}

// A scheme outside the eligible set is never bypassed, even for a listed payer
// with an otherwise valid signature; it takes the normal verify + settle path.
func TestExempt_IneligibleSchemeNotBypassed(t *testing.T) {
	mw, calls := buildExemptMiddleware(t, "upto", []string{"0xReviewer"}, true, nil)
	w := serveExempt(mw, "0xReviewer", "upto")

	if w.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d: %s", w.Code, w.Body.String())
	}
	if calls.verifySig {
		t.Error("expected verify-signature NOT to be called for an ineligible scheme")
	}
	if !calls.verify || !calls.settle {
		t.Error("expected normal verify + settle for an ineligible scheme")
	}
}

func TestPaymentMiddlewareFromHTTPServer_SettlesVerifiedPayment(t *testing.T) {
	settleCalled := false

	mockClient := &mockFacilitatorClient{
		verifyFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.VerifyResponse, error) {
			return &x402.VerifyResponse{IsValid: true, Payer: "0xpayer"}, nil
		},
		settleFunc: func(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.SettleResponse, error) {
			settleCalled = true
			return &x402.SettleResponse{
				Success:     true,
				Transaction: "0xtx",
				Network:     "eip155:1",
				Payer:       "0xpayer",
			}, nil
		},
		supportedFunc: defaultSupportedFunc(),
	}
	mockServer := &mockSchemeServer{scheme: "exact"}

	routes := x402http.RoutesConfig{
		"POST /api": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{
					Scheme:  "exact",
					PayTo:   "0xtest",
					Price:   "$1.00",
					Network: "eip155:1",
				},
			},
		},
	}

	resourceServer := x402.Newx402ResourceServer(
		x402.WithFacilitatorClient(mockClient),
	)
	resourceServer.Register("eip155:1", mockServer)

	httpServer := x402http.Wrappedx402HTTPResourceServer(routes, resourceServer)

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_ = json.NewEncoder(w).Encode(map[string]string{"data": "protected-data"})
	})

	middleware := PaymentMiddlewareFromHTTPServer(httpServer,
		WithSyncFacilitatorOnStart(true),
		WithTimeout(5*time.Second),
	)
	wrapped := middleware(handler)

	req := httptest.NewRequest("POST", "/api", nil)
	req.Header.Set("PAYMENT-SIGNATURE", createPaymentHeader("0xtest"))
	req.Host = "example.com"

	w := httptest.NewRecorder()
	wrapped.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Errorf("Expected status 200, got %d. Body: %s", w.Code, w.Body.String())
	}

	if !settleCalled {
		t.Error("Expected settlement to be called")
	}

	if w.Header().Get("PAYMENT-RESPONSE") == "" {
		t.Error("Expected PAYMENT-RESPONSE header")
	}
}
