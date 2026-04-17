package http

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func newTestOKXClient(t *testing.T, serverURL string) *OKXFacilitatorClient {
	t.Helper()
	client, err := NewOKXFacilitatorClient(&OKXFacilitatorConfig{
		Auth: OKXAuthConfig{
			APIKey:     "test-key",
			SecretKey:  "test-secret",
			Passphrase: "test-passphrase",
		},
		BaseURL: serverURL,
	})
	if err != nil {
		t.Fatalf("failed to create test client: %v", err)
	}
	return client
}

func TestOKXFacilitatorClient_GetSupported(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v6/pay/x402/supported" {
			t.Errorf("unexpected path: %s", r.URL.Path)
		}
		if r.Method != "GET" {
			t.Errorf("unexpected method: %s", r.Method)
		}

		if r.Header.Get("OK-ACCESS-KEY") == "" {
			t.Error("missing OK-ACCESS-KEY header")
		}
		if r.Header.Get("OK-ACCESS-SIGN") == "" {
			t.Error("missing OK-ACCESS-SIGN header")
		}

		resp := `{
			"code": 0,
			"msg": "",
			"data": {
				"kinds": [
					{"x402Version": 2, "scheme": "exact", "network": "eip155:196"},
					{"x402Version": 2, "scheme": "exact", "network": "eip155:8453"}
				],
				"extensions": [],
				"signers": {}
			}
		}`
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(resp))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	supported, err := client.GetSupported(context.Background())
	if err != nil {
		t.Fatalf("GetSupported failed: %v", err)
	}

	if len(supported.Kinds) != 2 {
		t.Fatalf("expected 2 kinds, got %d", len(supported.Kinds))
	}

	if supported.Kinds[0].Network != "eip155:196" {
		t.Errorf("expected network eip155:196, got %s", supported.Kinds[0].Network)
	}
	if supported.Kinds[1].Network != "eip155:8453" {
		t.Errorf("expected network eip155:8453, got %s", supported.Kinds[1].Network)
	}

	for i, kind := range supported.Kinds {
		if kind.X402Version != 2 {
			t.Errorf("kind[%d]: expected x402Version=2, got %d", i, kind.X402Version)
		}
	}

	if supported.Kinds[0].Scheme != "exact" {
		t.Errorf("expected scheme exact, got %s", supported.Kinds[0].Scheme)
	}
}

func TestOKXFacilitatorClient_Verify(t *testing.T) {
	var receivedBody map[string]interface{}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v6/pay/x402/verify" {
			t.Errorf("unexpected path: %s", r.URL.Path)
		}
		if r.Method != "POST" {
			t.Errorf("unexpected method: %s", r.Method)
		}

		body, _ := io.ReadAll(r.Body)
		json.Unmarshal(body, &receivedBody)

		resp := `{
			"code": 0,
			"msg": "",
			"data": {
				"isValid": true,
				"payer": "0xabc123"
			}
		}`
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(resp))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)

	payload := `{"x402Version":2,"scheme":"exact","network":"eip155:196","amount":"1000000"}`
	requirements := `{"scheme":"exact","network":"eip155:196","asset":"0xUSDC","amount":"1000000","payTo":"0xmerchant","maxTimeoutSeconds":30}`

	resp, err := client.Verify(context.Background(), []byte(payload), []byte(requirements))
	if err != nil {
		t.Fatalf("Verify failed: %v", err)
	}

	if !resp.IsValid {
		t.Error("expected isValid=true")
	}
	if resp.Payer != "0xabc123" {
		t.Errorf("expected payer 0xabc123, got %s", resp.Payer)
	}

	// Verify request body format: x402Version=2
	if v, ok := receivedBody["x402Version"].(float64); !ok || int(v) != 2 {
		t.Errorf("expected x402Version=2 in request body, got %v", receivedBody["x402Version"])
	}

	// Verify paymentRequirements is passed through as-is (should have network and amount, not chainIndex/maxAmountRequired)
	reqs, ok := receivedBody["paymentRequirements"].(map[string]interface{})
	if !ok {
		t.Fatal("paymentRequirements not found in request body")
	}
	if reqs["network"] != "eip155:196" {
		t.Errorf("expected network=eip155:196 in request body, got %v", reqs["network"])
	}
	if reqs["amount"] != "1000000" {
		t.Errorf("expected amount=1000000 in request body, got %v", reqs["amount"])
	}
	// Should NOT have chainIndex or maxAmountRequired in request body
	if _, exists := reqs["chainIndex"]; exists {
		t.Error("request body should not have 'chainIndex' field, should use 'network'")
	}
	if _, exists := reqs["maxAmountRequired"]; exists {
		t.Error("request body should not have 'maxAmountRequired' field, should use 'amount'")
	}
}

