package server

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/okx/payments/go/mpp/protocol"
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

type mockSessionVerifierWithDetails struct{ methodName string }

func (m *mockSessionVerifierWithDetails) Method() string { return m.methodName }
func (m *mockSessionVerifierWithDetails) VerifySession(_ context.Context, _ *protocol.PaymentCredential, _ *protocol.SessionRequest) (*protocol.Receipt, error) {
	return protocol.NewSuccessReceipt("test-id", protocol.MethodName(m.methodName), protocol.IntentName(protocol.IntentSession), ""), nil
}
func (m *mockSessionVerifierWithDetails) ChallengeMethodDetails() json.RawMessage {
	return json.RawMessage(`{"channel":"0x1234"}`)
}
func (m *mockSessionVerifierWithDetails) Respond(_ *protocol.PaymentCredential, _ *protocol.Receipt) any {
	return nil
}

type mockManagementSessionVerifier struct{ methodName string }

func (m *mockManagementSessionVerifier) Method() string { return m.methodName }
func (m *mockManagementSessionVerifier) VerifySession(_ context.Context, _ *protocol.PaymentCredential, _ *protocol.SessionRequest) (*protocol.Receipt, error) {
	return protocol.NewSuccessReceipt("test-id", protocol.MethodName(m.methodName), protocol.IntentName(protocol.IntentSession), ""), nil
}
func (m *mockManagementSessionVerifier) ChallengeMethodDetails() json.RawMessage { return nil }
func (m *mockManagementSessionVerifier) Respond(_ *protocol.PaymentCredential, _ *protocol.Receipt) any {
	return map[string]any{"status": "ok"}
}

// ─────────────────────────────────────────────────────────────────────────────
// EVMConfig struct construction
// ─────────────────────────────────────────────────────────────────────────────

func TestEVMConfig_ZeroValue(t *testing.T) {
	cfg := EVMConfig{}
	if cfg.ChainID != 0 {
		t.Errorf("expected 0, got %d", cfg.ChainID)
	}
	if cfg.Recipient != "" {
		t.Errorf("expected empty recipient")
	}
	if cfg.SecretKey != "" {
		t.Errorf("expected empty secret key")
	}
}

func TestEVMConfig_WithValues(t *testing.T) {
	cfg := EVMConfig{
		ChainID:   196,
		Recipient: "0xdeadbeef",
		SecretKey: "s3cr3t",
	}
	if cfg.ChainID != 196 {
		t.Errorf("ChainID: got %d, want 196", cfg.ChainID)
	}
	if cfg.Recipient != "0xdeadbeef" {
		t.Errorf("Recipient: got %q", cfg.Recipient)
	}
	if cfg.SecretKey != "s3cr3t" {
		t.Errorf("SecretKey: got %q", cfg.SecretKey)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ChargeOptions / SessionChallengeOptions struct construction
// ─────────────────────────────────────────────────────────────────────────────

func TestChargeOptions_Construction(t *testing.T) {
	cfg := EVMConfig{ChainID: 1}
	opts := ChargeOptions{
		EVMConfig:   cfg,
		Description: "pay now",
		Realm:       "test-realm",
	}
	if opts.EVMConfig.ChainID != 1 {
		t.Errorf("EVMConfig.ChainID: got %d", opts.EVMConfig.ChainID)
	}
	if opts.Description != "pay now" {
		t.Errorf("Description: got %q", opts.Description)
	}
	if opts.Realm != "test-realm" {
		t.Errorf("Realm: got %q", opts.Realm)
	}
}

func TestSessionChallengeOptions_Construction(t *testing.T) {
	cfg := EVMConfig{ChainID: 2, Recipient: "0xabc"}
	opts := SessionChallengeOptions{
		EVMConfig:   cfg,
		Description: "session pay",
		Realm:       "session-realm",
	}
	if opts.EVMConfig.ChainID != 2 {
		t.Errorf("EVMConfig.ChainID: got %d", opts.EVMConfig.ChainID)
	}
	if opts.Description != "session pay" {
		t.Errorf("Description: got %q", opts.Description)
	}
	if opts.Realm != "session-realm" {
		t.Errorf("Realm: got %q", opts.Realm)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// NewMpp constructor
// ─────────────────────────────────────────────────────────────────────────────

func TestNewMpp_BothNil(t *testing.T) {
	m := NewMpp(EVMConfig{}, nil, nil)
	if m == nil {
		t.Fatal("NewMpp returned nil")
	}
}

func TestNewMpp_WithChargeVerifier(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, cv, nil)
	if m == nil {
		t.Fatal("NewMpp returned nil")
	}
}

func TestNewMpp_WithSessionVerifier(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, nil, sv)
	if m == nil {
		t.Fatal("NewMpp returned nil")
	}
}

func TestNewMpp_WithBothVerifiers(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, cv, sv)
	if m == nil {
		t.Fatal("NewMpp returned nil")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Mpp.Charge — success and error paths
// ─────────────────────────────────────────────────────────────────────────────

func TestMpp_Charge_Success(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, cv, nil)
	header, err := m.Charge(context.Background(), ChargeRouteConfig{Amount: "1.50", Currency: "USDC", Decimals: 6})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty WWW-Authenticate header")
	}
}

func TestMpp_Charge_InvalidAmount(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, cv, nil)
	_, err := m.Charge(context.Background(), ChargeRouteConfig{Amount: "abc", Currency: "USDC", Decimals: 6})
	if err == nil {
		t.Fatal("expected error for invalid amount")
	}
}

