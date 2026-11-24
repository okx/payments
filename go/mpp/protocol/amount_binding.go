package protocol

import "strings"

// CheckAmount compares canonical base-unit integers without narrowing to uint64.
func CheckAmount(actual, expected string) error {
	valid := func(value string) bool {
		if value == "" {
			return false
		}
		for _, digit := range value {
			if digit < '0' || digit > '9' {
				return false
			}
		}
		return true
	}
	if !valid(actual) || !valid(expected) || strings.TrimLeft(actual, "0") != strings.TrimLeft(expected, "0") {
		return NewVerificationErrorWithCode("credential amount does not match current route", ErrorCodeCredentialMismatch)
	}
	return nil
}