func TestOKXFacilitatorClient_Settle(t *testing.T) {
	var receivedBody map[string]interface{}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v6/pay/x402/settle" {
			t.Errorf("unexpected path: %s", r.URL.Path)
		}
		if r.Method != "POST" {
			t.Errorf("unexpected method: %s", r.Method)
		}

		body, _ := io.ReadAll(r.Body)
		json.Unmarshal(body, &receivedBody)

		resp := `{
			"code": 0,
			"msg": "",
			"data": {
				"success": true,
				"transaction": "0xtx123",
				"network": "eip155:196",
				"payer": "0xpayer"
			}
		}`
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(resp))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)

	payload := `{"x402Version":2,"scheme":"exact","network":"eip155:196","amount":"1000000"}`
	requirements := `{"scheme":"exact","network":"eip155:196","asset":"0xUSDC","amount":"1000000","payTo":"0xmerchant","maxTimeoutSeconds":30}`

	resp, err := client.Settle(context.Background(), []byte(payload), []byte(requirements))
	if err != nil {
		t.Fatalf("Settle failed: %v", err)
	}

	if resp.Transaction != "0xtx123" {
		t.Errorf("expected transaction 0xtx123, got %s", resp.Transaction)
	}
	if string(resp.Network) != "eip155:196" {
		t.Errorf("expected network eip155:196, got %s", resp.Network)
	}
	if !resp.Success {
		t.Error("expected success=true")
	}
	if resp.Payer != "0xpayer" {
		t.Errorf("expected payer 0xpayer, got %s", resp.Payer)
	}

	// Verify syncSettle is in the body (default true)
	if ss, ok := receivedBody["syncSettle"].(bool); !ok || !ss {
		t.Errorf("expected syncSettle=true in body, got %v", receivedBody["syncSettle"])
	}
}

func TestOKXFacilitatorClient_SettleAsync(t *testing.T) {
	var receivedBody map[string]interface{}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		json.Unmarshal(body, &receivedBody)

		resp := `{
			"code": 0,
			"msg": "",
			"data": {
				"success": true,
				"transaction": "0xtxasync",
				"network": "eip155:196",
				"payer": "0xpayer"
			}
		}`
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(resp))
	}))
	defer server.Close()

	syncSettleFalse := false
	client, err := NewOKXFacilitatorClient(&OKXFacilitatorConfig{
		Auth: OKXAuthConfig{
			APIKey:     "test-key",
			SecretKey:  "test-secret",
			Passphrase: "test-passphrase",
		},
		BaseURL:    server.URL,
		SyncSettle: &syncSettleFalse,
	})
	if err != nil {
		t.Fatalf("failed to create client: %v", err)
	}

	payload := `{"x402Version":2,"scheme":"exact","network":"eip155:196","amount":"1000000"}`
	requirements := `{"scheme":"exact","network":"eip155:196","asset":"0xUSDC","amount":"1000000","payTo":"0xmerchant","maxTimeoutSeconds":30}`

	_, err = client.Settle(context.Background(), []byte(payload), []byte(requirements))
	if err != nil {
		t.Fatalf("Settle async failed: %v", err)
	}

	// Verify syncSettle=false is in the body
	if ss, ok := receivedBody["syncSettle"].(bool); !ok || ss {
		t.Errorf("expected syncSettle=false in body, got %v", receivedBody["syncSettle"])
	}
}

func TestOKXFacilitatorClient_ErrorEnvelope(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		resp := `{
			"code": 50103,
			"msg": "Invalid API key",
			"error_code": "50103",
			"error_message": "Invalid API key",
			"data": {}
		}`
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(resp))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	_, err := client.GetSupported(context.Background())
	if err == nil {
		t.Fatal("expected error for code=50103")
	}

	errMsg := err.Error()
	if !containsStr(errMsg, "50103") {
		t.Errorf("error should contain code 50103, got: %s", errMsg)
	}
	if !containsStr(errMsg, "Invalid API key") {
		t.Errorf("error should contain message, got: %s", errMsg)
	}
}

