package store

import (
	"context"
	"fmt"
	"math/big"
)

// ChannelState holds the full state of a payment channel.
type ChannelState struct {
	ChannelID               string   `json:"channelId"`
	ChainID                 uint64   `json:"chainId"`
	EscrowContract          string   `json:"escrowContract"`
	Payer                   string   `json:"payer"`
	Payee                   string   `json:"payee"`
	Token                   string   `json:"token"`
	AuthorizedSigner        string   `json:"authorizedSigner"`
	Deposit                 *big.Int `json:"deposit"`
	HighestVoucherAmount    *big.Int `json:"highestVoucherAmount"`
	HighestVoucherSignature []byte   `json:"highestVoucherSignature,omitempty"`
	MinVoucherDelta         *big.Int `json:"minVoucherDelta,omitempty"`
	Spent                   *big.Int `json:"spent"`
	Units                   uint64   `json:"units"`
	Finalized               bool     `json:"finalized"`
	CloseRequestedAt        uint64   `json:"closeRequestedAt"`
	CreatedAt               string   `json:"createdAt"`
}

// OnChainChannel represents the on-chain view of a payment channel.
type OnChainChannel struct {
	Payer            string   `json:"payer"`
	Payee            string   `json:"payee"`
	Token            string   `json:"token"`
	AuthorizedSigner string   `json:"authorizedSigner"`
	Deposit          *big.Int `json:"deposit"`
	Settled          *big.Int `json:"settled"`
	CloseRequestedAt uint64   `json:"closeRequestedAt"`
	Finalized        bool     `json:"finalized"`
}

// DeductFromChannel atomically deducts amount from the available balance.
// Reads the channel, checks constraints, updates Spent, and writes back.
// The caller must hold any necessary lock for atomicity.
func DeductFromChannel(ctx context.Context, s Store[ChannelState], channelID string, amount *big.Int) (*ChannelState, error) {
	cs, err := s.Get(ctx, channelID)
	if err != nil {
		return nil, err
	}
	if cs == nil {
		return nil, fmt.Errorf("channel %q not found", channelID)
	}
	if cs.Finalized {
		return nil, fmt.Errorf("channel %q is finalized", channelID)
	}
	if cs.CloseRequestedAt != 0 {
		return nil, fmt.Errorf("channel %q has a pending close request", channelID)
	}

	ceiling := cs.HighestVoucherAmount
	if ceiling == nil {
		ceiling = new(big.Int)
	}
	spent := cs.Spent
	if spent == nil {
		spent = new(big.Int)
	}

	available := new(big.Int).Sub(ceiling, spent)
	if available.Cmp(amount) < 0 {
		return nil, fmt.Errorf("insufficient balance in channel %q: available %s, requested %s",
			channelID, available.String(), amount.String())
	}

	cs.Spent = new(big.Int).Add(spent, amount)
	cs.Units++

	if err := s.Put(ctx, channelID, cs); err != nil {
		return nil, err
	}
	return cs, nil
}
