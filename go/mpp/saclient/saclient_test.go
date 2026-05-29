package saclient

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	mpperrors "github.com/okx/payments/go/mpp/errors"
)

// ─────────────────────────────────────────────────────────────────────────────
// Test helpers
// ─────────────────────────────────────────────────────────────────────────────

// newMockServer starts a test HTTP server with the provided handler and registers
// its Close method as a cleanup function.
func newMockServer(t *testing.T, handler http.HandlerFunc) *httptest.Server {
	t.Helper()
	srv := httptest.NewServer(handler)
	t.Cleanup(srv.Close)
	return srv
}

// saSuccessBody marshals v into the SA success envelope JSON string.
func saSuccessBody(t *testing.T, v interface{}) string {
	t.Helper()
	data, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("saSuccessBody: marshal data: %v", err)
	}
	env := map[string]interface{}{
		"code": 0,
		"msg":  "OK",
		"data": json.RawMessage(data),
	}
	b, err := json.Marshal(env)
	if err != nil {
		t.Fatalf("saSuccessBody: marshal envelope: %v", err)
	}
	return string(b)
}

// saErrorBody returns an SA error envelope with the given code and message.
func saErrorBody(code int, msg string) string {
	return fmt.Sprintf(`{"code":%d,"msg":%q,"data":null}`, code, msg)
}

// newClientFor creates an OKXSAClient pointing at srv with a fixed API key.
func newClientFor(srv *httptest.Server) *OKXSAClient {
	return NewOKXSAClient(srv.URL, "test-api-key", "", "")
}

// mppCode extracts the MppErrorCode from an error, failing the test if not possible.
func mppCode(t *testing.T, err error) mpperrors.MppErrorCode {
	t.Helper()
	if err == nil {
		t.Fatal("expected an error but got nil")
	}
	mppErr, ok := err.(*mpperrors.MppError)
	if !ok {
		t.Fatalf("expected *mpperrors.MppError, got %T: %v", err, err)
	}
	return mppErr.Code
}

// ─────────────────────────────────────────────────────────────────────────────
// Constructor and options
// ─────────────────────────────────────────────────────────────────────────────

func TestNewSAClient_DefaultHTTPClient(t *testing.T) {
	c := NewOKXSAClient("http://example.com", "key", "", "")
	if c.httpClient == nil {
		t.Fatal("expected non-nil httpClient")
	}
	if c.httpClient != http.DefaultClient {
		t.Error("expected default http.Client")
	}
}

func TestNewSAClient_WithHTTPClient(t *testing.T) {
	custom := &http.Client{}
	c := NewOKXSAClient("http://example.com", "key", "", "", WithHTTPClient(custom))
	if c.httpClient != custom {
		t.Error("WithHTTPClient did not replace the HTTP client")
	}
}

func TestNewSAClient_StoresAllCredentials(t *testing.T) {
	c := NewOKXSAClient("http://host:1234", "my-key", "my-secret", "my-pass")
	if c.baseURL != "http://host:1234" {
		t.Errorf("baseURL: got %q", c.baseURL)
	}
	if c.apiKey != "my-key" {
		t.Errorf("apiKey: got %q", c.apiKey)
	}
	if c.secretKey != "my-secret" {
		t.Errorf("secretKey: got %q", c.secretKey)
	}
	if c.passphrase != "my-pass" {
		t.Errorf("passphrase: got %q", c.passphrase)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Common request behaviour (header injection, path routing)
// ─────────────────────────────────────────────────────────────────────────────

func TestSAClient_SendsAPIKeyHeader(t *testing.T) {
	var capturedKey string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedKey = r.Header.Get("OK-ACCESS-KEY")
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, ChargeReceipt{Reference: "0xabc"}))
	})

	c := newClientFor(srv)
	_, _ = c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})

	if capturedKey != "test-api-key" {
		t.Errorf("OK-ACCESS-KEY header: got %q, want %q", capturedKey, "test-api-key")
	}
}

