package evm

import (
	"encoding/json"
	"fmt"
)

// parseSessionDetails is a shared helper that silently returns a zero-value
// EVMSessionMethodDetails on parse failure. Used by the optional-field accessors
// that return nil/false/zero rather than errors.
func parseSessionDetails(methodDetails json.RawMessage) *EVMSessionMethodDetails {
	var d EVMSessionMethodDetails
	if err := json.Unmarshal(methodDetails, &d); err != nil {
		return nil
	}
	return &d
}

// SessionEscrowContract extracts the escrow contract address from session method_details.
// Returns an error if the JSON cannot be decoded or the field is absent/empty.
func SessionEscrowContract(methodDetails json.RawMessage) (string, error) {
	d, err := ParseEVMSessionMethodDetails(methodDetails)
	if err != nil {
		return "", fmt.Errorf("evm: SessionEscrowContract: %w", err)
	}
	return d.EscrowContract, nil
}

// SessionChannelID extracts the optional channel ID from session method_details.
// Returns nil if the field is absent or the JSON cannot be decoded.
func SessionChannelID(methodDetails json.RawMessage) *string {
	d := parseSessionDetails(methodDetails)
	if d == nil {
		return nil
	}
	return d.ChannelID
}

// SessionMinVoucherDelta extracts the optional minimum voucher delta from session
// method_details. Returns nil if the field is absent or the JSON cannot be decoded.
func SessionMinVoucherDelta(methodDetails json.RawMessage) *string {
	d := parseSessionDetails(methodDetails)
	if d == nil {
		return nil
	}
	return d.MinVoucherDelta
}

// SessionChainID extracts the optional chain ID from session method_details.
// Returns nil if the field is absent or the JSON cannot be decoded.
func SessionChainID(methodDetails json.RawMessage) *uint64 {
	d := parseSessionDetails(methodDetails)
	if d == nil {
		return nil
	}
	return d.ChainID
}

// SessionFeePayer reports whether the fee-payer flag is enabled in session
// method_details. Returns false if the field is absent or the JSON cannot be decoded.
func SessionFeePayer(methodDetails json.RawMessage) bool {
	d := parseSessionDetails(methodDetails)
	if d == nil {
		return false
	}
	return d.IsFeePayer()
}
