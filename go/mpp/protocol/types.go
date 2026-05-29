package protocol

import (
	"encoding/base64"
	"encoding/json"
	"fmt"

	"strings"
)

// Intent constants.
const IntentCharge = "charge"
const IntentSession = "session"

// MethodName represents a normalized lowercase payment method name.
type MethodName string

// NewMethodName normalizes the given name to ASCII lowercase and returns a MethodName.
func NewMethodName(name string) MethodName {
	return MethodName(asciiLower(name))
}

// String returns the string representation of the MethodName.
func (m MethodName) String() string {
	return string(m)
}

// IsValid reports whether the MethodName contains only lowercase ASCII letters (a-z).
func (m MethodName) IsValid() bool {
	if len(m) == 0 {
		return false
	}
	for i := 0; i < len(m); i++ {
		c := m[i]
		if c < 'a' || c > 'z' {
			return false
		}
	}
	return true
}

// IntentName represents a normalized lowercase payment intent name.
type IntentName string

// NewIntentName normalizes the given name to lowercase and returns an IntentName.
func NewIntentName(name string) IntentName {
	return IntentName(asciiLower(name))
}

// String returns the string representation of the IntentName.
func (i IntentName) String() string {
	return string(i)
}

// IsCharge reports whether the intent is a charge intent.
func (i IntentName) IsCharge() bool {
	return string(i) == IntentCharge
}

// IsSession reports whether the intent is a session intent.
func (i IntentName) IsSession() bool {
	return string(i) == IntentSession
}

// PayloadType represents the type of a payment payload.
type PayloadType string

const (
	PayloadTypeTransaction PayloadType = "transaction"
	PayloadTypeHash        PayloadType = "hash"
	PayloadTypeProof       PayloadType = "proof"
)

// ReceiptStatus represents the status of a payment receipt.
type ReceiptStatus string

const ReceiptStatusSuccess ReceiptStatus = "success"

// PaymentProtocol identifies a payment protocol variant.
type PaymentProtocol int

const PaymentProtocolWebPaymentAuth PaymentProtocol = 1

// DetectPaymentProtocol inspects a WWW-Authenticate header value and returns
// the matching PaymentProtocol. It returns false if no known scheme is found.
func DetectPaymentProtocol(wwwAuth string) (PaymentProtocol, bool) {
	scheme, _ := ExtractPaymentScheme(wwwAuth)
	if strings.EqualFold(scheme, PaymentScheme) {
		return PaymentProtocolWebPaymentAuth, true
	}
	return 0, false
}

// Base64URLJson holds a base64url-encoded JSON value.
type Base64URLJson struct {
	Raw string
}

// NewBase64URLJson creates a Base64URLJson from an already-encoded raw string.
func NewBase64URLJson(raw string) Base64URLJson {
	return Base64URLJson{Raw: raw}
}

// Base64URLJsonFromValue JSON-marshals v and base64url-encodes the result.
func Base64URLJsonFromValue(v interface{}) (Base64URLJson, error) {
	data, err := json.Marshal(v)
	if err != nil {
		return Base64URLJson{}, fmt.Errorf("base64urljson marshal: %w", err)
	}
	return Base64URLJson{Raw: base64.RawURLEncoding.EncodeToString(data)}, nil
}

// DecodeValue base64url-decodes the raw string and returns the raw JSON bytes.
func (b Base64URLJson) DecodeValue() (json.RawMessage, error) {
	data, err := base64.RawURLEncoding.DecodeString(b.Raw)
	if err != nil {
		return nil, fmt.Errorf("base64urljson decode: %w", err)
	}
	return json.RawMessage(data), nil
}

// Decode base64url-decodes the raw string and unmarshals the JSON into v.
func (b Base64URLJson) Decode(v interface{}) error {
	raw, err := b.DecodeValue()
	if err != nil {
		return err
	}
	if err := json.Unmarshal(raw, v); err != nil {
		return fmt.Errorf("base64urljson unmarshal: %w", err)
	}
	return nil
}

// IsEmpty reports whether the raw string is empty.
func (b Base64URLJson) IsEmpty() bool {
	return b.Raw == ""
}