func TestSAClient_SendsAuthHeaders_WhenSecretKeySet(t *testing.T) {
	var capturedSign, capturedTS, capturedPass string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedSign = r.Header.Get("OK-ACCESS-SIGN")
		capturedTS = r.Header.Get("OK-ACCESS-TIMESTAMP")
		capturedPass = r.Header.Get("OK-ACCESS-PASSPHRASE")
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, ChargeReceipt{Reference: "0xabc"}))
	})

	c := NewOKXSAClient(srv.URL, "key", "secret", "pass")
	_, _ = c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})

	if capturedSign == "" {
		t.Error("OK-ACCESS-SIGN header should be set when secretKey is configured")
	}
	if capturedTS == "" {
		t.Error("OK-ACCESS-TIMESTAMP header should be set when secretKey is configured")
	}
	if capturedPass != "pass" {
		t.Errorf("OK-ACCESS-PASSPHRASE: got %q, want %q", capturedPass, "pass")
	}
}

func TestSAClient_SkipsAuthHeaders_WhenNoSecretKey(t *testing.T) {
	var capturedSign, capturedTS string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedSign = r.Header.Get("OK-ACCESS-SIGN")
		capturedTS = r.Header.Get("OK-ACCESS-TIMESTAMP")
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, ChargeReceipt{Reference: "0xabc"}))
	})

	c := NewOKXSAClient(srv.URL, "key", "", "")
	_, _ = c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})

	if capturedSign != "" {
		t.Error("OK-ACCESS-SIGN should not be set when secretKey is empty")
	}
	if capturedTS != "" {
		t.Error("OK-ACCESS-TIMESTAMP should not be set when secretKey is empty")
	}
}

func TestSAClient_POSTSetsContentTypeJSON(t *testing.T) {
	var capturedCT string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedCT = r.Header.Get("Content-Type")
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, ChargeReceipt{}))
	})

	c := newClientFor(srv)
	_, _ = c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})

	if capturedCT != "application/json" {
		t.Errorf("Content-Type: got %q, want %q", capturedCT, "application/json")
	}
}

func TestSAClient_GETDoesNotSetContentType(t *testing.T) {
	var capturedCT string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedCT = r.Header.Get("Content-Type")
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, SessionStatus{ChannelID: "ch-1"}))
	})

	c := newClientFor(srv)
	_, _ = c.SessionStatus(context.Background(), "ch-1")

	if capturedCT != "" {
		t.Errorf("GET should not set Content-Type, got %q", capturedCT)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Charge endpoints
// ─────────────────────────────────────────────────────────────────────────────

func TestSettle_CorrectPath(t *testing.T) {
	var capturedPath string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedPath = r.URL.Path
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, ChargeReceipt{Reference: "0x1"}))
	})

	c := newClientFor(srv)
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if err != nil {
		t.Fatalf("Settle: %v", err)
	}
	if capturedPath != pathChargeSettle {
		t.Errorf("path: got %q, want %q", capturedPath, pathChargeSettle)
	}
}

func TestSettle_ReturnsChargeReceipt(t *testing.T) {
	receipt := ChargeReceipt{Reference: "0xdeadbeef", ChainID: 196, ChallengeID: "cid-42"}
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, receipt))
	})

	c := newClientFor(srv)
	got, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if err != nil {
		t.Fatalf("Settle: %v", err)
	}
	if got.Reference != receipt.Reference {
		t.Errorf("Reference: got %q, want %q", got.Reference, receipt.Reference)
	}
	if got.ChainID != receipt.ChainID {
		t.Errorf("ChainID: got %d, want %d", got.ChainID, receipt.ChainID)
	}
	if got.ChallengeID != receipt.ChallengeID {
		t.Errorf("ChallengeID: got %q, want %q", got.ChallengeID, receipt.ChallengeID)
	}
}

func TestVerifyHash_CorrectPath(t *testing.T) {
	var capturedPath string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedPath = r.URL.Path
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, ChargeReceipt{}))
	})

	c := newClientFor(srv)
	_, err := c.VerifyHash(context.Background(), &ChargeVerifyHashRequest{Payload: ChargeHashPayload{Type: "hash", Hash: "0x1"}})
	if err != nil {
		t.Fatalf("VerifyHash: %v", err)
	}
	if capturedPath != pathChargeVerifyHash {
		t.Errorf("path: got %q, want %q", capturedPath, pathChargeVerifyHash)
	}
}

