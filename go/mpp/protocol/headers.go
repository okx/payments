package protocol

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"strconv"
	"strings"
	"time"
)

// Header name constants.
const WWWAuthenticateHeader = "WWW-Authenticate"
const AuthorizationHeader = "Authorization"
const PaymentReceiptHeader = "Payment-Receipt"
const PaymentScheme = "Payment"

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

// parseParams extracts key="value" pairs from the portion of a header that
// follows the scheme token (e.g. the comma-separated params of a PaymentAuth
// header).  It is intentionally tolerant: unquoted values and extra whitespace
// are handled gracefully.  Each duplicate key overwrites the previous value.
func parseParams(s string) map[string]string {
	out := make(map[string]string)
	s = strings.TrimSpace(s)
	for len(s) > 0 {
		// Skip leading commas/spaces.
		s = strings.TrimLeft(s, ", \t")
		if len(s) == 0 {
			break
		}

		// Find '='.
		eqIdx := strings.IndexByte(s, '=')
		if eqIdx < 0 {
			break
		}
		key := strings.TrimSpace(s[:eqIdx])
		s = s[eqIdx+1:]

		// Read value — quoted or unquoted.
		var value string
		if len(s) > 0 && s[0] == '"' {
			// Quoted value — scan for closing '"' (no escape handling needed for
			// our use-case).
			end := strings.IndexByte(s[1:], '"')
			if end < 0 {
				// Unterminated quote — consume the rest.
				value = s[1:]
				s = ""
			} else {
				value = s[1 : end+1]
				s = s[end+2:]
			}
		} else {
			// Unquoted — read up to the next comma.
			commaIdx := strings.IndexByte(s, ',')
			if commaIdx < 0 {
				value = strings.TrimSpace(s)
				s = ""
			} else {
				value = strings.TrimSpace(s[:commaIdx])
				s = s[commaIdx+1:]
			}
		}
		if key != "" {
			out[key] = value
		}
	}
	return out
}

// ─────────────────────────────────────────────────────────────────────────────
// Scheme extraction
// ─────────────────────────────────────────────────────────────────────────────

// ExtractPaymentScheme returns the leading scheme token from a header value and
// whether it equals PaymentScheme ("PaymentAuth").
func ExtractPaymentScheme(header string) (string, bool) {
	header = strings.TrimSpace(header)
	idx := strings.IndexAny(header, " \t")
	var scheme string
	if idx < 0 {
		scheme = header
	} else {
		scheme = header[:idx]
	}
	return scheme, scheme == PaymentScheme
}

// ─────────────────────────────────────────────────────────────────────────────
// WWW-Authenticate
// ─────────────────────────────────────────────────────────────────────────────

// ParseWWWAuthenticate parses a WWW-Authenticate: PaymentAuth header value into
// a PaymentChallenge.
//
// Expected format:
//
//	PaymentAuth realm="<realm>", method="<method>", intent="<intent>",
//	  request="<base64url>", id="<id>" [, expires="<unix>"]
//	  [, description="<desc>"] [, digest="<digest>"] [, opaque="<opaque>"]
func ParseWWWAuthenticate(header string) (*PaymentChallenge, error) {
	scheme, ok := ExtractPaymentScheme(header)
	if !ok {
		return nil, fmt.Errorf("unsupported auth scheme %q, expected %q", scheme, PaymentScheme)
	}

	// Advance past the scheme token.
	rest := strings.TrimSpace(header[len(scheme):])
	params := parseParams(rest)

	required := []string{"realm", "method", "intent", "request", "id"}
	for _, k := range required {
		if _, exists := params[k]; !exists {
			return nil, fmt.Errorf("WWW-Authenticate missing required param %q", k)
		}
	}

	c := &PaymentChallenge{
		ID:          params["id"],
		Realm:       params["realm"],
		Method:      params["method"],
		Intent:      params["intent"],
		Request:     params["request"],
		Description: params["description"],
		Digest:      params["digest"],
		Opaque:      params["opaque"],
	}

	if raw, ok := params["expires"]; ok && raw != "" {
		// Accept both RFC3339 and unix timestamp for backwards compatibility.
		if t, err := time.Parse(time.RFC3339, raw); err == nil {
			c.Expires = uint64(t.Unix())
		} else if v, err := strconv.ParseUint(raw, 10, 64); err == nil {
			c.Expires = v
		} else {
			return nil, fmt.Errorf("WWW-Authenticate invalid expires value %q", raw)
		}
	}

	return c, nil
}

// ParseWWWAuthenticateAll parses each header value and returns a slice of
// PaymentChallenge.  The first error encountered halts processing.
func ParseWWWAuthenticateAll(headers []string) ([]*PaymentChallenge, error) {
	out := make([]*PaymentChallenge, 0, len(headers))
	for _, h := range headers {
		c, err := ParseWWWAuthenticate(h)
		if err != nil {
			return nil, err
		}
		out = append(out, c)
	}
	return out, nil
}

