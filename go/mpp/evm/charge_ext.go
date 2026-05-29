package evm

import (
	"encoding/json"
	"fmt"
	"math/big"
)

// ParseEVMMethodDetails parses method_details JSON into EVMMethodDetails.
func ParseEVMMethodDetails(methodDetails json.RawMessage) (*EVMMethodDetails, error) {
	var d EVMMethodDetails
	if err := json.Unmarshal(methodDetails, &d); err != nil {
		return nil, fmt.Errorf("evm: parse method_details: %w", err)
	}
	return &d, nil
}

// ParseEVMSessionMethodDetails parses method_details JSON into EVMSessionMethodDetails.
func ParseEVMSessionMethodDetails(methodDetails json.RawMessage) (*EVMSessionMethodDetails, error) {
	var d EVMSessionMethodDetails
	if err := json.Unmarshal(methodDetails, &d); err != nil {
		return nil, fmt.Errorf("evm: parse session method_details: %w", err)
	}
	if d.EscrowContract == "" {
		return nil, fmt.Errorf("evm: session method_details missing required field escrowContract")
	}
	return &d, nil
}

// GetTransfers computes the ordered list of Transfer values for a charge.
//
// The primary recipient receives totalAmount minus the sum of all split amounts.
// Split amounts are validated as non-negative decimal integers. The sum of splits
// must be strictly less than totalAmount (the primary leg must be positive). The
// number of splits must not exceed MaxSplits.
//
// Returned slice order: primary transfer first, followed by splits in input order.
func GetTransfers(totalAmount *big.Int, primaryRecipient string, primaryMemo *[32]byte, splits []Split) ([]Transfer, error) {
	if totalAmount == nil {
		return nil, fmt.Errorf("evm: totalAmount must not be nil")
	}
	if len(splits) > MaxSplits {
		return nil, fmt.Errorf("evm: number of splits %d exceeds maximum %d", len(splits), MaxSplits)
	}

	type parsedSplit struct {
		amount    *big.Int
		recipient string
		memo      *[32]byte
	}
	parsed := make([]parsedSplit, 0, len(splits))

	splitTotal := new(big.Int)
	for i, s := range splits {
		amt, err := ParseAmount(s.Amount)
		if err != nil {
			return nil, fmt.Errorf("evm: split[%d] invalid amount: %w", i, err)
		}
		addr, err := ParseAddress(s.Recipient)
		if err != nil {
			return nil, fmt.Errorf("evm: split[%d] invalid recipient: %w", i, err)
		}
		var memo *[32]byte
		if s.Memo != nil {
			m, err := ParseMemoBytes(*s.Memo)
			if err != nil {
				return nil, fmt.Errorf("evm: split[%d] invalid memo: %w", i, err)
			}
			memo = &m
		}
		splitTotal.Add(splitTotal, amt)
		parsed = append(parsed, parsedSplit{amount: amt, recipient: addr, memo: memo})
	}

	// Sum of splits must be strictly less than total so the primary leg is positive.
	if splitTotal.Cmp(totalAmount) >= 0 {
		return nil, fmt.Errorf("evm: sum of split amounts (%s) must be less than totalAmount (%s)", splitTotal.String(), totalAmount.String())
	}

	primaryAmount := new(big.Int).Sub(totalAmount, splitTotal)

	result := make([]Transfer, 0, 1+len(parsed))
	result = append(result, Transfer{
		Amount:    primaryAmount,
		Recipient: primaryRecipient,
		Memo:      primaryMemo,
	})
	for _, p := range parsed {
		result = append(result, Transfer{
			Amount:    p.amount,
			Recipient: p.recipient,
			Memo:      p.memo,
		})
	}
	return result, nil
}