func TestVerifyHash_ReturnsChargeReceipt(t *testing.T) {
	receipt := ChargeReceipt{Reference: "0xaabbcc", ChainID: 1, Timestamp: "2024-01-01T00:00:00Z"}
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, receipt))
	})

	c := newClientFor(srv)
	got, err := c.VerifyHash(context.Background(), &ChargeVerifyHashRequest{Payload: ChargeHashPayload{Type: "hash", Hash: "0xaabbcc"}})
	if err != nil {
		t.Fatalf("VerifyHash: %v", err)
	}
	if got.Reference != receipt.Reference {
		t.Errorf("Reference: got %q, want %q", got.Reference, receipt.Reference)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Session endpoints
// ─────────────────────────────────────────────────────────────────────────────

func TestSessionOpen_CorrectPath(t *testing.T) {
	var capturedPath string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedPath = r.URL.Path
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, SessionReceipt{ChannelID: "ch-1"}))
	})

	c := newClientFor(srv)
	_, err := c.SessionOpen(context.Background(), &SessionOpenRequest{Payload: SessionOpenPayload{Action: "open", Type: "transaction"}})
	if err != nil {
		t.Fatalf("SessionOpen: %v", err)
	}
	if capturedPath != pathSessionOpen {
		t.Errorf("path: got %q, want %q", capturedPath, pathSessionOpen)
	}
}

func TestSessionOpen_ReturnsSessionReceipt(t *testing.T) {
	receipt := SessionReceipt{ChannelID: "ch-open-1", ChainID: 196, Timestamp: "ts"}
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, receipt))
	})

	c := newClientFor(srv)
	got, err := c.SessionOpen(context.Background(), &SessionOpenRequest{Payload: SessionOpenPayload{Action: "open", Type: "transaction"}})
	if err != nil {
		t.Fatalf("SessionOpen: %v", err)
	}
	if got.ChannelID != receipt.ChannelID {
		t.Errorf("ChannelID: got %q, want %q", got.ChannelID, receipt.ChannelID)
	}
}

func TestSessionTopUp_CorrectPath(t *testing.T) {
	var capturedPath string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedPath = r.URL.Path
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, SessionReceipt{}))
	})

	c := newClientFor(srv)
	_, err := c.SessionTopUp(context.Background(), &SessionTopUpRequest{Payload: SessionTopUpPayload{Action: "topUp", Type: "transaction", ChannelID: "ch-1"}})
	if err != nil {
		t.Fatalf("SessionTopUp: %v", err)
	}
	if capturedPath != pathSessionTopUp {
		t.Errorf("path: got %q, want %q", capturedPath, pathSessionTopUp)
	}
}

func TestSessionSettle_CorrectPath(t *testing.T) {
	var capturedPath string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedPath = r.URL.Path
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, SessionReceipt{}))
	})

	c := newClientFor(srv)
	_, err := c.SessionSettle(context.Background(), &SessionSettleRequest{Payload: SessionSettlePayload{ChannelID: "ch-1"}})
	if err != nil {
		t.Fatalf("SessionSettle: %v", err)
	}
	if capturedPath != pathSessionSettle {
		t.Errorf("path: got %q, want %q", capturedPath, pathSessionSettle)
	}
}

func TestSessionClose_CorrectPath(t *testing.T) {
	var capturedPath string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedPath = r.URL.Path
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, SessionReceipt{}))
	})

	c := newClientFor(srv)
	_, err := c.SessionClose(context.Background(), &SessionCloseRequest{Payload: SessionClosePayload{ChannelID: "ch-1"}})
	if err != nil {
		t.Fatalf("SessionClose: %v", err)
	}
	if capturedPath != pathSessionClose {
		t.Errorf("path: got %q, want %q", capturedPath, pathSessionClose)
	}
}