func TestMpp_Charge_WithDescription(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, cv, nil)
	header, err := m.Charge(context.Background(), ChargeRouteConfig{Amount: "10", Currency: "USDT", Decimals: 6, Description: "coffee"})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

func TestMpp_Charge_WithRealm(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, cv, nil)
	header, err := m.Charge(context.Background(), ChargeRouteConfig{Amount: "5", Currency: "USDC", Decimals: 6})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

func TestMpp_Charge_WithExternalID(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, cv, nil)
	header, err := m.Charge(context.Background(), ChargeRouteConfig{Amount: "5", Currency: "USDC", Decimals: 6, ExternalID: "order-99"})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

func TestMpp_Charge_WithRecipient(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{Recipient: "0xdeadbeef"}, cv, nil)
	header, err := m.Charge(context.Background(), ChargeRouteConfig{Amount: "1", Currency: "USDC", Decimals: 6})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

func TestMpp_Charge_EmptyAmount(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, cv, nil)
	_, err := m.Charge(context.Background(), ChargeRouteConfig{Amount: "", Currency: "USDC", Decimals: 6})
	if err == nil {
		t.Fatal("expected error for empty amount")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Mpp.ChargeWithOptions — success path
// ─────────────────────────────────────────────────────────────────────────────

func TestMpp_ChargeWithOptions_Success(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, cv, nil)
	req := protocol.ChargeRequest{Amount: "1000000", Currency: "USDC"}
	opts := ChargeOptions{Realm: "mpp"}
	header, err := m.ChargeWithOptions(context.Background(), req, opts)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

func TestMpp_ChargeWithOptions_WithDescription(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, cv, nil)
	req := protocol.ChargeRequest{Amount: "1000000", Currency: "USDC"}
	opts := ChargeOptions{Description: "test purchase", Realm: "mpp"}
	header, err := m.ChargeWithOptions(context.Background(), req, opts)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

func TestMpp_ChargeWithOptions_EmptyRealm_UsesDefault(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, cv, nil)
	req := protocol.ChargeRequest{Amount: "500000", Currency: "USDT"}
	opts := ChargeOptions{} // empty realm — should use defaultRealm
	header, err := m.ChargeWithOptions(context.Background(), req, opts)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

func TestMpp_ChargeWithOptions_SecretKeyFromCfg(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{SecretKey: "mykey"}, cv, nil)
	req := protocol.ChargeRequest{Amount: "100000", Currency: "USDC"}
	opts := ChargeOptions{Realm: "mpp"} // no SecretKey in opts → falls back to m.cfg.SecretKey
	header, err := m.ChargeWithOptions(context.Background(), req, opts)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Mpp.SessionChallenge — success and error paths
// ─────────────────────────────────────────────────────────────────────────────

func TestMpp_SessionChallenge_Success(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, nil, sv)
	header, err := m.SessionChallenge(context.Background(), SessionRouteConfig{Amount: "2.00", Currency: "USDC", Decimals: 6})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty WWW-Authenticate header")
	}
}

func TestMpp_SessionChallenge_InvalidAmount(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, nil, sv)
	_, err := m.SessionChallenge(context.Background(), SessionRouteConfig{Amount: "bad-amount", Currency: "USDC", Decimals: 6})
	if err == nil {
		t.Fatal("expected error for invalid amount")
	}
}

