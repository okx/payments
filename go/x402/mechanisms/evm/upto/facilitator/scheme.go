// Package facilitator provides the facilitator-side implementation of the
// upto EVM payment scheme.
//
// The upto facilitator owns:
//   - Verify: off-chain payload validation (via upto/server.ValidateUptoPayload)
//   - on-chain simulation against x402UptoPermit2Proxy.settle().
//   - Settle: building & broadcasting the settle() call on x402UptoPermit2Proxy.
//
// Construction pattern (mirrors exact/facilitator):
//
//	signer := ... // FacilitatorEvmSigner
//	facil := x402.Newx402Facilitator()
//	facil.Register([]x402.Network{"eip155:8453"},
//	    uptofac.NewUptoEvmScheme(signer, nil))
package facilitator

import (
	"context"
	"fmt"

	"github.com/okx/payments/go/x402"
	"github.com/okx/payments/go/x402/mechanisms/evm"
	"github.com/okx/payments/go/x402/types"
)

// UptoEvmSchemeConfig holds optional settle-time configuration for the upto
// EVM facilitator. Parallel to exact/facilitator.ExactEvmSchemeConfig — kept
// minimal here because the upto base flow has no smart-wallet auto-deploy /
// EIP-6492 path.
type UptoEvmSchemeConfig struct {
	// SimulateInSettle reruns simulation (eth_call pre-flight) during settle.
	// Defaults to true when nil. Set to a pointer to false to opt out (e.g.
	// when verify already simulated and the gap is acceptable).
	SimulateInSettle *bool
}

// UptoEvmScheme implements the x402.SchemeNetworkFacilitator interface for the
// upto EVM payment scheme.
type UptoEvmScheme struct {
	signer evm.FacilitatorEvmSigner
	config UptoEvmSchemeConfig
}

// NewUptoEvmScheme creates a new UptoEvmScheme. The signer is required — it
// supplies the facilitator's signing addresses (one of which must be the
// `witness.facilitator` the client signed over) plus the RPC primitives used
// for verify / settle.
//
// Pass `config = nil` to accept the defaults (SimulateInSettle defaults to true).
func NewUptoEvmScheme(signer evm.FacilitatorEvmSigner, config *UptoEvmSchemeConfig) *UptoEvmScheme {
	cfg := UptoEvmSchemeConfig{}
	if config != nil {
		cfg = *config
	}
	return &UptoEvmScheme{
		signer: signer,
		config: cfg,
	}
}

// Scheme returns the scheme identifier. Always returns evm.SchemeUpto.
func (f *UptoEvmScheme) Scheme() string {
	return evm.SchemeUpto
}

// CaipFamily returns the CAIP family pattern this facilitator supports.
// Mirrors exact-scheme — every EVM facilitator returns "eip155:*".
func (f *UptoEvmScheme) CaipFamily() string {
	return "eip155:*"
}

// GetExtra returns scheme-specific extra data for the supported kinds endpoint.
//
// For upto we publish the facilitator's first signer address under the key
// "facilitatorAddress". The server (`upto/server`) propagates this into
// `paymentRequirements.Extra.facilitatorAddress` so the client can sign the
// witness with it. The same key string is shared across client/server/facilitator
// (see upto/client.ExtraFacilitatorAddressKey + upto/server.ExtraFacilitatorAddressKey).
//
// Returns nil when the signer has no addresses.
func (f *UptoEvmScheme) GetExtra(_ x402.Network) map[string]interface{} {
	addresses := f.signer.GetAddresses()
	if len(addresses) == 0 {
		return nil
	}
	return map[string]interface{}{
		ExtraFacilitatorAddressKey: addresses[0],
	}
}

// GetSigners returns the facilitator's signing addresses for the given network.
func (f *UptoEvmScheme) GetSigners(_ x402.Network) []string {
	return f.signer.GetAddresses()
}

// Verify verifies an upto payment payload against requirements.
//
// Rejects non-upto payloads early with ErrUnsupportedPayloadType — payloads
// without `witness.facilitator` belong to the exact scheme.
func (f *UptoEvmScheme) Verify(
	ctx context.Context,
	payload types.PaymentPayload,
	requirements types.PaymentRequirements,
	fctx *x402.FacilitatorContext,
) (*x402.VerifyResponse, error) {
	if !evm.IsUptoPermit2Payload(payload.Payload) {
		return nil, x402.NewVerifyError(ErrUnsupportedPayloadType, "",
			"payload is not an upto Permit2 payload (missing witness.facilitator)")
	}

	uptoPayload, err := evm.UptoPermit2PayloadFromMap(payload.Payload)
	if err != nil {
		return nil, x402.NewVerifyError(ErrInvalidPayload, "",
			fmt.Sprintf("failed to parse upto Permit2 payload: %s", err.Error()))
	}

	return VerifyUptoPermit2(ctx, f.signer, payload, requirements, uptoPayload, fctx, nil)
}

// Settle settles an upto payment on-chain by calling x402UptoPermit2Proxy.settle(...).
//
// Rejects non-upto payloads early with ErrUnsupportedPayloadType.
func (f *UptoEvmScheme) Settle(
	ctx context.Context,
	payload types.PaymentPayload,
	requirements types.PaymentRequirements,
	fctx *x402.FacilitatorContext,
) (*x402.SettleResponse, error) {
	network := x402.Network(payload.Accepted.Network)

	if !evm.IsUptoPermit2Payload(payload.Payload) {
		return nil, x402.NewSettleError(ErrUnsupportedPayloadType, "", network, "",
			"payload is not an upto Permit2 payload (missing witness.facilitator)")
	}

	uptoPayload, err := evm.UptoPermit2PayloadFromMap(payload.Payload)
	if err != nil {
		return nil, x402.NewSettleError(ErrInvalidPayload, "", network, "",
			fmt.Sprintf("failed to parse upto Permit2 payload: %s", err.Error()))
	}

	// f.config.SimulateInSettle is a *bool (nil ⇒ default true), passed through verbatim.
	return SettleUptoPermit2(ctx, f.signer, payload, requirements, uptoPayload, fctx,
		&UptoPermit2FacilitatorConfig{SimulateInSettle: f.config.SimulateInSettle})
}

// ExtraFacilitatorAddressKey is the `paymentRequirements.Extra` key the upto
// facilitator publishes. Mirrors the same key used by client/server. Kept
// inline (not imported) to keep this package free of upward dependencies on
// the upto/client or upto/server packages.
const ExtraFacilitatorAddressKey = "facilitatorAddress"
