package adapters

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/okx/payments/go/mpp/protocol"
	"github.com/okx/payments/go/mpp/server"
)

// ─────────────────────────────────────────────────────────────────────────────
// Stub verifiers
// ─────────────────────────────────────────────────────────────────────────────

type stubChargeVerifier struct{}

func (s *stubChargeVerifier) Method() string { return "evm" }
func (s *stubChargeVerifier) PrepareRequest(req protocol.ChargeRequest, _ *protocol.PaymentCredential) protocol.ChargeRequest {
	return req
}
func (s *stubChargeVerifier) Verify(_ context.Context, _ *protocol.PaymentCredential, _ *protocol.ChargeRequest) (*protocol.Receipt, error) {
	return &protocol.Receipt{
		ID:     "r1",
		Status: "success",
		Method: "evm",
		Intent: "charge",
	}, nil
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

func newTestMpp() *server.Mpp {
	cfg := server.EVMConfig{
		ChainID:   1,
		Recipient: "0xdeadbeef",
		SecretKey: "test-secret",
	}
	return server.NewMpp(cfg, &stubChargeVerifier{}, nil)
}

func newTestAdapter() *MppAdapter {
	return NewMppAdapter(newTestMpp())
}

// ─────────────────────────────────────────────────────────────────────────────
// Name / Priority
// ─────────────────────────────────────────────────────────────────────────────

func TestMppAdapter_Name(t *testing.T) {
	a := newTestAdapter()
	if got := a.Name(); got != "mpp" {
		t.Errorf("Name() = %q, want %q", got, "mpp")
	}
}

func TestMppAdapter_Priority(t *testing.T) {
	a := newTestAdapter()
	if got := a.Priority(); got != 10 {
		t.Errorf("Priority() = %d, want 10", got)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Detect
// ─────────────────────────────────────────────────────────────────────────────

func TestMppAdapter_Detect_Payment(t *testing.T) {
	a := newTestAdapter()
	r := httptest.NewRequest(http.MethodGet, "/", nil)
	r.Header.Set(protocol.AuthorizationHeader, "Payment echo=\"abc\", payload-type=\"transaction\", payload=\"xyz\"")
	if !a.Detect(r) {
		t.Error("Detect() = false, want true for Payment header")
	}
}

func TestMppAdapter_Detect_NoHeader(t *testing.T) {
	a := newTestAdapter()
	r := httptest.NewRequest(http.MethodGet, "/", nil)
	if a.Detect(r) {
		t.Error("Detect() = true, want false for missing header")
	}
}

func TestMppAdapter_Detect_BearerScheme(t *testing.T) {
	a := newTestAdapter()
	r := httptest.NewRequest(http.MethodGet, "/", nil)
	r.Header.Set(protocol.AuthorizationHeader, "Bearer sometoken")
	if a.Detect(r) {
		t.Error("Detect() = true, want false for Bearer scheme")
	}
}

func TestMppAdapter_Detect_OnlySchemeNoParams(t *testing.T) {
	a := newTestAdapter()
	r := httptest.NewRequest(http.MethodGet, "/", nil)
	r.Header.Set(protocol.AuthorizationHeader, "Payment")
	if !a.Detect(r) {
		t.Error("Detect() = false, want true for Payment-only header")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// GetChallenge
// ─────────────────────────────────────────────────────────────────────────────

func TestMppAdapter_GetChallenge_NilConfig(t *testing.T) {
	a := newTestAdapter()
	r := httptest.NewRequest(http.MethodGet, "/resource", nil)
	h, err := a.GetChallenge(context.Background(), r, nil)
	if err != nil {
		t.Fatalf("GetChallenge(nil) unexpected error: %v", err)
	}
	if h != nil {
		t.Errorf("GetChallenge(nil) expected nil headers, got %v", h)
	}
}

func TestMppAdapter_GetChallenge_ChargeIntent(t *testing.T) {
	a := newTestAdapter()
	r := httptest.NewRequest(http.MethodGet, "/resource", nil)

	cfg := MppRouteConfig{
		Intent:   "charge",
		Amount:   "1.00",
		Currency: "USDC",
		Decimals: 6,
	}

	h, err := a.GetChallenge(context.Background(), r, cfg)
	if err != nil {
		t.Fatalf("GetChallenge(charge) unexpected error: %v", err)
	}
	if h == nil {
		t.Fatal("GetChallenge(charge) returned nil headers")
	}

	wwwAuth := h.Get(protocol.WWWAuthenticateHeader)
	if wwwAuth == "" {
		t.Fatal("WWW-Authenticate header is empty")
	}
	if !strings.HasPrefix(wwwAuth, "Payment") {
		t.Errorf("WWW-Authenticate header should start with Payment, got: %q", wwwAuth)
	}
	if !strings.Contains(wwwAuth, `intent="charge"`) {
		t.Errorf("WWW-Authenticate header should contain intent=charge, got: %q", wwwAuth)
	}
}

func TestMppAdapter_GetChallenge_EmptyIntent(t *testing.T) {
	a := newTestAdapter()
	r := httptest.NewRequest(http.MethodGet, "/resource", nil)

	cfg := MppRouteConfig{Amount: "1.00", Currency: "USDC"}

	h, err := a.GetChallenge(context.Background(), r, cfg)
	if err != nil {
		t.Fatalf("GetChallenge(empty intent) unexpected error: %v", err)
	}
	if h != nil {
		t.Errorf("GetChallenge(empty intent) expected nil headers, got %v", h)
	}
}

func TestMppAdapter_GetChallenge_UnknownIntent(t *testing.T) {
	a := newTestAdapter()
	r := httptest.NewRequest(http.MethodGet, "/resource", nil)

	cfg := MppRouteConfig{Intent: "unknown", Amount: "1.00", Currency: "USDC"}

	_, err := a.GetChallenge(context.Background(), r, cfg)
	if err == nil {
		t.Fatal("GetChallenge(unknown intent) expected error, got nil")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Handle — error cases (no valid Authorization header)
// ─────────────────────────────────────────────────────────────────────────────

func TestMppAdapter_Handle_MissingAuth(t *testing.T) {
	a := newTestAdapter()
	r := httptest.NewRequest(http.MethodGet, "/resource", nil)
	w := httptest.NewRecorder()

	err := a.Handle(w, r, nil)
	if err == nil {
		t.Fatal("Handle with missing auth header expected error, got nil")
	}
	if w.Code != http.StatusUnauthorized {
		t.Errorf("expected status %d, got %d", http.StatusUnauthorized, w.Code)
	}
}

func TestMppAdapter_Handle_InvalidAuthHeader(t *testing.T) {
	a := newTestAdapter()
	r := httptest.NewRequest(http.MethodGet, "/resource", nil)
	r.Header.Set(protocol.AuthorizationHeader, "Payment invalid-garbage")
	w := httptest.NewRecorder()

	err := a.Handle(w, r, nil)
	if err == nil {
		t.Fatal("Handle with invalid auth header expected error, got nil")
	}
	if w.Code != http.StatusBadRequest {
		t.Errorf("expected status %d, got %d", http.StatusBadRequest, w.Code)
	}
}
