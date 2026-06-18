// Package client provides the client-side scheme implementation for the upto
// EVM payment scheme.
//
// The upto scheme is Permit2-only: every payment is built and signed as a
// PermitWitnessTransferFrom message with the upto witness (which includes the
// `facilitator` field). The base flow is implemented in CreateUptoPermit2Payload;
// this file ties that helper to the x402.SchemeNetworkClient interface so
// callers can register the scheme on an x402Client.
//
// Construction pattern matches the exact-scheme client:
//
//	client := x402.Newx402Client()
//	client.Register("eip155:*", uptoclient.NewUptoEvmScheme(signer))
package client

import (
	"context"

	"github.com/okx/payments/go/x402/mechanisms/evm"
	"github.com/okx/payments/go/x402/types"
)

// UptoEvmScheme implements the x402.SchemeNetworkClient interface for the
// upto EVM payment scheme. Construction takes only a signer — the spender
// address (X402UptoPermit2ProxyAddress) and witness shape are determined by
// the scheme itself.
type UptoEvmScheme struct {
	signer evm.ClientEvmSigner
}

// NewUptoEvmScheme creates a new UptoEvmScheme. The signer is the only
// required dependency — it must implement evm.ClientEvmSigner (Address +
// SignTypedData).
//
// Unlike NewExactEvmScheme there is no second config arg: the upto base
// flow has no opt-in RPC extension surface in this stage.
func NewUptoEvmScheme(signer evm.ClientEvmSigner) *UptoEvmScheme {
	return &UptoEvmScheme{
		signer: signer,
	}
}

// Scheme returns the scheme identifier. Always returns evm.SchemeUpto ("upto").
func (c *UptoEvmScheme) Scheme() string {
	return evm.SchemeUpto
}

// CreatePaymentPayload creates a V2 payment payload for the upto scheme. The
// upto scheme always uses Permit2 (with the upto witness); there is no
// EIP-3009 fallback.
func (c *UptoEvmScheme) CreatePaymentPayload(
	ctx context.Context,
	requirements types.PaymentRequirements,
) (types.PaymentPayload, error) {
	return CreateUptoPermit2Payload(ctx, c.signer, requirements)
}
