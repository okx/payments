package protocol

import (
	"encoding/json"
	"testing"
	"time"
)

// ─────────────────────────────────────────────────────────────────────────────
// MethodName
// ─────────────────────────────────────────────────────────────────────────────

func TestMethodName(t *testing.T) {
	t.Run("NewMethodName normalizes to lowercase", func(t *testing.T) {
		m := NewMethodName("TEMPO")
		if m.String() != "tempo" {
			t.Errorf("expected %q, got %q", "tempo", m.String())
		}
	})

	t.Run("IsValid true for lowercase-only", func(t *testing.T) {
		cases := []string{"evm", "bitcoin", "a", "mymethod"}
		for _, c := range cases {
			if !MethodName(c).IsValid() {
				t.Errorf("expected %q to be valid", c)
			}
		}
	})

	t.Run("IsValid false for uppercase or spaces", func(t *testing.T) {
		cases := []string{"EVM", "Has Spaces", "TEMPO", "Mixed123", ""}
		for _, c := range cases {
			if MethodName(c).IsValid() {
				t.Errorf("expected %q to be invalid", c)
			}
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// IntentName
// ─────────────────────────────────────────────────────────────────────────────

func TestIntentName(t *testing.T) {
	t.Run("IsCharge", func(t *testing.T) {
		cases := []struct {
			name     string
			input    string
			wantTrue bool
		}{
			{"charge", "charge", true},
			{"session", "session", false},
			{"other", "other", false},
		}
		for _, tc := range cases {
			t.Run(tc.name, func(t *testing.T) {
				got := IntentName(tc.input).IsCharge()
				if got != tc.wantTrue {
					t.Errorf("IsCharge(%q) = %v, want %v", tc.input, got, tc.wantTrue)
				}
			})
		}
	})

	t.Run("IsSession", func(t *testing.T) {
		cases := []struct {
			name     string
			input    string
			wantTrue bool
		}{
			{"session", "session", true},
			{"charge", "charge", false},
			{"other", "other", false},
		}
		for _, tc := range cases {
			t.Run(tc.name, func(t *testing.T) {
				got := IntentName(tc.input).IsSession()
				if got != tc.wantTrue {
					t.Errorf("IsSession(%q) = %v, want %v", tc.input, got, tc.wantTrue)
				}
			})
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// Base64URL encode / decode
// ─────────────────────────────────────────────────────────────────────────────

func TestBase64URLRoundTrip(t *testing.T) {
	inputs := [][]byte{
		[]byte("hello world"),
		[]byte{0x00, 0xFF, 0xAB},
		[]byte(""),
		[]byte("payload=data&more"),
	}
	for _, data := range inputs {
		encoded := Base64URLEncode(data)
		decoded, err := Base64URLDecode(encoded)
		if err != nil {
			t.Errorf("Base64URLDecode(%q) error: %v", encoded, err)
			continue
		}
		if string(decoded) != string(data) {
			t.Errorf("round-trip mismatch: got %q, want %q", decoded, data)
		}
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Base64URLJson
// ─────────────────────────────────────────────────────────────────────────────

func TestBase64URLJson(t *testing.T) {
	type payload struct {
		Amount   string `json:"amount"`
		Currency string `json:"currency"`
	}

	original := payload{Amount: "1000000", Currency: "USDC"}

	t.Run("NewBase64URLJson round-trip via Decode", func(t *testing.T) {
		b, err := Base64URLJsonFromValue(original)
		if err != nil {
			t.Fatalf("Base64URLJsonFromValue error: %v", err)
		}
		var result payload
		if err := b.Decode(&result); err != nil {
			t.Fatalf("Decode error: %v", err)
		}
		if result != original {
			t.Errorf("got %+v, want %+v", result, original)
		}
	})

	t.Run("NewBase64URLJson from raw string", func(t *testing.T) {
		b, err := Base64URLJsonFromValue(original)
		if err != nil {
			t.Fatalf("Base64URLJsonFromValue error: %v", err)
		}
		b2 := NewBase64URLJson(b.Raw)
		var result payload
		if err := b2.Decode(&result); err != nil {
			t.Fatalf("Decode from raw error: %v", err)
		}
		if result != original {
			t.Errorf("got %+v, want %+v", result, original)
		}
	})

	t.Run("DecodeValue returns raw JSON", func(t *testing.T) {
		b, err := Base64URLJsonFromValue(original)
		if err != nil {
			t.Fatalf("Base64URLJsonFromValue error: %v", err)
		}
		raw, err := b.DecodeValue()
		if err != nil {
			t.Fatalf("DecodeValue error: %v", err)
		}
		var m map[string]interface{}
		if err := json.Unmarshal(raw, &m); err != nil {
			t.Fatalf("unmarshal raw: %v", err)
		}
		if m["amount"] != "1000000" {
			t.Errorf("expected amount 1000000, got %v", m["amount"])
		}
	})

	t.Run("IsEmpty", func(t *testing.T) {
		if !NewBase64URLJson("").IsEmpty() {
			t.Error("empty raw should be empty")
		}
		if NewBase64URLJson("abc").IsEmpty() {
			t.Error("non-empty raw should not be empty")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// ParseUnits
// ─────────────────────────────────────────────────────────────────────────────

func TestParseUnits(t *testing.T) {
	cases := []struct {
		name     string
		amount   string
		decimals uint8
		want     string
		wantErr  bool
	}{
		{"1.5 with 6 decimals", "1.5", 6, "1500000", false},
		{"100 with 18 decimals", "100", 18, "100000000000000000000", false},
		{"0.001 with 3 decimals", "0.001", 3, "1", false},
		{"1 with 0 decimals", "1", 0, "1", false},
		{"0.1 with 2 decimals", "0.1", 2, "10", false},
		{"zero", "0", 6, "0", false},
		{"too many decimals", "1.123456789", 6, "", true},
		{"non-digit chars", "1.2a3", 6, "12a3000", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, err := ParseUnits(tc.amount, tc.decimals)
			if tc.wantErr {
				if err == nil {
					t.Errorf("expected error, got %q", got)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if got != tc.want {
				t.Errorf("ParseUnits(%q, %d) = %q, want %q", tc.amount, tc.decimals, got, tc.want)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ComputeChallengeID
// ─────────────────────────────────────────────────────────────────────────────

func TestComputeChallengeID(t *testing.T) {
	t.Run("same inputs produce same output", func(t *testing.T) {
		a := ComputeChallengeID("key", "realm", "evm", "charge", "req", 0, "", "")
		b := ComputeChallengeID("key", "realm", "evm", "charge", "req", 0, "", "")
		if a != b {
			t.Errorf("expected same output, got %q vs %q", a, b)
		}
	})

	t.Run("different inputs produce different output", func(t *testing.T) {
		a := ComputeChallengeID("key", "realm", "evm", "charge", "req1", 0, "", "")
		b := ComputeChallengeID("key", "realm", "evm", "charge", "req2", 0, "", "")
		if a == b {
			t.Errorf("expected different output for different request, got same: %q", a)
		}
	})

	t.Run("different secret key changes output", func(t *testing.T) {
		a := ComputeChallengeID("key1", "realm", "evm", "charge", "req", 0, "", "")
		b := ComputeChallengeID("key2", "realm", "evm", "charge", "req", 0, "", "")
		if a == b {
			t.Errorf("expected different output for different secret keys, got same: %q", a)
		}
	})

	t.Run("output is non-empty hex string", func(t *testing.T) {
		id := ComputeChallengeID("", "realm", "evm", "charge", "req", 0, "", "")
		if len(id) == 0 {
			t.Error("expected non-empty id")
		}
		// HMAC-SHA256 is 32 bytes = 64 hex chars
		if len(id) != 64 {
			t.Errorf("expected 64 hex chars, got %d: %q", len(id), id)
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// PaymentChallenge
// ─────────────────────────────────────────────────────────────────────────────

func TestPaymentChallenge(t *testing.T) {
	t.Run("NewPaymentChallenge sets fields and computes ID", func(t *testing.T) {
		c := NewPaymentChallenge("https://example.com", "evm", "charge", "encodedReq")
		if c.Realm != "https://example.com" {
			t.Errorf("unexpected realm: %q", c.Realm)
		}
		if c.Method != "evm" {
			t.Errorf("unexpected method: %q", c.Method)
		}
		if c.Intent != "charge" {
			t.Errorf("unexpected intent: %q", c.Intent)
		}
		if c.ID == "" {
			t.Error("expected non-empty ID")
		}
	})

	t.Run("WithExpires recomputes ID", func(t *testing.T) {
		c := NewPaymentChallenge("realm", "evm", "charge", "req")
		oldID := c.ID
		c.WithExpires(9999999999)
		if c.ID == oldID {
			t.Error("expected ID to change after WithExpires")
		}
		if c.Expires != 9999999999 {
			t.Errorf("unexpected Expires: %d", c.Expires)
		}
	})

	t.Run("IsExpired false for zero Expires", func(t *testing.T) {
		c := NewPaymentChallenge("realm", "evm", "charge", "req")
		if c.IsExpired() {
			t.Error("expected not expired when Expires=0")
		}
	})

	t.Run("IsExpired true for past timestamp", func(t *testing.T) {
		c := NewPaymentChallenge("realm", "evm", "charge", "req")
		past := uint64(time.Now().Add(-1 * time.Hour).Unix())
		c.WithExpires(past)
		if !c.IsExpired() {
			t.Error("expected expired for past timestamp")
		}
	})

	t.Run("IsExpired false for future timestamp", func(t *testing.T) {
		c := NewPaymentChallenge("realm", "evm", "charge", "req")
		future := uint64(time.Now().Add(1 * time.Hour).Unix())
		c.WithExpires(future)
		if c.IsExpired() {
			t.Error("expected not expired for future timestamp")
		}
	})

	t.Run("ValidateForCharge success", func(t *testing.T) {
		c := NewPaymentChallenge("realm", "evm", "charge", "req")
		if err := c.ValidateForCharge(); err != nil {
			t.Errorf("unexpected error: %v", err)
		}
	})

	t.Run("ValidateForCharge error on session intent", func(t *testing.T) {
		c := NewPaymentChallenge("realm", "evm", "session", "req")
		if err := c.ValidateForCharge(); err == nil {
			t.Error("expected error for session intent")
		}
	})

	t.Run("ValidateForSession success", func(t *testing.T) {
		c := NewPaymentChallenge("realm", "evm", "session", "req")
		if err := c.ValidateForSession(); err != nil {
			t.Errorf("unexpected error: %v", err)
		}
	})

	t.Run("ValidateForSession error on charge intent", func(t *testing.T) {
		c := NewPaymentChallenge("realm", "evm", "charge", "req")
		if err := c.ValidateForSession(); err == nil {
			t.Error("expected error for charge intent")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// PaymentPayload constructors
// ─────────────────────────────────────────────────────────────────────────────

func TestPaymentPayloadConstructors(t *testing.T) {
	cases := []struct {
		name        string
		constructor func(string) *PaymentPayload
		wantType    PayloadType
	}{
		{"transaction", NewTransactionPayload, PayloadTypeTransaction},
		{"hash", NewHashPayload, PayloadTypeHash},
		{"proof", NewProofPayload, PayloadTypeProof},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			p := tc.constructor(`{"data":"test"}`)
			if p.Type != tc.wantType {
				t.Errorf("expected type %q, got %q", tc.wantType, p.Type)
			}
			if p.Payload != `{"data":"test"}` {
				t.Errorf("expected payload %q, got %q", `{"data":"test"}`, p.Payload)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// PaymentCredential
// ─────────────────────────────────────────────────────────────────────────────

func TestPaymentCredential(t *testing.T) {
	echo := &ChallengeEcho{
		ID:     "testid",
		Realm:  "realm",
		Method: "evm",
		Intent: "charge",
	}
	payload := NewTransactionPayload(`{"type":"transaction"}`)

	t.Run("NewPaymentCredential", func(t *testing.T) {
		cred := NewPaymentCredential(echo, payload)
		if cred.Echo != echo {
			t.Error("echo mismatch")
		}
		if cred.Payload != payload {
			t.Error("payload mismatch")
		}
		if cred.Source != "" {
			t.Errorf("expected empty source, got %q", cred.Source)
		}
	})

	t.Run("NewPaymentCredentialWithSource", func(t *testing.T) {
		cred := NewPaymentCredentialWithSource(echo, "did:example:123", payload)
		if cred.Source != "did:example:123" {
			t.Errorf("expected source %q, got %q", "did:example:123", cred.Source)
		}
		if cred.Echo != echo {
			t.Error("echo mismatch")
		}
		if cred.Payload != payload {
			t.Error("payload mismatch")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// Receipt
// ─────────────────────────────────────────────────────────────────────────────

func TestReceipt(t *testing.T) {
	t.Run("NewSuccessReceipt sets fields", func(t *testing.T) {
		r := NewSuccessReceipt("rcpt-1", NewMethodName("evm"), NewIntentName("charge"), "")
		if r.ID != "rcpt-1" {
			t.Errorf("expected ID %q, got %q", "rcpt-1", r.ID)
		}
		if r.Status != ReceiptStatusSuccess {
			t.Errorf("expected status %q, got %q", ReceiptStatusSuccess, r.Status)
		}
		if r.Method != "evm" {
			t.Errorf("expected method %q, got %q", "evm", r.Method)
		}
		if r.Intent != "charge" {
			t.Errorf("expected intent %q, got %q", "charge", r.Intent)
		}
		if r.Settlement != "" {
			t.Errorf("expected empty settlement, got %q", r.Settlement)
		}
	})

	t.Run("settlement via constructor param", func(t *testing.T) {
		r := NewSuccessReceipt("rcpt-2", NewMethodName("evm"), NewIntentName("charge"), "settlementBlob")
		if r.Settlement != "settlementBlob" {
			t.Errorf("expected settlement %q, got %q", "settlementBlob", r.Settlement)
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// ErrorCode and VerificationError
// ─────────────────────────────────────────────────────────────────────────────

func TestErrorCode(t *testing.T) {
	t.Run("String() returns raw string", func(t *testing.T) {
		cases := []struct {
			code ErrorCode
			want string
		}{
			{ErrorCodeExpired, "expired"},
			{ErrorCodeInvalidAmount, "invalid-amount"},
			{ErrorCodeNetworkError, "network-error"},
			{ErrorCodeTransactionFailed, "transaction-failed"},
		}
		for _, tc := range cases {
			t.Run(tc.want, func(t *testing.T) {
				if tc.code.String() != tc.want {
					t.Errorf("expected %q, got %q", tc.want, tc.code.String())
				}
			})
		}
	})
}

func TestVerificationErrorFactories(t *testing.T) {
	t.Run("VerificationErrorExpired sets correct code", func(t *testing.T) {
		e := VerificationErrorExpired("expired msg")
		if e.Code != ErrorCodeExpired {
			t.Errorf("expected code %q, got %q", ErrorCodeExpired, e.Code)
		}
	})

	t.Run("VerificationErrorInvalidAmount sets correct code", func(t *testing.T) {
		e := VerificationErrorInvalidAmount("bad amount")
		if e.Code != ErrorCodeInvalidAmount {
			t.Errorf("expected code %q, got %q", ErrorCodeInvalidAmount, e.Code)
		}
	})

	t.Run("VerificationErrorNetworkError is retryable", func(t *testing.T) {
		e := VerificationErrorNetworkError("network down")
		if e.Code != ErrorCodeNetworkError {
			t.Errorf("expected code %q, got %q", ErrorCodeNetworkError, e.Code)
		}
		if !e.Retryable {
			t.Error("expected Retryable=true for network error")
		}
	})

	t.Run("VerificationErrorInvalidCredential not retryable", func(t *testing.T) {
		e := VerificationErrorInvalidCredential("bad cred")
		if e.Retryable {
			t.Error("expected Retryable=false for invalid credential")
		}
	})

	t.Run("WithRetryable marks retryable", func(t *testing.T) {
		e := NewVerificationError("some error").WithRetryable()
		if !e.Retryable {
			t.Error("expected Retryable=true after WithRetryable")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// SerializeRequest / DeserializeRequest round-trip
// ─────────────────────────────────────────────────────────────────────────────

func TestSerializeDeserializeRequest(t *testing.T) {
	t.Run("ChargeRequest round-trip", func(t *testing.T) {
		original := ChargeRequest{
			Amount:   "1000000",
			Currency: "USDC",
		}
		encoded, err := SerializeRequest(original)
		if err != nil {
			t.Fatalf("SerializeRequest error: %v", err)
		}
		if encoded == "" {
			t.Fatal("expected non-empty encoded string")
		}

		raw, err := DeserializeRequest(encoded)
		if err != nil {
			t.Fatalf("DeserializeRequest error: %v", err)
		}

		var result ChargeRequest
		if err := json.Unmarshal(raw, &result); err != nil {
			t.Fatalf("unmarshal error: %v", err)
		}
		if result.Amount != original.Amount || result.Currency != original.Currency {
			t.Errorf("round-trip mismatch: got %+v, want %+v", result, original)
		}
	})

	t.Run("DeserializeRequestTyped round-trip", func(t *testing.T) {
		original := SessionRequest{
			Amount:   "500",
			Currency: "ETH",
		}
		encoded, err := SerializeRequest(original)
		if err != nil {
			t.Fatalf("SerializeRequest error: %v", err)
		}

		var result SessionRequest
		if err := DeserializeRequestTyped(encoded, &result); err != nil {
			t.Fatalf("DeserializeRequestTyped error: %v", err)
		}
		if result.Amount != original.Amount || result.Currency != original.Currency {
			t.Errorf("round-trip mismatch: got %+v, want %+v", result, original)
		}
	})
}
