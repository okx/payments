// Package server provides the deferred EVM payment scheme for x402 sellers.
//
// From the seller's perspective, the deferred scheme behaves identically to
// "exact" for building payment requirements — the difference is handled
// entirely by the Facilitator during verify/settle. The Facilitator converts
// session-key signatures to EOA signatures and batches settlements on-chain.
package server

import (
	"context"

	x402 "github.com/okx/payments/go"
	"github.com/okx/payments/go/mechanisms/evm"
	exact "github.com/okx/payments/go/mechanisms/evm/exact/server"
	"github.com/okx/payments/go/types"
)

// AggrDeferredEvmScheme implements SchemeNetworkServer for deferred EVM payments.
// It delegates all logic to ExactEvmScheme, differing only in scheme name.
type AggrDeferredEvmScheme struct {
	exact *exact.ExactEvmScheme
}

// NewAggrDeferredEvmScheme creates a new AggrDeferredEvmScheme.
func NewAggrDeferredEvmScheme() *AggrDeferredEvmScheme {
	return &AggrDeferredEvmScheme{
		exact: exact.NewExactEvmScheme(),
	}
}

// Scheme returns "aggr_deferred".
func (s *AggrDeferredEvmScheme) Scheme() string {
	return evm.SchemeAggrDeferred
}

// ParsePrice delegates to the exact scheme's ParsePrice.
func (s *AggrDeferredEvmScheme) ParsePrice(price x402.Price, network x402.Network) (x402.AssetAmount, error) {
	return s.exact.ParsePrice(price, network)
}

// EnhancePaymentRequirements delegates to the exact scheme's EnhancePaymentRequirements.
func (s *AggrDeferredEvmScheme) EnhancePaymentRequirements(
	ctx context.Context,
	requirements types.PaymentRequirements,
	supportedKind types.SupportedKind,
	extensions []string,
) (types.PaymentRequirements, error) {
	return s.exact.EnhancePaymentRequirements(ctx, requirements, supportedKind, extensions)
}
