package server

import (
	"fmt"
	"math/big"
	"strings"
)

// ParseDollarAmount converts a human-readable decimal amount string to its
// integer base-unit representation using the given number of decimal places.
//
// Rules:
//   - Leading and trailing whitespace is stripped.
//   - The string must be non-empty after trimming.
//   - Only ASCII digits (0-9) and at most one decimal point (".") are allowed.
//   - The result is the amount multiplied by 10^decimals, truncating any
//     fractional digits beyond `decimals`.
//   - A zero result is allowed and returned as "0".
//
// Examples:
//
//	ParseDollarAmount("1.50",  6) → "1500000", nil
//	ParseDollarAmount("0.01",  2) → "1", nil
//	ParseDollarAmount("100",   0) → "100", nil
func ParseDollarAmount(amount string, decimals uint32) (string, error) {
	amount = strings.TrimSpace(amount)
	if amount == "" {
		return "", fmt.Errorf("ParseDollarAmount: amount is empty")
	}

	// Split on decimal point — allow at most one.
	parts := strings.Split(amount, ".")
	if len(parts) > 2 {
		return "", fmt.Errorf("ParseDollarAmount: invalid amount %q: multiple decimal points", amount)
	}

	intPart := parts[0]
	fracPart := ""
	if len(parts) == 2 {
		fracPart = parts[1]
	}

	// Validate all characters are digits.
	for _, c := range intPart {
		if c < '0' || c > '9' {
			return "", fmt.Errorf("ParseDollarAmount: invalid character %q in amount %q", c, amount)
		}
	}
	for _, c := range fracPart {
		if c < '0' || c > '9' {
			return "", fmt.Errorf("ParseDollarAmount: invalid character %q in amount %q", c, amount)
		}
	}

	// Require at least one digit.
	if intPart == "" && fracPart == "" {
		return "", fmt.Errorf("ParseDollarAmount: invalid amount %q: no digits", amount)
	}
	if intPart == "" {
		intPart = "0"
	}

	d := int(decimals)

	// Truncate or pad the fractional part to exactly `decimals` digits.
	if len(fracPart) > d {
		fracPart = fracPart[:d]
	} else {
		fracPart = fracPart + strings.Repeat("0", d-len(fracPart))
	}

	// Combine integer and padded fractional digits into one integer string.
	combined := intPart + fracPart

	// Strip leading zeros (keep at least one digit).
	combined = strings.TrimLeft(combined, "0")
	if combined == "" {
		combined = "0"
	}

	result := new(big.Int)
	if _, ok := result.SetString(combined, 10); !ok {
		return "", fmt.Errorf("ParseDollarAmount: could not parse combined value %q", combined)
	}

	return result.String(), nil
}
