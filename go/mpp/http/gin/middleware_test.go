package gin_test

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/okx/payments/go/mpp/protocol"
	"github.com/okx/payments/go/mpp/server"

	ginmw "github.com/okx/payments/go/mpp/http/gin"
)

// ─────────────────────────────────────────────────────────────────────────────
// Mock verifier implementations (local to this package)
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

func newGinContext(method, path string) (*gin.Context, *httptest.ResponseRecorder) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	req, _ := http.NewRequest(method, path, nil)
	c.Request = req
	return c, w
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
	handler := ginmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})

	c, w := newGinContext(http.MethodGet, "/pay")
	handler(c)

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
	handler := ginmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})

	c, w := newGinContext(http.MethodGet, "/pay")
	c.Request.Header.Set(protocol.AuthorizationHeader, "Bearer notvalid")
	handler(c)

	if w.Code != http.StatusBadRequest {
		t.Errorf("expected 400, got %d", w.Code)
	}
}

func TestChargeMiddleware_NilVerifier_Returns500(t *testing.T) {
	m := server.NewMpp(server.EVMConfig{}, nil, nil)
	handler := ginmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})

	c, w := newGinContext(http.MethodGet, "/pay")
	handler(c)

	if w.Code != http.StatusInternalServerError {
		t.Errorf("expected 500, got %d", w.Code)
	}
}

func TestChargeMiddleware_WithOptions_NoAuth_Returns402(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{}, cv, nil)
	handler := ginmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "5.00", Currency: "USDT", Decimals: 6, Description: "premium access"})

	c, w := newGinContext(http.MethodGet, "/pay")
	handler(c)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("expected 402, got %d", w.Code)
	}
}

func TestChargeMiddleware_WithValidAuth_CallsNext(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{SecretKey: "test-secret"}, cv, nil)

	_, authHeader := buildChargeAuthHeader(t, m)

	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	_, engine := gin.CreateTestContext(w)

	nextCalled := false
	engine.GET("/pay", ginmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6}), func(c *gin.Context) {
		nextCalled = true
		c.Status(http.StatusOK)
	})

	req, _ := http.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, authHeader)

	engine.ServeHTTP(w, req)

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

	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	_, engine := gin.CreateTestContext(w)

	engine.GET("/pay", ginmw.ChargeMiddleware(m, server.ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6}))

	req, _ := http.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, "Payment !!!invalid-base64!!!")

	engine.ServeHTTP(w, req)

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
	handler := ginmw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})

	c, w := newGinContext(http.MethodGet, "/session-pay")
	handler(c)

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
	handler := ginmw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})

	c, w := newGinContext(http.MethodGet, "/session-pay")
	c.Request.Header.Set(protocol.AuthorizationHeader, "Bearer notvalid")
	handler(c)

	if w.Code != http.StatusBadRequest {
		t.Errorf("expected 400, got %d", w.Code)
	}
}

func TestSessionMiddleware_NilVerifier_Returns500(t *testing.T) {
	m := server.NewMpp(server.EVMConfig{}, nil, nil)
	handler := ginmw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})

	c, w := newGinContext(http.MethodGet, "/session-pay")
	handler(c)

	if w.Code != http.StatusInternalServerError {
		t.Errorf("expected 500, got %d", w.Code)
	}
}

func TestSessionMiddleware_WithOptions_NoAuth_Returns402(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{}, nil, sv)
	handler := ginmw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "2.50", Currency: "USDC", Decimals: 6, Description: "channel session"})

	c, w := newGinContext(http.MethodGet, "/session-pay")
	handler(c)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("expected 402, got %d", w.Code)
	}
}

func TestSessionMiddleware_WithValidAuth_CallsNext(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := server.NewMpp(server.EVMConfig{SecretKey: "test-secret"}, nil, sv)

	_, authHeader := buildSessionAuthHeader(t, m)

	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	_, engine := gin.CreateTestContext(w)

	nextCalled := false
	engine.GET("/pay", ginmw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6}), func(c *gin.Context) {
		nextCalled = true
		c.Status(http.StatusOK)
	})

	req, _ := http.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, authHeader)

	engine.ServeHTTP(w, req)

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

	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	_, engine := gin.CreateTestContext(w)

	engine.GET("/pay", ginmw.SessionMiddleware(m, server.SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6}))

	req, _ := http.NewRequest(http.MethodGet, "/pay", nil)
	req.Header.Set(protocol.AuthorizationHeader, "Payment !!!invalid-base64!!!")

	engine.ServeHTTP(w, req)

	if w.Code != http.StatusBadRequest && w.Code != http.StatusPaymentRequired {
		t.Errorf("expected 400 or 402 for bad session auth, got %d", w.Code)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// GetReceipt tests
// ─────────────────────────────────────────────────────────────────────────────

func TestGetReceipt_WithReceipt(t *testing.T) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	req, _ := http.NewRequest(http.MethodGet, "/", nil)
	c.Request = req

	receipt := protocol.NewSuccessReceipt("rcpt-1", "evm", protocol.IntentName(protocol.IntentCharge), "")
	c.Set(server.ReceiptContextKey, receipt)

	got := ginmw.GetReceipt(c)
	if got == nil {
		t.Fatal("expected non-nil receipt")
	}
	if got.ID != "rcpt-1" {
		t.Errorf("Receipt.ID: got %q, want %q", got.ID, "rcpt-1")
	}
}

func TestGetReceipt_NoReceipt(t *testing.T) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	req, _ := http.NewRequest(http.MethodGet, "/", nil)
	c.Request = req

	got := ginmw.GetReceipt(c)
	if got != nil {
		t.Errorf("expected nil receipt, got %+v", got)
	}
}

func TestGetReceipt_WrongType(t *testing.T) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	req, _ := http.NewRequest(http.MethodGet, "/", nil)
	c.Request = req

	c.Set(server.ReceiptContextKey, "not-a-receipt")

	got := ginmw.GetReceipt(c)
	if got != nil {
		t.Errorf("expected nil when wrong type stored, got %+v", got)
	}
}