func TestOKXFacilitatorClient_MissingCredentials(t *testing.T) {
	// Nil config
	_, err := NewOKXFacilitatorClient(nil)
	if err == nil {
		t.Fatal("expected error for nil config")
	}

	// Missing API key
	_, err = NewOKXFacilitatorClient(&OKXFacilitatorConfig{
		Auth: OKXAuthConfig{
			SecretKey:  "secret",
			Passphrase: "pass",
		},
	})
	if err == nil {
		t.Fatal("expected error for missing API key")
	}

	// Missing secret key
	_, err = NewOKXFacilitatorClient(&OKXFacilitatorConfig{
		Auth: OKXAuthConfig{
			APIKey:     "key",
			Passphrase: "pass",
		},
	})
	if err == nil {
		t.Fatal("expected error for missing secret key")
	}

	// Missing passphrase
	_, err = NewOKXFacilitatorClient(&OKXFacilitatorConfig{
		Auth: OKXAuthConfig{
			APIKey:    "key",
			SecretKey: "secret",
		},
	})
	if err == nil {
		t.Fatal("expected error for missing passphrase")
	}
}

func TestOKXFacilitatorClient_VerifyInvalidSignature(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		resp := `{
			"code": 0,
			"data": {
				"isValid": false,
				"invalidReason": "signature mismatch"
			}
		}`
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(resp))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	payload := `{"x402Version":2,"scheme":"exact","network":"eip155:196","amount":"1000"}`
	requirements := `{"scheme":"exact","network":"eip155:196","asset":"0xUSDC","amount":"1000","payTo":"0xmerchant","maxTimeoutSeconds":30}`

	resp, err := client.Verify(context.Background(), []byte(payload), []byte(requirements))
	if err != nil {
		t.Fatalf("Verify should not return error for invalid signature: %v", err)
	}
	if resp.IsValid {
		t.Error("expected isValid=false")
	}
	if resp.InvalidReason != "signature mismatch" {
		t.Errorf("expected invalidReason='signature mismatch', got '%s'", resp.InvalidReason)
	}
}

func TestOKXFacilitatorClient_SettleInsufficientBalance(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		resp := `{
			"code": 0,
			"data": {
				"success": false,
				"transaction": "",
				"errorReason": "insufficient balance",
				"payer": "0xpayer"
			}
		}`
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(resp))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	payload := `{"x402Version":2,"scheme":"exact","network":"eip155:196","amount":"1000"}`
	requirements := `{"scheme":"exact","network":"eip155:196","asset":"0xUSDC","amount":"1000","payTo":"0xmerchant","maxTimeoutSeconds":30}`

	resp, err := client.Settle(context.Background(), []byte(payload), []byte(requirements))
	if err != nil {
		t.Fatalf("Settle should not return error for insufficient balance: %v", err)
	}
	if resp.Success {
		t.Error("expected success=false")
	}
	if resp.ErrorReason != "insufficient balance" {
		t.Errorf("expected errorReason='insufficient balance', got '%s'", resp.ErrorReason)
	}
}

func TestOKXFacilitatorClient_APITimeout(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		<-r.Context().Done()
	}))
	defer server.Close()

	client, err := NewOKXFacilitatorClient(&OKXFacilitatorConfig{
		Auth: OKXAuthConfig{
			APIKey:     "test-key",
			SecretKey:  "test-secret",
			Passphrase: "test-passphrase",
		},
		BaseURL: server.URL,
		Timeout: 50 * time.Millisecond,
	})
	if err != nil {
		t.Fatalf("failed to create client: %v", err)
	}

	_, err = client.GetSupported(context.Background())
	if err == nil {
		t.Fatal("expected timeout error")
	}
	if !containsStr(err.Error(), "/supported") {
		t.Errorf("timeout error should mention endpoint, got: %s", err.Error())
	}
}

func containsStr(s, substr string) bool {
	if len(substr) == 0 {
		return true
	}
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
