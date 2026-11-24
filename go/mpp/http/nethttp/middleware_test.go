package nethttp_test

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/okx/payments/go/mpp/protocol"
	"github.com/okx/payments/go/mpp/server"

	nethttpmw "github.com/okx/payments/go/mpp/http/nethttp"
)

// ─────────────────────────────────────────────────────────────────────────────
// Mock verifier implementations
// ─────────────────────────────────────────────────────────────────────────────

type mockChargeVerifier struct{ methodName string }

func (m *mockChargeVerifier) Method() string { return m.methodName }
func (m *mockChargeVerifier) PrepareRequest(req protocol.ChargeRequest, _ *protocol.PaymentCredential) protocol.ChargeRequest {
	return req
}
func (m *mockChargeVerifier) Verify(_ context.Context, _ *protocol.PaymentCredential, _ *protocol.ChargeRequest) (*protocol.Receipt, error) {
	return protocol.NewSuccessReceipt("test-id", protocol.MethodName(m.methodName), protocol.IntentName(protocol.IntentCharge), ""), nil
}

type mockSessionVerifier struct{ methodName string }

func (m *mockSessionVerifier) Method() string { return m.methodName }
func (m *mockSessionVerifier) VerifySession(_ context.Context, _ *protocol.PaymentCredential, _ *protocol.SessionRequest) (*protocol.Receipt, error) {
	return protocol.NewSuccessReceipt("test-id", protocol.MethodName(m.methodName), protocol.IntentName(protocol.IntentSession), ""), nil
}
func (m *mockSessionVerifier) ChallengeMethodDetails() json.RawMessage { return nil }
func (m *mockSessionVerifier) Respond(_ *protocol.PaymentCredential, _ *protocol.Receipt) any {
	return nil
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

func buildChargeAuthHeader(t *testing.T, m *server.Mpp) (challengeHeader, authHeader string) {
	t.Helper()
	ctx := context.Background()
	ch, err := m.Charge(ctx, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	if err != nil {
		t.Fatalf("Charge: %v", err)
	}
	challenge, err := protocol.PaymentChallengeFromHeader(ch)
	if err != nil {
		t.Fatalf("PaymentChallengeFromHeader: %v", err)
	}
	echo := challenge.ToEcho()
	cred := protocol.NewPaymentCredential(echo, protocol.NewTransactionPayload(`{"type":"transaction","tx":"dummy"}`))
	ah, err := protocol.FormatAuthorization(cred)
	if err != nil {
		t.Fatalf("FormatAuthorization: %v", err)
	}
	return ch, ah
}

func buildSessionAuthHeader(t *testing.T, m *server.Mpp) (challengeHeader, authHeader string) {
	t.Helper()
	ctx := context.Background()
	ch, err := m.SessionChallenge(ctx, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	if err != nil {
		t.Fatalf("SessionChallenge: %v", err)
	}
	challenge, err := protocol.PaymentChallengeFromHeader(ch)
	if err != nil {
		t.Fatalf("PaymentChallengeFromHeader: %v", err)
	}
	echo := challenge.ToEcho()
	cred := protocol.NewPaymentCredential(echo, protocol.NewTransactionPayload(`{"type":"transaction","tx":"dummy"}`))
	ah, err := protocol.FormatAuthorization(cred)
	if err != nil {
		t.Fatalf("FormatAuthorization: %v", err)
	}
	return ch, ah
}

// ─────────────────────────────────────────────────────────────────────────────
// ChargeMiddleware tests
// ─────────────────────────────────────────────────────────────────────────────

func TestChargeMiddleware_NoAuth_Returns402(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{}, cv, nil)

	mw := nethttpmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("expected 402, got %d", w.Code)
	}
	if w.Header().Get(protocol.WWWAuthenticateHeader) == "" {
		t.Error("expected WWW-Authenticate header to be set")
	}
}

func TestChargeMiddleware_InvalidAuth_Returns400(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{}, cv, nil)

	mw := nethttpmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, "Bearer notvalid")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if w.Code != http.StatusBadRequest {
		t.Errorf("expected 400, got %d", w.Code)
	}
}

func TestChargeMiddleware_NilVerifier_Returns500(t *testing.T) {
	m := server.NewMpp(server.EVMConfig{}, nil, nil)

	mw := nethttpmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if w.Code != http.StatusInternalServerError {
		t.Errorf("expected 500, got %d", w.Code)
	}
}