// Base64URLEncode encodes data using base64 raw URL encoding (no padding).
func Base64URLEncode(data []byte) string {
	return base64.RawURLEncoding.EncodeToString(data)
}

// Base64URLDecode decodes a base64url string to bytes.
// Accepts both URL-safe (-,_) and standard (+,/) alphabets, with or without
// padding. This follows Postel's law — strict in output, lenient in input.
// Rejects inputs with non-base64 characters or non-zero trailing bits.
func Base64URLDecode(input string) ([]byte, error) {
	var b strings.Builder
	b.Grow(len(input))
	for i := 0; i < len(input); i++ {
		c := input[i]
		switch {
		case c == '=':
			// strip padding
		case c == '+':
			b.WriteByte('-')
		case c == '/':
			b.WriteByte('_')
		case (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-' || c == '_':
			b.WriteByte(c)
		default:
			return nil, fmt.Errorf("base64url decode: Invalid symbol %d, offset %d.", c, i)
		}
	}
	normalized := b.String()
	if err := validateBase64Trailing(normalized); err != nil {
		return nil, fmt.Errorf("base64url decode: %w", err)
	}
	data, err := base64.RawURLEncoding.DecodeString(normalized)
	if err != nil {
		return nil, fmt.Errorf("base64url decode: %w", err)
	}
	return data, nil
}

// validateBase64Trailing rejects inputs whose trailing bits are non-zero,
// matching the Rust base64 crate's strict decoding behavior.
func validateBase64Trailing(s string) error {
	if len(s) == 0 {
		return nil
	}
	remainder := len(s) % 4
	if remainder == 1 {
		return fmt.Errorf("invalid base64url length")
	}
	if remainder == 0 {
		return nil // no trailing bits
	}
	last := s[len(s)-1]
	val := base64URLCharVal(last)
	if val < 0 {
		return nil // let the decoder handle invalid chars
	}
	// remainder==2 means 12 bits for 1 byte → 4 trailing bits; last char must have lower 4 bits = 0
	// remainder==3 means 18 bits for 2 bytes → 2 trailing bits; last char must have lower 2 bits = 0
	if remainder == 2 && (val&0x0F) != 0 {
		return fmt.Errorf("Invalid last symbol %d, offset %d.", last, len(s)-1)
	}
	if remainder == 3 && (val&0x03) != 0 {
		return fmt.Errorf("Invalid last symbol %d, offset %d.", last, len(s)-1)
	}
	return nil
}

func asciiLower(s string) string {
	b := make([]byte, len(s))
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c >= 'A' && c <= 'Z' {
			b[i] = c + ('a' - 'A')
		} else {
			b[i] = c
		}
	}
	return string(b)
}

func base64URLCharVal(c byte) int {
	switch {
	case c >= 'A' && c <= 'Z':
		return int(c - 'A')
	case c >= 'a' && c <= 'z':
		return int(c-'a') + 26
	case c >= '0' && c <= '9':
		return int(c-'0') + 52
	case c == '-':
		return 62
	case c == '_':
		return 63
	default:
		return -1
	}
}

// ParseUnits converts a decimal amount string to its integer base-unit
// representation by multiplying by 10^decimals.
//
// Examples:
//
//	ParseUnits("1.5", 6)  → "1500000"
//	ParseUnits("1",   18) → "1000000000000000000"
//	ParseUnits("0.1", 2)  → "10"
func ParseUnits(amount string, decimals uint8) (string, error) {
	if amount == "" {
		return "", fmt.Errorf("parseunits: Amount cannot be empty")
	}

	parts := strings.Split(amount, ".")
	if len(parts) > 2 {
		return "", fmt.Errorf("parseunits: Invalid amount format: %s", amount)
	}

	intPart := parts[0]
	fracPart := ""
	if len(parts) == 2 {
		fracPart = parts[1]
	}

	d := int(decimals)

	if len(fracPart) > d {
		return "", fmt.Errorf("parseunits: Amount %s has more than %d decimal places", amount, decimals)
	}

	paddedFrac := fracPart + strings.Repeat("0", d-len(fracPart))
	combined := intPart + paddedFrac

	result := strings.TrimLeft(combined, "0")
	if result == "" {
		return "0", nil
	}
	return result, nil
}