func TestSessionClose_ReturnsSessionReceipt(t *testing.T) {
	receipt := SessionReceipt{ChannelID: "ch-close-2", ChainID: 196}
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, receipt))
	})

	c := newClientFor(srv)
	got, err := c.SessionClose(context.Background(), &SessionCloseRequest{Payload: SessionClosePayload{ChannelID: "ch-close-2"}})
	if err != nil {
		t.Fatalf("SessionClose: %v", err)
	}
	if got.ChannelID != receipt.ChannelID {
		t.Errorf("ChannelID: got %q, want %q", got.ChannelID, receipt.ChannelID)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// SessionStatus (GET)
// ─────────────────────────────────────────────────────────────────────────────

func TestSessionStatus_CorrectPathAndQueryParam(t *testing.T) {
	var capturedPath, capturedChannelID string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedPath = r.URL.Path
		capturedChannelID = r.URL.Query().Get("channelId")
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, SessionStatus{ChannelID: "ch-status-1"}))
	})

	c := newClientFor(srv)
	_, err := c.SessionStatus(context.Background(), "ch-status-1")
	if err != nil {
		t.Fatalf("SessionStatus: %v", err)
	}
	if capturedPath != pathSessionStatus {
		t.Errorf("path: got %q, want %q", capturedPath, pathSessionStatus)
	}
	if capturedChannelID != "ch-status-1" {
		t.Errorf("channelId query param: got %q, want %q", capturedChannelID, "ch-status-1")
	}
}

func TestSessionStatus_UsesGETMethod(t *testing.T) {
	var capturedMethod string
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		capturedMethod = r.Method
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, SessionStatus{ChannelID: "x"}))
	})

	c := newClientFor(srv)
	_, _ = c.SessionStatus(context.Background(), "x")

	if capturedMethod != http.MethodGet {
		t.Errorf("expected GET, got %q", capturedMethod)
	}
}

func TestSessionStatus_ReturnsSessionStatus(t *testing.T) {
	status := SessionStatus{
		ChannelID:        "ch-stat-42",
		Payer:            "0xpayer",
		Payee:            "0xpayee",
		Token:            "0xtoken",
		Deposit:          "1000000",
		CumulativeAmount: "500000",
		SettledOnChain:   "200000",
		RemainingBalance: "500000",
		SessionStatus:    "OPEN",
	}
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, saSuccessBody(t, status))
	})

	c := newClientFor(srv)
	got, err := c.SessionStatus(context.Background(), "ch-stat-42")
	if err != nil {
		t.Fatalf("SessionStatus: %v", err)
	}
	if got.ChannelID != status.ChannelID {
		t.Errorf("ChannelID: got %q", got.ChannelID)
	}
	if got.SessionStatus != "OPEN" {
		t.Errorf("SessionStatus: got %q", got.SessionStatus)
	}
	if got.RemainingBalance != status.RemainingBalance {
		t.Errorf("RemainingBalance: got %q", got.RemainingBalance)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// HTTP-level error handling
// ─────────────────────────────────────────────────────────────────────────────

func TestSAClient_HTTP500_ReturnsCodeHttp(t *testing.T) {
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		http.Error(w, "internal server error", http.StatusInternalServerError)
	})

	c := newClientFor(srv)
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeHttp {
		t.Errorf("expected CodeHttp for HTTP 500, got %v", err)
	}
}

func TestSAClient_HTTP404_ReturnsCodeHttp(t *testing.T) {
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		http.Error(w, "not found", http.StatusNotFound)
	})

	c := newClientFor(srv)
	_, err := c.SessionStatus(context.Background(), "ch-x")
	if mppCode(t, err) != mpperrors.CodeHttp {
		t.Errorf("expected CodeHttp for HTTP 404, got %v", err)
	}
}

func TestSAClient_InvalidJSONResponse_ReturnsCodeJson(t *testing.T) {
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		fmt.Fprint(w, "not-json")
	})

	c := newClientFor(srv)
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeJson {
		t.Errorf("expected CodeJson for invalid JSON body, got %v", err)
	}
}

func TestSAClient_InvalidDataJSON_ReturnsCodeJson(t *testing.T) {
	// Valid envelope but data field is not a valid ChargeReceipt JSON.
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		// data is a JSON string that can't be unmarshalled into ChargeReceipt.
		fmt.Fprint(w, `{"code":0,"msg":"ok","data":"not-an-object"}`)
	})

	c := newClientFor(srv)
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeJson {
		t.Errorf("expected CodeJson for invalid data field, got %v", err)
	}
}

func TestSAClient_UnreachableHost_ReturnsCodeHttp(t *testing.T) {
	c := NewOKXSAClient("http://127.0.0.1:1", "key", "", "")
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeHttp {
		t.Errorf("expected CodeHttp for unreachable host, got %v", err)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// SA business error code mapping
// ─────────────────────────────────────────────────────────────────────────────

func saErrServer(t *testing.T, saCode int) *httptest.Server {
	return newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		fmt.Fprint(w, saErrorBody(saCode, "error"))
	})
}

