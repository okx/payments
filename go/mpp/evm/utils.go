package evm

import (
	"encoding/hex"
	"fmt"
	"math/big"
	"strings"
)

// ParseAddress validates a hex Ethereum address (with or without 0x prefix)
// and returns the normalized lowercase "0x"-prefixed form.
func ParseAddress(s string) (string, error) {
	raw := s
	if strings.HasPrefix(raw, "0x") || strings.HasPrefix(raw, "0X") {
		raw = raw[2:]
	}
	if len(raw) != 40 {
		return "", fmt.Errorf("invalid address length: expected 40 hex chars, got %d", len(raw))
	}
	if _, err := hex.DecodeString(raw); err != nil {
		return "", fmt.Errorf("invalid address hex: %w", err)
	}
	return "0x" + strings.ToLower(raw), nil
}

// ParseAmount parses a decimal string into a *big.Int, rejecting negative values.
func ParseAmount(s string) (*big.Int, error) {
	s = strings.TrimSpace(s)
	n := new(big.Int)
	if _, ok := n.SetString(s, 10); !ok {
		return nil, fmt.Errorf("invalid amount: %q is not a valid decimal integer", s)
	}
	if n.Sign() < 0 {
		return nil, fmt.Errorf("invalid amount: negative value %s not allowed", s)
	}
	return n, nil
}

// ParseMemoBytes decodes a hex string into a [32]byte array.
// The decoded bytes must be exactly 32 bytes.
func ParseMemoBytes(memo string) ([32]byte, error) {
	var result [32]byte
	raw := memo
	if strings.HasPrefix(raw, "0x") || strings.HasPrefix(raw, "0X") {
		raw = raw[2:]
	}
	b, err := hex.DecodeString(raw)
	if err != nil {
		return result, fmt.Errorf("invalid memo hex: %w", err)
	}
	if len(b) != 32 {
		return result, fmt.Errorf("invalid memo length: expected 32 bytes, got %d", len(b))
	}
	copy(result[:], b)
	return result, nil
}

// ParseUnits multiplies a human-readable decimal amount string by 10^decimals
// and returns the result as a base-unit string. Handles both integer ("100")
// and decimal ("1.5") inputs without losing precision.
func ParseUnits(amount string, decimals uint8) (string, error) {
	amount = strings.TrimSpace(amount)
	if amount == "" {
		return "", fmt.Errorf("amount must not be empty")
	}

	// Split on decimal point
	parts := strings.SplitN(amount, ".", 2)
	if len(parts) > 2 {
		return "", fmt.Errorf("invalid amount: multiple decimal points in %q", amount)
	}

	intPart := parts[0]
	fracPart := ""
	if len(parts) == 2 {
		fracPart = parts[1]
	}

	// Validate both parts contain only digits (intPart may have a leading minus)
	negative := false
	if strings.HasPrefix(intPart, "-") {
		negative = true
		intPart = intPart[1:]
	}
	if negative {
		return "", fmt.Errorf("invalid amount: negative value not allowed")
	}

	if !isAllDigits(intPart) || intPart == "" {
		return "", fmt.Errorf("invalid amount: non-numeric integer part in %q", amount)
	}
	if fracPart != "" && !isAllDigits(fracPart) {
		return "", fmt.Errorf("invalid amount: non-numeric fractional part in %q", amount)
	}

	d := int(decimals)

	// Pad or truncate fractional part to exactly `decimals` digits
	if len(fracPart) > d {
		// More decimal digits than allowed — truncation would lose precision; return error
		return "", fmt.Errorf("invalid amount: fractional part %q exceeds %d decimal places", fracPart, d)
	}
	// Right-pad with zeros to reach `decimals` digits
	fracPart = fracPart + strings.Repeat("0", d-len(fracPart))

	// Combine: intPart + fracPart forms the result without any decimal point
	combined := intPart + fracPart

	// Parse as big.Int to normalise (strips any leading zeros introduced by padding)
	n := new(big.Int)
	if _, ok := n.SetString(combined, 10); !ok {
		return "", fmt.Errorf("invalid amount: could not parse combined value %q", combined)
	}

	return n.String(), nil
}

// isAllDigits returns true if s is non-empty and contains only ASCII digit characters.
func isAllDigits(s string) bool {
	if s == "" {
		return false
	}
	for _, c := range s {
		if c < '0' || c > '9' {
			return false
		}
	}
	return true
}