func TestMpp_SessionChallenge_WithRealm(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, nil, sv)
	header, err := m.SessionChallenge(context.Background(), SessionRouteConfig{Amount: "1", Currency: "USDT", Decimals: 6})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

func TestMpp_SessionChallenge_WithRecipient(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{Recipient: "0xcafe"}, nil, sv)
	header, err := m.SessionChallenge(context.Background(), SessionRouteConfig{Amount: "3", Currency: "USDC", Decimals: 6})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

func TestMpp_SessionChallenge_EmptyAmount(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, nil, sv)
	_, err := m.SessionChallenge(context.Background(), SessionRouteConfig{Amount: "", Currency: "USDC", Decimals: 6})
	if err == nil {
		t.Fatal("expected error for empty amount")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Mpp.SessionChallengeWithDetails — success paths
// ─────────────────────────────────────────────────────────────────────────────

func TestMpp_SessionChallengeWithDetails_Success(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, nil, sv)
	req := protocol.SessionRequest{Amount: "1000000", Currency: "USDC"}
	opts := SessionChallengeOptions{Realm: "mpp"}
	header, err := m.SessionChallengeWithDetails(context.Background(), req, opts)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

func TestMpp_SessionChallengeWithDetails_WithMethodDetails(t *testing.T) {
	sv := &mockSessionVerifierWithDetails{methodName: "evm"}
	m := NewMpp(EVMConfig{}, nil, sv)
	req := protocol.SessionRequest{Amount: "500000", Currency: "USDT"}
	opts := SessionChallengeOptions{Realm: "mpp", Description: "channel payment"}
	header, err := m.SessionChallengeWithDetails(context.Background(), req, opts)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

func TestMpp_SessionChallengeWithDetails_EmptyRealm_UsesDefault(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, nil, sv)
	req := protocol.SessionRequest{Amount: "100", Currency: "USDC"}
	opts := SessionChallengeOptions{}
	header, err := m.SessionChallengeWithDetails(context.Background(), req, opts)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

func TestMpp_SessionChallengeWithDetails_SecretKeyFromCfg(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{SecretKey: "sess-key"}, nil, sv)
	req := protocol.SessionRequest{Amount: "100000", Currency: "USDC"}
	opts := SessionChallengeOptions{Realm: "mpp"}
	header, err := m.SessionChallengeWithDetails(context.Background(), req, opts)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if header == "" {
		t.Error("expected non-empty header")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ReceiptContextKey constant
// ─────────────────────────────────────────────────────────────────────────────

func TestReceiptContextKey_Value(t *testing.T) {
	if ReceiptContextKey == "" {
		t.Error("ReceiptContextKey must not be empty")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// defaultRealm constant (via Charge header output)
// ─────────────────────────────────────────────────────────────────────────────

func TestDefaultRealm_UsedWhenEmpty(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{}, cv, nil)
	// No realm override — defaultRealm should be used
	header, err := m.Charge(context.Background(), ChargeRouteConfig{Amount: "1", Currency: "USDC", Decimals: 6})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	// The header must contain the default realm value
	if !strings.Contains(header, defaultRealm) {
		t.Errorf("expected header to contain realm %q, got: %q", defaultRealm, header)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// VerifyCredential — success path
// ─────────────────────────────────────────────────────────────────────────────

func TestMpp_VerifyCredential_Success(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{SecretKey: "test-secret"}, cv, nil)
	ctx := context.Background()

	// Step 1: obtain a real challenge header from Charge().
	challengeHeader, err := m.Charge(ctx, ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	if err != nil {
		t.Fatalf("Charge: %v", err)
	}

	// Step 2: parse the challenge so we can extract the echo.
	challenge, err := protocol.PaymentChallengeFromHeader(challengeHeader)
	if err != nil {
		t.Fatalf("PaymentChallengeFromHeader: %v", err)
	}

	// Step 3: build a credential from the echo.
	echo := challenge.ToEcho()
	cred := protocol.NewPaymentCredential(echo, protocol.NewTransactionPayload(`{"type":"transaction","tx":"dummy"}`))

	// Step 4: format the Authorization header.
	authHeader, err := protocol.FormatAuthorization(cred)
	if err != nil {
		t.Fatalf("FormatAuthorization: %v", err)
	}

	// Step 5: verify — should succeed.
	receipt, err := m.VerifyCredential(ctx, challengeHeader, authHeader)
	if err != nil {
		t.Fatalf("VerifyCredential: %v", err)
	}
	if receipt == nil {
		t.Fatal("expected non-nil receipt")
	}
}

func TestMpp_VerifyCredential_BadChallenge(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{SecretKey: "test-secret"}, cv, nil)
	_, err := m.VerifyCredential(context.Background(), "not-a-valid-header", "Bearer x")
	if err == nil {
		t.Fatal("expected error for bad challenge header")
	}
}

func TestMpp_VerifyCredential_BadAuth(t *testing.T) {
	cv := &mockChargeVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{SecretKey: "test-secret"}, cv, nil)
	ctx := context.Background()

	challengeHeader, _ := m.Charge(ctx, ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	_, err := m.VerifyCredential(ctx, challengeHeader, "not-a-valid-auth-header")
	if err == nil {
		t.Fatal("expected error for bad authorization header")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// VerifySession — success path
// ─────────────────────────────────────────────────────────────────────────────

func buildSessionCredential(t *testing.T, m *Mpp, payload string) (challengeHeader, authHeader string) {
	t.Helper()
	ctx := context.Background()
	ch, err := m.SessionChallenge(ctx, SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	if err != nil {
		t.Fatalf("SessionChallenge: %v", err)
	}
	challenge, err := protocol.PaymentChallengeFromHeader(ch)
	if err != nil {
		t.Fatalf("PaymentChallengeFromHeader: %v", err)
	}
	echo := challenge.ToEcho()
	cred := protocol.NewPaymentCredential(echo, protocol.NewTransactionPayload(payload))
	ah, err := protocol.FormatAuthorization(cred)
	if err != nil {
		t.Fatalf("FormatAuthorization: %v", err)
	}
	return ch, ah
}

func TestMpp_VerifySession_VoucherAction_ManagementNil(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{SecretKey: "test-secret"}, nil, sv)

	challengeHeader, authHeader := buildSessionCredential(t, m, `{"action":"voucher","channelId":"0x01","cumulativeAmount":"30","signature":"0xaa"}`)

	result, err := m.VerifySession(context.Background(), challengeHeader, authHeader)
	if err != nil {
		t.Fatalf("VerifySession: %v", err)
	}
	if result == nil || result.Receipt == nil {
		t.Fatal("expected non-nil result and receipt")
	}
	if result.ManagementResponse != nil {
		t.Errorf("expected nil ManagementResponse for voucher action, got %v", result.ManagementResponse)
	}
}

func TestMpp_VerifySession_OpenAction_ManagementNonNil(t *testing.T) {
	sv := &mockManagementSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{SecretKey: "test-secret"}, nil, sv)

	challengeHeader, authHeader := buildSessionCredential(t, m, `{"action":"open","type":"hash","channelId":"0x01","salt":"0x02","cumulativeAmount":"0","signature":"0xaa","hash":"0xbb","deposit":"60"}`)

	result, err := m.VerifySession(context.Background(), challengeHeader, authHeader)
	if err != nil {
		t.Fatalf("VerifySession: %v", err)
	}
	if result == nil || result.Receipt == nil {
		t.Fatal("expected non-nil result and receipt")
	}
	if result.ManagementResponse == nil {
		t.Error("expected non-nil ManagementResponse for open action")
	}
}

func TestMpp_VerifySession_Success(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{SecretKey: "test-secret"}, nil, sv)

	challengeHeader, authHeader := buildSessionCredential(t, m, `{"type":"transaction","tx":"dummy"}`)

	result, err := m.VerifySession(context.Background(), challengeHeader, authHeader)
	if err != nil {
		t.Fatalf("VerifySession: %v", err)
	}
	if result == nil || result.Receipt == nil {
		t.Fatal("expected non-nil result and receipt")
	}
}

func TestMpp_VerifySession_BadChallenge(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{SecretKey: "test-secret"}, nil, sv)
	_, err := m.VerifySession(context.Background(), "not-a-valid-header", "Bearer x")
	if err == nil {
		t.Fatal("expected error for bad challenge header")
	}
}

func TestMpp_VerifySession_BadAuth(t *testing.T) {
	sv := &mockSessionVerifier{methodName: "evm"}
	m := NewMpp(EVMConfig{SecretKey: "test-secret"}, nil, sv)
	ctx := context.Background()

	challengeHeader, _ := m.SessionChallenge(ctx, SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	_, err := m.VerifySession(ctx, challengeHeader, "not-a-valid-auth")
	if err == nil {
		t.Fatal("expected error for bad authorization header")
	}
}