func TestMapSAError_InvalidSignature(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodeInvalidSignature)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeInvalidSignature {
		t.Errorf("expected CodeInvalidSignature, got %v", err)
	}
}

func TestMapSAError_InsufficientBalance(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodeInsufficientBalance)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeInsufficientBalance {
		t.Errorf("expected CodeInsufficientBalance, got %v", err)
	}
}

func TestMapSAError_AmountExceedsDeposit(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodeAmountExceedsDeposit)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeAmountExceedsDeposit {
		t.Errorf("expected CodeAmountExceedsDeposit, got %v", err)
	}
}

func TestMapSAError_ChannelNotFound(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodeChannelNotFound)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeChannelNotFound {
		t.Errorf("expected CodeChannelNotFound, got %v", err)
	}
}

func TestMapSAError_ChannelClosed(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodeChannelClosed)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeChannelClosed {
		t.Errorf("expected CodeChannelClosed, got %v", err)
	}
}

func TestMapSAError_ChannelClosing(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodeChannelClosing)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeChannelClosed {
		t.Errorf("expected CodeChannelClosed for SACodeChannelClosing, got %v", err)
	}
}

func TestMapSAError_DeltaTooSmall(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodeDeltaTooSmall)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeDeltaTooSmall {
		t.Errorf("expected CodeDeltaTooSmall, got %v", err)
	}
}

func TestMapSAError_DeltaBelowMinimum(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodeDeltaBelowMinimum)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeDeltaTooSmall {
		t.Errorf("expected CodeDeltaTooSmall for SACodeDeltaBelowMinimum, got %v", err)
	}
}

func TestMapSAError_SignerMismatch(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodeSignerMismatch)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeSignerMismatch {
		t.Errorf("expected CodeSignerMismatch, got %v", err)
	}
}

func TestMapSAError_PayerBlocked(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodePayerBlocked)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeMalformedCredential {
		t.Errorf("expected CodeMalformedCredential for SACodePayerBlocked, got %v", err)
	}
}

func TestMapSAError_InvalidCredential(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodeInvalidCredential)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeMalformedCredential {
		t.Errorf("expected CodeMalformedCredential for SACodeInvalidCredential, got %v", err)
	}
}

func TestMapSAError_InternalError(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodeInternalError)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeInternal {
		t.Errorf("expected CodeInternal for SACodeInternalError, got %v", err)
	}
}

func TestMapSAError_UnsupportedChain(t *testing.T) {
	c := newClientFor(saErrServer(t, int(SACodeUnsupportedChain)))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeInternal {
		t.Errorf("expected CodeInternal for SACodeUnsupportedChain, got %v", err)
	}
}

func TestMapSAError_UnknownCode_FallsBackToCodeInternal(t *testing.T) {
	c := newClientFor(saErrServer(t, 99999))
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if mppCode(t, err) != mpperrors.CodeInternal {
		t.Errorf("expected CodeInternal for unknown SA code, got %v", err)
	}
}

func TestMapSAError_ErrorMessagePreserved(t *testing.T) {
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		fmt.Fprint(w, saErrorBody(int(SACodeChannelNotFound), "channel does not exist"))
	})

	c := newClientFor(srv)
	_, err := c.Settle(context.Background(), &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "channel does not exist") {
		t.Errorf("expected original SA message in error, got: %v", err)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Context cancellation
// ─────────────────────────────────────────────────────────────────────────────

func TestSAClient_CancelledContext_ReturnsError(t *testing.T) {
	srv := newMockServer(t, func(w http.ResponseWriter, r *http.Request) {
		// Handler that the request will never reach because context is cancelled.
		w.WriteHeader(http.StatusOK)
		fmt.Fprint(w, saSuccessBody(t, ChargeReceipt{}))
	})

	ctx, cancel := context.WithCancel(context.Background())
	cancel() // cancel immediately

	c := newClientFor(srv)
	_, err := c.Settle(ctx, &ChargeSettleRequest{Payload: ChargeTransactionPayload{Type: "transaction"}})
	if err == nil {
		t.Fatal("expected error for cancelled context")
	}
}