func TestChargeMiddleware_WithOptions_NoAuth_Returns402(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{}, cv, nil)

	mw := nethttpmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "5.00", Currency: "USDT", Decimals: 6, Description: "premium access"})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("expected 402, got %d", w.Code)
	}
}

func TestChargeMiddleware_WithValidAuth_CallsNext(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{SecretKey: "test-secret"}, cv, nil)

	_, authHeader := buildChargeAuthHeader(t, m)

	nextCalled := false
	mw := nethttpmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		nextCalled = true
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, authHeader)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if !nextCalled {
		t.Error("expected next handler to be called on valid auth")
	}
	if w.Code != http.StatusOK {
		t.Errorf("expected 200, got %d", w.Code)
	}
	if w.Header().Get(protocol.PaymentReceiptHeader) == "" {
		t.Error("expected Payment-Receipt header to be set")
	}
}

func TestChargeMiddleware_WithInvalidAuth_Returns4xx(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{SecretKey: "test-secret"}, cv, nil)

	mw := nethttpmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, `PaymentAuth echo="bad", payload-type="transaction", payload="dummy"`)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if w.Code != http.StatusBadRequest && w.Code != http.StatusPaymentRequired {
		t.Errorf("expected 400 or 402 for bad auth, got %d", w.Code)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// SessionMiddleware tests
// ─────────────────────────────────────────────────────────────────────────────

func TestSessionMiddleware_NoAuth_Returns402(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{}, nil, sv)

	mw := nethttpmw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/session-pay", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("expected 402, got %d", w.Code)
	}
	if w.Header().Get(protocol.WWWAuthenticateHeader) == "" {
		t.Error("expected WWW-Authenticate header to be set")
	}
}

func TestSessionMiddleware_InvalidAuth_Returns400(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{}, nil, sv)

	mw := nethttpmw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/session-pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, "Bearer notvalid")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if w.Code != http.StatusBadRequest {
		t.Errorf("expected 400, got %d", w.Code)
	}
}

func TestSessionMiddleware_NilVerifier_Returns500(t *testing.T) {
	m := server.NewMpp(server.EVMConfig{}, nil, nil)

	mw := nethttpmw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/session-pay", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if w.Code != http.StatusInternalServerError {
		t.Errorf("expected 500, got %d", w.Code)
	}
}

func TestSessionMiddleware_WithOptions_NoAuth_Returns402(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{}, nil, sv)

	mw := nethttpmw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "2.50", Currency: "USDC", Decimals: 6, Description: "channel session"})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/session-pay", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("expected 402, got %d", w.Code)
	}
}

func TestSessionMiddleware_WithValidAuth_CallsNext(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{SecretKey: "test-secret"}, nil, sv)

	_, authHeader := buildSessionAuthHeader(t, m)

	nextCalled := false
	mw := nethttpmw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		nextCalled = true
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, authHeader)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if !nextCalled {
		t.Error("expected next handler to be called on valid session auth")
	}
	if w.Code != http.StatusOK {
		t.Errorf("expected 200, got %d", w.Code)
	}
	if w.Header().Get(protocol.PaymentReceiptHeader) == "" {
		t.Error("expected Payment-Receipt header to be set")
	}
}

func TestSessionMiddleware_WithInvalidAuth_Returns4xx(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{SecretKey: "test-secret"}, nil, sv)

	mw := nethttpmw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, `PaymentAuth echo="bad", payload-type="transaction", payload="dummy"`)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if w.Code != http.StatusBadRequest && w.Code != http.StatusPaymentRequired {
		t.Errorf("expected 400 or 402 for bad session auth, got %d", w.Code)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// GetReceipt tests
// ─────────────────────────────────────────────────────────────────────────────

func TestGetReceipt_WithReceipt(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{SecretKey: "test-secret"}, cv, nil)

	_, authHeader := buildChargeAuthHeader(t, m)

	var capturedReq *http.Request
	mw := nethttpmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		capturedReq = r
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, authHeader)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	if capturedReq == nil {
		t.Fatal("next handler was not called")
	}

	receipt := nethttpmw.GetReceipt(capturedReq)
	if receipt == nil {
		t.Fatal("expected non-nil receipt")
	}
	if receipt.ID != "test-id" {
		t.Errorf("Receipt.ID: got %q, want %q", receipt.ID, "test-id")
	}
}

func TestGetReceipt_NoReceipt(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	got := nethttpmw.GetReceipt(req)
	if got != nil {
		t.Errorf("expected nil receipt, got %+v", got)
	}
}