// FormatWWWAuthenticate formats a PaymentChallenge as a WWW-Authenticate header
// value.  The ID, Realm, Method, Intent, and Request fields are required.
func FormatWWWAuthenticate(c *PaymentChallenge) (string, error) {
	if c.Realm == "" {
		return "", fmt.Errorf("FormatWWWAuthenticate: realm is empty")
	}
	if c.Method == "" {
		return "", fmt.Errorf("FormatWWWAuthenticate: method is empty")
	}
	if c.Intent == "" {
		return "", fmt.Errorf("FormatWWWAuthenticate: intent is empty")
	}
	if c.Request == "" {
		return "", fmt.Errorf("FormatWWWAuthenticate: request is empty")
	}
	if c.ID == "" {
		return "", fmt.Errorf("FormatWWWAuthenticate: id is empty")
	}

	var sb strings.Builder
	sb.WriteString(PaymentScheme)
	sb.WriteString(` realm="`)
	sb.WriteString(c.Realm)
	sb.WriteString(`", method="`)
	sb.WriteString(c.Method)
	sb.WriteString(`", intent="`)
	sb.WriteString(c.Intent)
	sb.WriteString(`", request="`)
	sb.WriteString(c.Request)
	sb.WriteString(`", id="`)
	sb.WriteString(c.ID)
	sb.WriteByte('"')

	if c.Expires != 0 {
		sb.WriteString(`, expires="`)
		sb.WriteString(time.Unix(int64(c.Expires), 0).UTC().Format(time.RFC3339))
		sb.WriteByte('"')
	}
	if c.Description != "" {
		sb.WriteString(`, description="`)
		sb.WriteString(c.Description)
		sb.WriteByte('"')
	}
	if c.Digest != "" {
		sb.WriteString(`, digest="`)
		sb.WriteString(c.Digest)
		sb.WriteByte('"')
	}
	if c.Opaque != "" {
		sb.WriteString(`, opaque="`)
		sb.WriteString(c.Opaque)
		sb.WriteByte('"')
	}

	return sb.String(), nil
}

// FormatWWWAuthenticateMany formats multiple challenges into individual header
// value strings.
func FormatWWWAuthenticateMany(cs []*PaymentChallenge) ([]string, error) {
	out := make([]string, 0, len(cs))
	for _, c := range cs {
		h, err := FormatWWWAuthenticate(c)
		if err != nil {
			return nil, err
		}
		out = append(out, h)
	}
	return out, nil
}

// ─────────────────────────────────────────────────────────────────────────────
// Authorization
// ─────────────────────────────────────────────────────────────────────────────

// ParseAuthorization parses an Authorization: Payment header value into a
// PaymentCredential.
//
// Expected format (base64url-encoded JSON blob):
//
//	Payment <base64url({"challenge":{...},"source":"did:...","payload":{...}})>
func ParseAuthorization(header string) (*PaymentCredential, error) {
	scheme, ok := ExtractPaymentScheme(header)
	if !ok {
		return nil, fmt.Errorf("unsupported auth scheme %q, expected %q", scheme, PaymentScheme)
	}
	token := strings.TrimSpace(header[len(scheme):])
	if token == "" {
		return nil, fmt.Errorf("Authorization: empty token after scheme")
	}

	credBytes, err := Base64URLDecode(token)
	if err != nil {
		return nil, fmt.Errorf("Authorization base64url decode: %w", err)
	}

	// Unmarshal into intermediate struct so we can keep payload as raw JSON.
	var raw struct {
		Challenge *ChallengeEcho  `json:"challenge"`
		Source    string          `json:"source"`
		Payload   json.RawMessage `json:"payload"`
	}
	if err := json.Unmarshal(credBytes, &raw); err != nil {
		return nil, fmt.Errorf("Authorization JSON unmarshal: %w", err)
	}
	if raw.Challenge == nil {
		return nil, fmt.Errorf("Authorization missing required field \"challenge\"")
	}

	// Extract type from the payload object.
	var payloadType PayloadType
	if len(raw.Payload) > 0 {
		var meta struct {
			Type string `json:"type"`
		}
		if err := json.Unmarshal(raw.Payload, &meta); err == nil {
			payloadType = PayloadType(meta.Type)
		}
	}

	return &PaymentCredential{
		Echo:   raw.Challenge,
		Source: raw.Source,
		Payload: &PaymentPayload{
			Type:    payloadType,
			Payload: string(raw.Payload),
		},
	}, nil
}

