package echo_test

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/labstack/echo/v4"
	"github.com/okx/payments/go/mpp/protocol"
	"github.com/okx/payments/go/mpp/server"

	echomw "github.com/okx/payments/go/mpp/http/echo"
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

func newEchoContext(method, path string) (echo.Context, *httptest.ResponseRecorder) {
	e := echo.New()
	req := httptest.NewRequest(method, path, nil)
	w := httptest.NewRecorder()
	return e.NewContext(req, w), w
}

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
	echoVal := challenge.ToEcho()
	cred := protocol.NewPaymentCredential(echoVal, protocol.NewTransactionPayload(`{"type":"transaction","tx":"dummy"}`))
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
	echoVal := challenge.ToEcho()
	cred := protocol.NewPaymentCredential(echoVal, protocol.NewTransactionPayload(`{"type":"transaction","tx":"dummy"}`))
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

	mwFunc := echomw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mwFunc(func(c echo.Context) error {
		return c.NoContent(http.StatusOK)
	})

	c, w := newEchoContext(http.MethodGet, "/pay")
	_ = handler(c)

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

	mwFunc := echomw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mwFunc(func(c echo.Context) error {
		return c.NoContent(http.StatusOK)
	})

	c, w := newEchoContext(http.MethodGet, "/pay")
	c.Request().Header.Set(protocol.AuthorizationHeader, "Bearer notvalid")
	_ = handler(c)

	if w.Code != http.StatusBadRequest {
		t.Errorf("expected 400, got %d", w.Code)
	}
}

func TestChargeMiddleware_NilVerifier_Returns500(t *testing.T) {
	m := server.NewMpp(server.EVMConfig{}, nil, nil)

	mwFunc := echomw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mwFunc(func(c echo.Context) error {
		return c.NoContent(http.StatusOK)
	})

	c, w := newEchoContext(http.MethodGet, "/pay")
	_ = handler(c)

	if w.Code != http.StatusInternalServerError {
		t.Errorf("expected 500, got %d", w.Code)
	}
}

func TestChargeMiddleware_WithOptions_NoAuth_Returns402(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{}, cv, nil)

	mwFunc := echomw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "5.00", Currency: "USDT", Decimals: 6, Description: "premium access"})
	handler := mwFunc(func(c echo.Context) error {
		return c.NoContent(http.StatusOK)
	})

	c, w := newEchoContext(http.MethodGet, "/pay")
	_ = handler(c)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("expected 402, got %d", w.Code)
	}
}

func TestChargeMiddleware_WithValidAuth_CallsNext(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{SecretKey: "test-secret"}, cv, nil)

	_, authHeader := buildChargeAuthHeader(t, m)

	nextCalled := false
	mwFunc := echomw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mwFunc(func(c echo.Context) error {
		nextCalled = true
		return c.NoContent(http.StatusOK)
	})

	e := echo.New()
	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, authHeader)
	w := httptest.NewRecorder()
	c := e.NewContext(req, w)
	_ = handler(c)

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

	mwFunc := echomw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mwFunc(func(c echo.Context) error {
		return c.NoContent(http.StatusOK)
	})

	e := echo.New()
	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, `PaymentAuth echo="bad", payload-type="transaction", payload="dummy"`)
	w := httptest.NewRecorder()
	c := e.NewContext(req, w)
	_ = handler(c)

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

	mwFunc := echomw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mwFunc(func(c echo.Context) error {
		return c.NoContent(http.StatusOK)
	})

	c, w := newEchoContext(http.MethodGet, "/session-pay")
	_ = handler(c)

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

	mwFunc := echomw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mwFunc(func(c echo.Context) error {
		return c.NoContent(http.StatusOK)
	})

	c, w := newEchoContext(http.MethodGet, "/session-pay")
	c.Request().Header.Set(protocol.AuthorizationHeader, "Bearer notvalid")
	_ = handler(c)

	if w.Code != http.StatusBadRequest {
		t.Errorf("expected 400, got %d", w.Code)
	}
}

func TestSessionMiddleware_NilVerifier_Returns500(t *testing.T) {
	m := server.NewMpp(server.EVMConfig{}, nil, nil)

	mwFunc := echomw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mwFunc(func(c echo.Context) error {
		return c.NoContent(http.StatusOK)
	})

	c, w := newEchoContext(http.MethodGet, "/session-pay")
	_ = handler(c)

	if w.Code != http.StatusInternalServerError {
		t.Errorf("expected 500, got %d", w.Code)
	}
}

func TestSessionMiddleware_WithOptions_NoAuth_Returns402(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{}, nil, sv)

	mwFunc := echomw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "2.50", Currency: "USDC", Decimals: 6, Description: "channel session"})
	handler := mwFunc(func(c echo.Context) error {
		return c.NoContent(http.StatusOK)
	})

	c, w := newEchoContext(http.MethodGet, "/session-pay")
	_ = handler(c)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("expected 402, got %d", w.Code)
	}
}

func TestSessionMiddleware_WithValidAuth_CallsNext(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{SecretKey: "test-secret"}, nil, sv)

	_, authHeader := buildSessionAuthHeader(t, m)

	nextCalled := false
	mwFunc := echomw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mwFunc(func(c echo.Context) error {
		nextCalled = true
		return c.NoContent(http.StatusOK)
	})

	e := echo.New()
	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, authHeader)
	w := httptest.NewRecorder()
	c := e.NewContext(req, w)
	_ = handler(c)

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

	mwFunc := echomw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mwFunc(func(c echo.Context) error {
		return c.NoContent(http.StatusOK)
	})

	e := echo.New()
	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, `PaymentAuth echo="bad", payload-type="transaction", payload="dummy"`)
	w := httptest.NewRecorder()
	c := e.NewContext(req, w)
	_ = handler(c)

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

	var capturedCtx echo.Context
	mwFunc := echomw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	handler := mwFunc(func(c echo.Context) error {
		capturedCtx = c
		return c.NoContent(http.StatusOK)
	})

	e := echo.New()
	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, authHeader)
	w := httptest.NewRecorder()
	c := e.NewContext(req, w)
	_ = handler(c)

	if capturedCtx == nil {
		t.Fatal("next handler was not called")
	}

	receipt := echomw.GetReceipt(capturedCtx)
	if receipt == nil {
		t.Fatal("expected non-nil receipt")
	}
	if receipt.ID != "test-id" {
		t.Errorf("Receipt.ID: got %q, want %q", receipt.ID, "test-id")
	}
}

func TestGetReceipt_NoReceipt(t *testing.T) {
	e := echo.New()
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	w := httptest.NewRecorder()
	c := e.NewContext(req, w)

	got := echomw.GetReceipt(c)
	if got != nil {
		t.Errorf("expected nil receipt, got %+v", got)
	}
}

func TestGetReceipt_WrongType(t *testing.T) {
	e := echo.New()
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	w := httptest.NewRecorder()
	c := e.NewContext(req, w)
	c.Set("mpp_payment_receipt", "not-a-receipt")

	got := echomw.GetReceipt(c)
	if got != nil {
		t.Errorf("expected nil when wrong type stored, got %+v", got)
	}
}
