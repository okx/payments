package protocol

import (
	"encoding/json"
	"testing"
	"time"
)

// ─────────────────────────────────────────────────────────────────────────────
// DetectPaymentProtocol
// ─────────────────────────────────────────────────────────────────────────────

func TestDetectPaymentProtocol(t *testing.T) {
	cases := []struct {
		name      string
		header    string
		wantProto PaymentProtocol
		wantFound bool
	}{
		{"contains Payment", "Payment realm=\"x\"", PaymentProtocolWebPaymentAuth, true},
		{"bare Payment", "Payment", PaymentProtocolWebPaymentAuth, true},
		{"empty string", "", 0, false},
		{"different scheme", "Bearer token123", 0, false},
		{"Basic scheme", "Basic dXNlcjpwYXNz", 0, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			proto, found := DetectPaymentProtocol(tc.header)
			if found != tc.wantFound {
				t.Errorf("found=%v, want %v", found, tc.wantFound)
			}
			if proto != tc.wantProto {
				t.Errorf("proto=%v, want %v", proto, tc.wantProto)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ExtractPaymentScheme
// ─────────────────────────────────────────────────────────────────────────────

func TestExtractPaymentScheme(t *testing.T) {
	cases := []struct {
		name      string
		header    string
		wantSch   string
		wantMatch bool
	}{
		{"Payment with params", `Payment realm="x"`, "Payment", true},
		{"Payment only", "Payment", "Payment", true},
		{"Bearer", "Bearer token", "Bearer", false},
		{"tab separated", "Payment\trealm=\"x\"", "Payment", true},
		{"empty", "", "", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			sch, match := ExtractPaymentScheme(tc.header)
			if sch != tc.wantSch {
				t.Errorf("scheme=%q, want %q", sch, tc.wantSch)
			}
			if match != tc.wantMatch {
				t.Errorf("match=%v, want %v", match, tc.wantMatch)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// parseParams (tested indirectly via ParseWWWAuthenticate)
// ─────────────────────────────────────────────────────────────────────────────

func TestParseParamsViaWWWAuthenticate(t *testing.T) {
	// Quoted values with various whitespace.
	header := `Payment realm="https://example.com", method="evm", intent="charge", request="abc123", id="id-1"`
	c, err := ParseWWWAuthenticate(header)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if c.Realm != "https://example.com" {
		t.Errorf("realm=%q", c.Realm)
	}
	if c.Method != "evm" {
		t.Errorf("method=%q", c.Method)
	}
	if c.Intent != "charge" {
		t.Errorf("intent=%q", c.Intent)
	}
	if c.Request != "abc123" {
		t.Errorf("request=%q", c.Request)
	}
	if c.ID != "id-1" {
		t.Errorf("id=%q", c.ID)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ParseWWWAuthenticate / FormatWWWAuthenticate round-trip
// ─────────────────────────────────────────────────────────────────────────────

func TestWWWAuthenticateRoundTrip(t *testing.T) {
	t.Run("basic round-trip", func(t *testing.T) {
		orig := NewPaymentChallenge("https://shop.example.com", "evm", "charge", "encdReq")
		header, err := FormatWWWAuthenticate(orig)
		if err != nil {
			t.Fatalf("FormatWWWAuthenticate: %v", err)
		}
		parsed, err := ParseWWWAuthenticate(header)
		if err != nil {
			t.Fatalf("ParseWWWAuthenticate: %v", err)
		}
		if parsed.Realm != orig.Realm {
			t.Errorf("realm mismatch: got %q, want %q", parsed.Realm, orig.Realm)
		}
		if parsed.Method != orig.Method {
			t.Errorf("method mismatch: got %q, want %q", parsed.Method, orig.Method)
		}
		if parsed.Intent != orig.Intent {
			t.Errorf("intent mismatch: got %q, want %q", parsed.Intent, orig.Intent)
		}
		if parsed.Request != orig.Request {
			t.Errorf("request mismatch: got %q, want %q", parsed.Request, orig.Request)
		}
		if parsed.ID != orig.ID {
			t.Errorf("id mismatch: got %q, want %q", parsed.ID, orig.ID)
		}
	})

	t.Run("with optional fields round-trip", func(t *testing.T) {
		orig := NewPaymentChallenge("realm", "bitcoin", "session", "rq")
		orig.WithExpires(9999999999).
			WithDescription("pay for coffee").
			WithDigest("sha256-abc").
			WithOpaque("nonce42")

		header, err := orig.ToHeader()
		if err != nil {
			t.Fatalf("ToHeader: %v", err)
		}
		parsed, err := ParseWWWAuthenticate(header)
		if err != nil {
			t.Fatalf("ParseWWWAuthenticate: %v", err)
		}
		if parsed.Expires != orig.Expires {
			t.Errorf("expires: got %d, want %d", parsed.Expires, orig.Expires)
		}
		if parsed.Description != orig.Description {
			t.Errorf("description: got %q, want %q", parsed.Description, orig.Description)
		}
		if parsed.Digest != orig.Digest {
			t.Errorf("digest: got %q, want %q", parsed.Digest, orig.Digest)
		}
		if parsed.Opaque != orig.Opaque {
			t.Errorf("opaque: got %q, want %q", parsed.Opaque, orig.Opaque)
		}
	})

	t.Run("ParseWWWAuthenticate error on wrong scheme", func(t *testing.T) {
		_, err := ParseWWWAuthenticate("Bearer token")
		if err == nil {
			t.Error("expected error for non-PaymentAuth scheme")
		}
	})

	t.Run("ParseWWWAuthenticate error on missing required param", func(t *testing.T) {
		// Missing "request" param.
		_, err := ParseWWWAuthenticate(`PaymentAuth realm="x", method="evm", intent="charge", id="1"`)
		if err == nil {
			t.Error("expected error for missing required param")
		}
	})

	t.Run("FormatWWWAuthenticate error on empty realm", func(t *testing.T) {
		c := &PaymentChallenge{Method: "evm", Intent: "charge", Request: "r", ID: "i"}
		_, err := FormatWWWAuthenticate(c)
		if err == nil {
			t.Error("expected error for empty realm")
		}
	})

	t.Run("FormatWWWAuthenticate error on empty method", func(t *testing.T) {
		c := &PaymentChallenge{Realm: "r", Intent: "charge", Request: "r", ID: "i"}
		_, err := FormatWWWAuthenticate(c)
		if err == nil {
			t.Error("expected error for empty method")
		}
	})

	t.Run("FormatWWWAuthenticate error on empty intent", func(t *testing.T) {
		c := &PaymentChallenge{Realm: "r", Method: "evm", Request: "r", ID: "i"}
		_, err := FormatWWWAuthenticate(c)
		if err == nil {
			t.Error("expected error for empty intent")
		}
	})

	t.Run("FormatWWWAuthenticate error on empty request", func(t *testing.T) {
		c := &PaymentChallenge{Realm: "r", Method: "evm", Intent: "charge", ID: "i"}
		_, err := FormatWWWAuthenticate(c)
		if err == nil {
			t.Error("expected error for empty request")
		}
	})

	t.Run("FormatWWWAuthenticate error on empty id", func(t *testing.T) {
		c := &PaymentChallenge{Realm: "r", Method: "evm", Intent: "charge", Request: "r"}
		_, err := FormatWWWAuthenticate(c)
		if err == nil {
			t.Error("expected error for empty id")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// ParseWWWAuthenticateAll / FormatWWWAuthenticateMany
// ─────────────────────────────────────────────────────────────────────────────

func TestWWWAuthenticateBatch(t *testing.T) {
	c1 := NewPaymentChallenge("realm1", "evm", "charge", "req1")
	c2 := NewPaymentChallenge("realm2", "bitcoin", "session", "req2")

	headers, err := FormatWWWAuthenticateMany([]*PaymentChallenge{c1, c2})
	if err != nil {
		t.Fatalf("FormatWWWAuthenticateMany: %v", err)
	}
	if len(headers) != 2 {
		t.Fatalf("expected 2 headers, got %d", len(headers))
	}

	parsed, err := ParseWWWAuthenticateAll(headers)
	if err != nil {
		t.Fatalf("ParseWWWAuthenticateAll: %v", err)
	}
	if len(parsed) != 2 {
		t.Fatalf("expected 2 challenges, got %d", len(parsed))
	}
	if parsed[0].Realm != "realm1" {
		t.Errorf("parsed[0].Realm=%q", parsed[0].Realm)
	}
	if parsed[1].Realm != "realm2" {
		t.Errorf("parsed[1].Realm=%q", parsed[1].Realm)
	}

	t.Run("ParseWWWAuthenticateAll error propagates", func(t *testing.T) {
		_, err := ParseWWWAuthenticateAll([]string{"Bearer token"})
		if err == nil {
			t.Error("expected error")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// Authorization (PaymentCredential) round-trip
// ─────────────────────────────────────────────────────────────────────────────

func TestAuthorizationRoundTrip(t *testing.T) {
	t.Run("basic round-trip", func(t *testing.T) {
		echo := &ChallengeEcho{
			ID:     "echo-id",
			Realm:  "https://shop.example.com",
			Method: "evm",
			Intent: "charge",
		}
		payload := NewTransactionPayload(`{"type":"transaction","tx":"0xdeadbeef"}`)
		cred := NewPaymentCredential(echo, payload)

		header, err := FormatAuthorization(cred)
		if err != nil {
			t.Fatalf("FormatAuthorization: %v", err)
		}

		parsed, err := ParseAuthorization(header)
		if err != nil {
			t.Fatalf("ParseAuthorization: %v", err)
		}
		if parsed.Echo == nil {
			t.Fatal("parsed echo is nil")
		}
		if parsed.Echo.ID != echo.ID {
			t.Errorf("echo.ID: got %q, want %q", parsed.Echo.ID, echo.ID)
		}
		if parsed.Echo.Realm != echo.Realm {
			t.Errorf("echo.Realm: got %q, want %q", parsed.Echo.Realm, echo.Realm)
		}
		if parsed.Payload == nil {
			t.Fatal("parsed payload is nil")
		}
		if parsed.Payload.Type != PayloadTypeTransaction {
			t.Errorf("payload type: got %q, want %q", parsed.Payload.Type, PayloadTypeTransaction)
		}
		wantPayload := `{"type":"transaction","tx":"0xdeadbeef"}`
			if parsed.Payload.Payload != wantPayload {
				t.Errorf("payload: got %q, want %q", parsed.Payload.Payload, wantPayload)
		}
		if parsed.Source != "" {
			t.Errorf("source should be empty, got %q", parsed.Source)
		}
	})

	t.Run("with source DID round-trip", func(t *testing.T) {
		echo := &ChallengeEcho{ID: "id2", Realm: "realm", Method: "evm", Intent: "charge"}
		payload := NewHashPayload(`{"type":"hash","hash":"0xabcdef"}`)
		cred := NewPaymentCredentialWithSource(echo, "did:example:123", payload)

		header, err := FormatAuthorization(cred)
		if err != nil {
			t.Fatalf("FormatAuthorization: %v", err)
		}
		parsed, err := ParseAuthorization(header)
		if err != nil {
			t.Fatalf("ParseAuthorization: %v", err)
		}
		if parsed.Source != "did:example:123" {
			t.Errorf("source: got %q, want did:example:123", parsed.Source)
		}
	})

	t.Run("ParseAuthorization error on wrong scheme", func(t *testing.T) {
		_, err := ParseAuthorization("Bearer token")
		if err == nil {
			t.Error("expected error for non-PaymentAuth scheme")
		}
	})

	t.Run("ParseAuthorization error on missing echo", func(t *testing.T) {
		_, err := ParseAuthorization(`PaymentAuth payload-type="transaction", payload="abc"`)
		if err == nil {
			t.Error("expected error for missing echo")
		}
	})

	t.Run("ParseAuthorization error on missing payload-type", func(t *testing.T) {
		_, err := ParseAuthorization(`PaymentAuth echo="abc", payload="abc"`)
		if err == nil {
			t.Error("expected error for missing payload-type")
		}
	})

	t.Run("ParseAuthorization error on missing payload", func(t *testing.T) {
		_, err := ParseAuthorization(`PaymentAuth echo="abc", payload-type="transaction"`)
		if err == nil {
			t.Error("expected error for missing payload")
		}
	})

	t.Run("FormatAuthorization error on nil echo", func(t *testing.T) {
		cred := &PaymentCredential{Payload: NewTransactionPayload(`{"type":"transaction"}`)}
		_, err := FormatAuthorization(cred)
		if err == nil {
			t.Error("expected error for nil echo")
		}
	})

	t.Run("FormatAuthorization error on nil payload", func(t *testing.T) {
		cred := &PaymentCredential{Echo: &ChallengeEcho{ID: "x"}}
		_, err := FormatAuthorization(cred)
		if err == nil {
			t.Error("expected error for nil payload")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// Receipt round-trip
// ─────────────────────────────────────────────────────────────────────────────

func TestReceiptRoundTrip(t *testing.T) {
	t.Run("basic round-trip without settlement", func(t *testing.T) {
		orig := NewSuccessReceipt("rcpt-100", NewMethodName("evm"), NewIntentName("charge"), "")

		header, err := FormatReceipt(orig)
		if err != nil {
			t.Fatalf("FormatReceipt: %v", err)
		}
		parsed, err := ParseReceipt(header)
		if err != nil {
			t.Fatalf("ParseReceipt: %v", err)
		}
		if parsed.ID != orig.ID {
			t.Errorf("id: got %q, want %q", parsed.ID, orig.ID)
		}
		if parsed.Status != orig.Status {
			t.Errorf("status: got %q, want %q", parsed.Status, orig.Status)
		}
		if parsed.Method != orig.Method {
			t.Errorf("method: got %q, want %q", parsed.Method, orig.Method)
		}
		if parsed.Intent != orig.Intent {
			t.Errorf("intent: got %q, want %q", parsed.Intent, orig.Intent)
		}
		if parsed.Settlement != "" {
			t.Errorf("settlement should be empty, got %q", parsed.Settlement)
		}
	})

	t.Run("with settlement round-trip", func(t *testing.T) {
		orig := NewSuccessReceipt("rcpt-200", NewMethodName("evm"), NewIntentName("charge"), "settlement-blob-b64url")

		header, err := orig.ToHeader()
		if err != nil {
			t.Fatalf("ToHeader: %v", err)
		}
		parsed, err := ParseReceipt(header)
		if err != nil {
			t.Fatalf("ParseReceipt: %v", err)
		}
		if parsed.Settlement != "settlement-blob-b64url" {
			t.Errorf("settlement: got %q, want settlement-blob-b64url", parsed.Settlement)
		}
	})

	t.Run("ParseReceipt error on wrong scheme", func(t *testing.T) {
		_, err := ParseReceipt("Bearer token")
		if err == nil {
			t.Error("expected error for wrong scheme")
		}
	})

	t.Run("ParseReceipt error on missing id", func(t *testing.T) {
		_, err := ParseReceipt(`PaymentAuth status="success", method="evm", intent="charge"`)
		if err == nil {
			t.Error("expected error for missing id")
		}
	})

	t.Run("FormatReceipt error on empty id", func(t *testing.T) {
		r := &Receipt{Status: "success", Method: "evm", Intent: "charge"}
		_, err := FormatReceipt(r)
		if err == nil {
			t.Error("expected error for empty id")
		}
	})

	t.Run("FormatReceipt error on empty status", func(t *testing.T) {
		r := &Receipt{ID: "1", Method: "evm", Intent: "charge"}
		_, err := FormatReceipt(r)
		if err == nil {
			t.Error("expected error for empty status")
		}
	})

	t.Run("FormatReceipt error on empty method", func(t *testing.T) {
		r := &Receipt{ID: "1", Status: "success", Intent: "charge"}
		_, err := FormatReceipt(r)
		if err == nil {
			t.Error("expected error for empty method")
		}
	})

	t.Run("FormatReceipt error on empty intent", func(t *testing.T) {
		r := &Receipt{ID: "1", Status: "success", Method: "evm"}
		_, err := FormatReceipt(r)
		if err == nil {
			t.Error("expected error for empty intent")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// ExtractTxHash
// ─────────────────────────────────────────────────────────────────────────────

func TestExtractTxHash(t *testing.T) {
	cases := []struct {
		name      string
		build     func() string
		wantHash  string
		wantFound bool
	}{
		{
			"valid tx_hash field",
			func() string {
				data := map[string]interface{}{"tx_hash": "0xdeadbeef", "other": 42}
				b, _ := json.Marshal(data)
				return Base64URLEncode(b)
			},
			"0xdeadbeef", true,
		},
		{
			"missing tx_hash field",
			func() string {
				data := map[string]interface{}{"block": "123"}
				b, _ := json.Marshal(data)
				return Base64URLEncode(b)
			},
			"", false,
		},
		{
			"invalid base64",
			func() string { return "!not-base64!" },
			"", false,
		},
		{
			"invalid JSON after decode",
			func() string { return Base64URLEncode([]byte("not-json")) },
			"", false,
		},
		{
			"tx_hash not a string",
			func() string {
				data := map[string]interface{}{"tx_hash": 12345}
				b, _ := json.Marshal(data)
				return Base64URLEncode(b)
			},
			"", false,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			hash, found := ExtractTxHash(tc.build())
			if found != tc.wantFound {
				t.Errorf("found=%v, want %v", found, tc.wantFound)
			}
			if hash != tc.wantHash {
				t.Errorf("hash=%q, want %q", hash, tc.wantHash)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// WithSecretKey
// ─────────────────────────────────────────────────────────────────────────────

func TestWithSecretKey(t *testing.T) {
	c := NewPaymentChallenge("realm", "evm", "charge", "req")
	originalID := c.ID

	c.WithSecretKey("mysecret")
	if c.SecretKey != "mysecret" {
		t.Errorf("SecretKey not set: got %q", c.SecretKey)
	}
	if c.ID == originalID {
		t.Error("ID should change after WithSecretKey")
	}

	// Same key produces same ID.
	c2 := NewPaymentChallenge("realm", "evm", "charge", "req")
	c2.WithSecretKey("mysecret")
	if c.ID != c2.ID {
		t.Errorf("same key should produce same ID: %q vs %q", c.ID, c2.ID)
	}

	// Different key produces different ID.
	c3 := NewPaymentChallenge("realm", "evm", "charge", "req")
	c3.WithSecretKey("othersecret")
	if c.ID == c3.ID {
		t.Error("different key should produce different ID")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ToEcho
// ─────────────────────────────────────────────────────────────────────────────

func TestToEcho(t *testing.T) {
	c := NewPaymentChallenge("https://example.com", "evm", "charge", "req123")
	c.WithExpires(9999999999).
		WithDescription("test desc").
		WithDigest("d-abc").
		WithOpaque("opaque-xyz")

	echo := c.ToEcho()

	if echo.ID != c.ID {
		t.Errorf("ID: got %q, want %q", echo.ID, c.ID)
	}
	if echo.Realm != c.Realm {
		t.Errorf("Realm: got %q, want %q", echo.Realm, c.Realm)
	}
	if echo.Method != c.Method {
		t.Errorf("Method: got %q, want %q", echo.Method, c.Method)
	}
	if echo.Intent != c.Intent {
		t.Errorf("Intent: got %q, want %q", echo.Intent, c.Intent)
	}
	if echo.Request != c.Request {
		t.Errorf("Request: got %q, want %q", echo.Request, c.Request)
	}
	if echo.ParseExpiresUnix() != c.Expires {
		t.Errorf("Expires: got %d, want %d", echo.ParseExpiresUnix(), c.Expires)
	}
	if echo.Description != c.Description {
		t.Errorf("Description: got %q, want %q", echo.Description, c.Description)
	}
	if echo.Digest != c.Digest {
		t.Errorf("Digest: got %q, want %q", echo.Digest, c.Digest)
	}
	if echo.Opaque != c.Opaque {
		t.Errorf("Opaque: got %q, want %q", echo.Opaque, c.Opaque)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Verify
// ─────────────────────────────────────────────────────────────────────────────

func TestVerify(t *testing.T) {
	makeChallenge := func() *PaymentChallenge {
		return NewPaymentChallenge("https://example.com", "evm", "charge", "req")
	}

	t.Run("valid credential passes", func(t *testing.T) {
		c := makeChallenge()
		echo := c.ToEcho()
		cred := NewPaymentCredential(echo, NewTransactionPayload(`{"type":"transaction"}`))
		if err := c.Verify(cred); err != nil {
			t.Errorf("unexpected error: %v", err)
		}
	})

	t.Run("nil credential fails", func(t *testing.T) {
		c := makeChallenge()
		if err := c.Verify(nil); err == nil {
			t.Error("expected error for nil credential")
		}
	})

	t.Run("nil echo fails", func(t *testing.T) {
		c := makeChallenge()
		cred := &PaymentCredential{Payload: NewTransactionPayload(`{"type":"transaction"}`)}
		if err := c.Verify(cred); err == nil {
			t.Error("expected error for nil echo")
		}
	})

	t.Run("ID mismatch fails", func(t *testing.T) {
		c := makeChallenge()
		echo := c.ToEcho()
		echo.ID = "wrong-id"
		cred := NewPaymentCredential(echo, NewTransactionPayload(`{"type":"transaction"}`))
		if err := c.Verify(cred); err == nil {
			t.Error("expected error for ID mismatch")
		}
	})

	t.Run("Realm mismatch fails", func(t *testing.T) {
		c := makeChallenge()
		echo := c.ToEcho()
		echo.Realm = "https://other.example.com"
		cred := NewPaymentCredential(echo, NewTransactionPayload(`{"type":"transaction"}`))
		if err := c.Verify(cred); err == nil {
			t.Error("expected error for Realm mismatch")
		}
	})

	t.Run("Method mismatch fails", func(t *testing.T) {
		c := makeChallenge()
		echo := c.ToEcho()
		echo.Method = "bitcoin"
		cred := NewPaymentCredential(echo, NewTransactionPayload(`{"type":"transaction"}`))
		if err := c.Verify(cred); err == nil {
			t.Error("expected error for Method mismatch")
		}
	})

	t.Run("Intent mismatch fails", func(t *testing.T) {
		c := makeChallenge()
		echo := c.ToEcho()
		echo.Intent = "session"
		cred := NewPaymentCredential(echo, NewTransactionPayload(`{"type":"transaction"}`))
		if err := c.Verify(cred); err == nil {
			t.Error("expected error for Intent mismatch")
		}
	})

	t.Run("Request mismatch fails", func(t *testing.T) {
		c := makeChallenge()
		echo := c.ToEcho()
		echo.Request = "differentrequest"
		cred := NewPaymentCredential(echo, NewTransactionPayload(`{"type":"transaction"}`))
		if err := c.Verify(cred); err == nil {
			t.Error("expected error for Request mismatch")
		}
	})

	t.Run("expired challenge fails", func(t *testing.T) {
		c := makeChallenge()
		past := uint64(time.Now().Add(-1 * time.Hour).Unix())
		c.WithExpires(past)
		echo := c.ToEcho()
		cred := NewPaymentCredential(echo, NewTransactionPayload(`{"type":"transaction"}`))
		if err := c.Verify(cred); err == nil {
			t.Error("expected error for expired challenge")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// PaymentChallengeFromHeader / PaymentChallengeFromResponse
// ─────────────────────────────────────────────────────────────────────────────

func TestPaymentChallengeFromHeader(t *testing.T) {
	c := NewPaymentChallenge("https://example.com", "evm", "charge", "req")
	header, err := c.ToHeader()
	if err != nil {
		t.Fatalf("ToHeader: %v", err)
	}

	parsed, err := PaymentChallengeFromHeader(header)
	if err != nil {
		t.Fatalf("PaymentChallengeFromHeader: %v", err)
	}
	if parsed.Realm != c.Realm {
		t.Errorf("realm: got %q, want %q", parsed.Realm, c.Realm)
	}
	if parsed.ID != c.ID {
		t.Errorf("id: got %q, want %q", parsed.ID, c.ID)
	}
}

func TestPaymentChallengeFromResponse(t *testing.T) {
	c := NewPaymentChallenge("https://example.com", "evm", "charge", "req")
	header, err := c.ToHeader()
	if err != nil {
		t.Fatalf("ToHeader: %v", err)
	}

	t.Run("status 402 succeeds", func(t *testing.T) {
		parsed, err := PaymentChallengeFromResponse(402, header)
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if parsed.Realm != c.Realm {
			t.Errorf("realm: got %q, want %q", parsed.Realm, c.Realm)
		}
	})

	t.Run("non-402 fails", func(t *testing.T) {
		_, err := PaymentChallengeFromResponse(200, header)
		if err == nil {
			t.Error("expected error for non-402 status")
		}
	})

	t.Run("status 401 fails", func(t *testing.T) {
		_, err := PaymentChallengeFromResponse(401, header)
		if err == nil {
			t.Error("expected error for 401 status")
		}
	})

	t.Run("status 500 fails", func(t *testing.T) {
		_, err := PaymentChallengeFromResponse(500, header)
		if err == nil {
			t.Error("expected error for 500 status")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// WithBaseUnits
// ─────────────────────────────────────────────────────────────────────────────

func TestWithBaseUnitsCharge(t *testing.T) {
	t.Run("nil Decimals is no-op", func(t *testing.T) {
		r := &ChargeRequest{Amount: "1.5", Currency: "USDC"}
		result, err := r.WithBaseUnits()
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if result.Amount != "1.5" {
			t.Errorf("amount changed: got %q, want 1.5", result.Amount)
		}
		if result.Decimals != nil {
			t.Error("decimals should remain nil")
		}
	})

	t.Run("converts 1.5 with 6 decimals", func(t *testing.T) {
		dec := uint8(6)
		r := &ChargeRequest{Amount: "1.5", Currency: "USDC", Decimals: &dec}
		result, err := r.WithBaseUnits()
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if result.Amount != "1500000" {
			t.Errorf("amount: got %q, want 1500000", result.Amount)
		}
		if result.Decimals != nil {
			t.Error("Decimals should be cleared after WithBaseUnits")
		}
	})

	t.Run("converts 100 with 18 decimals", func(t *testing.T) {
		dec := uint8(18)
		r := &ChargeRequest{Amount: "100", Currency: "ETH", Decimals: &dec}
		result, err := r.WithBaseUnits()
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if result.Amount != "100000000000000000000" {
			t.Errorf("amount: got %q, want 100000000000000000000", result.Amount)
		}
	})

	t.Run("too many decimal places returns error", func(t *testing.T) {
		dec := uint8(6)
		r := &ChargeRequest{Amount: "1.1234567890", Currency: "USDC", Decimals: &dec}
		_, err := r.WithBaseUnits()
		if err == nil {
			t.Error("expected error for amount with too many decimals")
		}
	})
}

func TestWithBaseUnitsSession(t *testing.T) {
	t.Run("nil Decimals is no-op", func(t *testing.T) {
		r := &SessionRequest{Amount: "2.5", Currency: "ETH"}
		result, err := r.WithBaseUnits()
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if result.Amount != "2.5" {
			t.Errorf("amount changed: got %q, want 2.5", result.Amount)
		}
	})

	t.Run("converts 0.001 with 3 decimals", func(t *testing.T) {
		dec := uint8(3)
		r := &SessionRequest{Amount: "0.001", Currency: "ETH", Decimals: &dec}
		result, err := r.WithBaseUnits()
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if result.Amount != "1" {
			t.Errorf("amount: got %q, want 1", result.Amount)
		}
		if result.Decimals != nil {
			t.Error("Decimals should be cleared")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// RequestFromChallenge / RequestFromChallengeTyped
// ─────────────────────────────────────────────────────────────────────────────

func TestRequestFromChallenge(t *testing.T) {
	original := ChargeRequest{
		Amount:   "999999",
		Currency: "USDC",
	}

	encoded, err := SerializeRequest(original)
	if err != nil {
		t.Fatalf("SerializeRequest: %v", err)
	}

	c := NewPaymentChallenge("realm", "evm", "charge", encoded)

	t.Run("RequestFromChallenge returns raw JSON", func(t *testing.T) {
		raw, err := RequestFromChallenge(c)
		if err != nil {
			t.Fatalf("RequestFromChallenge: %v", err)
		}
		var result ChargeRequest
		if err := json.Unmarshal(raw, &result); err != nil {
			t.Fatalf("unmarshal: %v", err)
		}
		if result.Amount != original.Amount || result.Currency != original.Currency {
			t.Errorf("round-trip mismatch: got %+v, want %+v", result, original)
		}
	})

	t.Run("RequestFromChallengeTyped decodes directly", func(t *testing.T) {
		var result ChargeRequest
		if err := RequestFromChallengeTyped(c, &result); err != nil {
			t.Fatalf("RequestFromChallengeTyped: %v", err)
		}
		if result.Amount != original.Amount || result.Currency != original.Currency {
			t.Errorf("round-trip mismatch: got %+v, want %+v", result, original)
		}
	})

	t.Run("RequestFromChallenge error on invalid request", func(t *testing.T) {
		bad := &PaymentChallenge{Request: "!not-base64!"}
		_, err := RequestFromChallenge(bad)
		if err == nil {
			t.Error("expected error for invalid base64")
		}
	})

	t.Run("RequestFromChallengeTyped error on invalid request", func(t *testing.T) {
		bad := &PaymentChallenge{Request: "!not-base64!"}
		var result ChargeRequest
		if err := RequestFromChallengeTyped(bad, &result); err == nil {
			t.Error("expected error for invalid base64")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// EffectiveExpires
// ─────────────────────────────────────────────────────────────────────────────

func TestEffectiveExpires(t *testing.T) {
	t.Run("returns Expires when set", func(t *testing.T) {
		c := NewPaymentChallenge("realm", "evm", "charge", "req")
		c.WithExpires(9999999999)
		if c.EffectiveExpires() != 9999999999 {
			t.Errorf("EffectiveExpires: got %d, want 9999999999", c.EffectiveExpires())
		}
	})

	t.Run("returns future time when Expires=0", func(t *testing.T) {
		c := NewPaymentChallenge("realm", "evm", "charge", "req")
		now := uint64(time.Now().Unix())
		effective := c.EffectiveExpires()
		if effective <= now {
			t.Errorf("EffectiveExpires should be in the future: got %d, now %d", effective, now)
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// WithDescription / WithDigest / WithOpaque (coverage for chained builders)
// ─────────────────────────────────────────────────────────────────────────────

func TestChallengeBuilders(t *testing.T) {
	c := NewPaymentChallenge("realm", "evm", "charge", "req")

	c.WithDescription("human readable description")
	if c.Description != "human readable description" {
		t.Errorf("Description: got %q", c.Description)
	}

	oldID := c.ID
	c.WithDigest("sha256-xyz")
	if c.Digest != "sha256-xyz" {
		t.Errorf("Digest: got %q", c.Digest)
	}
	if c.ID == oldID {
		t.Error("ID should change after WithDigest")
	}

	oldID = c.ID
	c.WithOpaque("nonce-opaque")
	if c.Opaque != "nonce-opaque" {
		t.Errorf("Opaque: got %q", c.Opaque)
	}
	if c.ID == oldID {
		t.Error("ID should change after WithOpaque")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ParseAmountBigInt / ValidateMaxAmount
// ─────────────────────────────────────────────────────────────────────────────

func TestChargeRequestHelpers(t *testing.T) {
	t.Run("ParseAmountBigInt valid", func(t *testing.T) {
		r := &ChargeRequest{Amount: "1000000", Currency: "USDC"}
		n, err := r.ParseAmountBigInt()
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if n.String() != "1000000" {
			t.Errorf("got %s", n.String())
		}
	})

	t.Run("ParseAmountBigInt invalid", func(t *testing.T) {
		r := &ChargeRequest{Amount: "not-a-number", Currency: "USDC"}
		_, err := r.ParseAmountBigInt()
		if err == nil {
			t.Error("expected error for invalid amount")
		}
	})

	t.Run("ValidateMaxAmount within limit", func(t *testing.T) {
		r := &ChargeRequest{Amount: "500000", Currency: "USDC"}
		if err := r.ValidateMaxAmount("1000000"); err != nil {
			t.Errorf("unexpected error: %v", err)
		}
	})

	t.Run("ValidateMaxAmount exceeds limit", func(t *testing.T) {
		r := &ChargeRequest{Amount: "2000000", Currency: "USDC"}
		if err := r.ValidateMaxAmount("1000000"); err == nil {
			t.Error("expected error when amount exceeds max")
		}
	})

	t.Run("ValidateMaxAmount equal to limit", func(t *testing.T) {
		r := &ChargeRequest{Amount: "1000000", Currency: "USDC"}
		if err := r.ValidateMaxAmount("1000000"); err != nil {
			t.Errorf("unexpected error: %v", err)
		}
	})

	t.Run("ValidateMaxAmount invalid max", func(t *testing.T) {
		r := &ChargeRequest{Amount: "1000000", Currency: "USDC"}
		if err := r.ValidateMaxAmount("not-a-number"); err == nil {
			t.Error("expected error for invalid max")
		}
	})
}

func TestSessionRequestValidateMaxAmount(t *testing.T) {
	t.Run("within limit", func(t *testing.T) {
		r := &SessionRequest{Amount: "500", Currency: "ETH"}
		if err := r.ValidateMaxAmount("1000"); err != nil {
			t.Errorf("unexpected error: %v", err)
		}
	})

	t.Run("exceeds limit", func(t *testing.T) {
		r := &SessionRequest{Amount: "2000", Currency: "ETH"}
		if err := r.ValidateMaxAmount("1000"); err == nil {
			t.Error("expected error when amount exceeds max")
		}
	})

	t.Run("invalid amount", func(t *testing.T) {
		r := &SessionRequest{Amount: "bad", Currency: "ETH"}
		if err := r.ValidateMaxAmount("1000"); err == nil {
			t.Error("expected error for invalid amount")
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// NewIntentName
// ─────────────────────────────────────────────────────────────────────────────

func TestNewIntentName(t *testing.T) {
	cases := []struct {
		input string
		want  string
	}{
		{"CHARGE", "charge"},
		{"SESSION", "session"},
		{"Charge", "charge"},
		{"charge", "charge"},
	}
	for _, tc := range cases {
		got := NewIntentName(tc.input)
		if got.String() != tc.want {
			t.Errorf("NewIntentName(%q) = %q, want %q", tc.input, got.String(), tc.want)
		}
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// VerificationError / ErrorCode additional coverage
// ─────────────────────────────────────────────────────────────────────────────

func TestVerificationErrorAdditional(t *testing.T) {
	t.Run("VerificationErrorTransactionFailed sets correct code", func(t *testing.T) {
		e := VerificationErrorTransactionFailed("failed")
		if e.Code != ErrorCodeTransactionFailed {
			t.Errorf("expected code %q, got %q", ErrorCodeTransactionFailed, e.Code)
		}
	})

	t.Run("Error() method returns non-empty string", func(t *testing.T) {
		e := NewVerificationError("some error message")
		if e.Error() == "" {
			t.Error("Error() should return non-empty string")
		}
	})

	t.Run("ErrorCodeUnknown string", func(t *testing.T) {
		e := ErrorCode("custom-error")
		if e.String() != "custom-error" {
			t.Errorf("got %q", e.String())
		}
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// Full integration: challenge → credential → verify flow
// ─────────────────────────────────────────────────────────────────────────────

func TestFullChallengeCredentialFlow(t *testing.T) {
	// Server creates a charge request and challenge.
	req := ChargeRequest{Amount: "1000000", Currency: "USDC"}
	encoded, err := SerializeRequest(req)
	if err != nil {
		t.Fatalf("SerializeRequest: %v", err)
	}

	challenge := NewPaymentChallenge("https://shop.example.com", "evm", "charge", encoded)
	challenge.WithSecretKey("server-secret")

	// Server formats WWW-Authenticate header.
	wwwAuth, err := challenge.ToHeader()
	if err != nil {
		t.Fatalf("ToHeader: %v", err)
	}

	// Client parses the header.
	parsed, err := PaymentChallengeFromResponse(402, wwwAuth)
	if err != nil {
		t.Fatalf("PaymentChallengeFromResponse: %v", err)
	}

	// Client decodes the request.
	var clientReq ChargeRequest
	if err := RequestFromChallengeTyped(parsed, &clientReq); err != nil {
		t.Fatalf("RequestFromChallengeTyped: %v", err)
	}
	if clientReq.Amount != req.Amount {
		t.Errorf("request.Amount: got %q, want %q", clientReq.Amount, req.Amount)
	}

	// Client sends credential with the echo.
	echo := parsed.ToEcho()
	payload := NewTransactionPayload(`{"type":"transaction","tx":"0xsometxhash"}`)
	cred := NewPaymentCredential(echo, payload)

	// Client formats Authorization header.
	authHeader, err := FormatAuthorization(cred)
	if err != nil {
		t.Fatalf("FormatAuthorization: %v", err)
	}

	// Server parses the Authorization header.
	parsedCred, err := ParseAuthorization(authHeader)
	if err != nil {
		t.Fatalf("ParseAuthorization: %v", err)
	}

	// Server verifies the credential against the original challenge.
	if err := challenge.Verify(parsedCred); err != nil {
		t.Fatalf("Verify: %v", err)
	}

	// Server creates receipt.
	receipt := NewSuccessReceipt(challenge.ID, NewMethodName("evm"), NewIntentName("charge"), "")
	receiptHeader, err := receipt.ToHeader()
	if err != nil {
		t.Fatalf("receipt.ToHeader: %v", err)
	}

	// Client parses receipt.
	parsedReceipt, err := ParseReceipt(receiptHeader)
	if err != nil {
		t.Fatalf("ParseReceipt: %v", err)
	}
	if parsedReceipt.Status != ReceiptStatusSuccess {
		t.Errorf("receipt status: got %q, want %q", parsedReceipt.Status, ReceiptStatusSuccess)
	}
}