// FormatAuthorization formats a PaymentCredential as an Authorization header
// value: Payment <base64url-encoded JSON>.
func FormatAuthorization(c *PaymentCredential) (string, error) {
	if c.Echo == nil {
		return "", fmt.Errorf("FormatAuthorization: echo is nil")
	}
	if c.Payload == nil {
		return "", fmt.Errorf("FormatAuthorization: payload is nil")
	}

	cred := struct {
		Challenge *ChallengeEcho  `json:"challenge"`
		Source    string          `json:"source,omitempty"`
		Payload   json.RawMessage `json:"payload"`
	}{
		Challenge: c.Echo,
		Source:    c.Source,
		Payload:   json.RawMessage(c.Payload.Payload),
	}
	data, err := json.Marshal(cred)
	if err != nil {
		return "", fmt.Errorf("FormatAuthorization: marshal: %w", err)
	}
	return PaymentScheme + " " + Base64URLEncode(data), nil
}

// ─────────────────────────────────────────────────────────────────────────────
// Payment-Receipt
// ─────────────────────────────────────────────────────────────────────────────

// ParseReceipt parses a Payment-Receipt: PaymentAuth header value into a Receipt.
//
// Expected format:
//
//	PaymentAuth id="<id>", status="<status>", method="<method>", intent="<intent>"
//	  [, settlement="<base64url>"]
func ParseReceipt(header string) (*Receipt, error) {
	scheme, ok := ExtractPaymentScheme(header)
	if !ok {
		return nil, fmt.Errorf("unsupported auth scheme %q, expected %q", scheme, PaymentScheme)
	}
	rest := strings.TrimSpace(header[len(scheme):])
	params := parseParams(rest)

	for _, k := range []string{"id", "status", "method", "intent"} {
		if _, exists := params[k]; !exists {
			return nil, fmt.Errorf("Payment-Receipt missing required param %q", k)
		}
	}

	return &Receipt{
		ID:         params["id"],
		Status:     ReceiptStatus(params["status"]),
		Method:     MethodName(params["method"]),
		Intent:     IntentName(params["intent"]),
		Settlement: params["settlement"],
	}, nil
}

// FormatReceipt formats a Receipt as a Payment-Receipt header value.
func FormatReceipt(r *Receipt) (string, error) {
	if r.ID == "" {
		return "", fmt.Errorf("FormatReceipt: id is empty")
	}
	if r.Status == "" {
		return "", fmt.Errorf("FormatReceipt: status is empty")
	}
	if r.Method == "" {
		return "", fmt.Errorf("FormatReceipt: method is empty")
	}
	if r.Intent == "" {
		return "", fmt.Errorf("FormatReceipt: intent is empty")
	}

	var sb strings.Builder
	sb.WriteString(PaymentScheme)
	sb.WriteString(` id="`)
	sb.WriteString(r.ID)
	sb.WriteString(`", status="`)
	sb.WriteString(string(r.Status))
	sb.WriteString(`", method="`)
	sb.WriteString(string(r.Method))
	sb.WriteString(`", intent="`)
	sb.WriteString(string(r.Intent))
	sb.WriteByte('"')

	if r.Settlement != "" {
		sb.WriteString(`, settlement="`)
		sb.WriteString(r.Settlement)
		sb.WriteByte('"')
	}

	return sb.String(), nil
}

// ─────────────────────────────────────────────────────────────────────────────
// ComputeChallengeID
// ─────────────────────────────────────────────────────────────────────────────

// ComputeChallengeID computes the HMAC-SHA256 hex digest of the pipe-joined
// challenge fields.  If secretKey is empty an empty HMAC key is used (the
// result is still deterministic).
//
// Fields are joined in this order:
//
//	realm | method | intent | request | expires | digest | opaque
func ComputeChallengeID(secretKey, realm, method, intent, request string, expires uint64, digest, opaque string) string {
	msg := strings.Join([]string{
		realm,
		method,
		intent,
		request,
		strconv.FormatUint(expires, 10),
		digest,
		opaque,
	}, "|")

	mac := hmac.New(sha256.New, []byte(secretKey))
	mac.Write([]byte(msg))
	return hex.EncodeToString(mac.Sum(nil))
}

// ─────────────────────────────────────────────────────────────────────────────
// ExtractTxHash
// ─────────────────────────────────────────────────────────────────────────────

// ExtractTxHash decodes a base64url-encoded JSON object and extracts the
// "tx_hash" string field.  Returns the hash and true on success, or ("", false)
// if the field is absent or the input is malformed.
func ExtractTxHash(receiptB64 string) (string, bool) {
	data, err := Base64URLDecode(receiptB64)
	if err != nil {
		return "", false
	}

	var obj map[string]json.RawMessage
	if err := json.Unmarshal(data, &obj); err != nil {
		return "", false
	}

	raw, ok := obj["tx_hash"]
	if !ok {
		return "", false
	}

	var txHash string
	if err := json.Unmarshal(raw, &txHash); err != nil {
		return "", false
	}

	return txHash, true
}
