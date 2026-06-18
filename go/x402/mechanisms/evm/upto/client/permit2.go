package client

import (
	"context"
	"fmt"
	"math/big"
	"time"

	"github.com/okx/payments/go/x402/mechanisms/evm"
	"github.com/okx/payments/go/x402/types"
)

// ExtraFacilitatorAddressKey is the PaymentRequirements.Extra key the server
// publishes (via the facilitator's GetExtra) so the client can sign the upto
// witness with the authorized facilitator address.
const ExtraFacilitatorAddressKey = "facilitatorAddress"

// CreateUptoPermit2Payload creates a signed upto Permit2 payment payload for
// the given PaymentRequirements.
//
// It reads `facilitatorAddress` from `requirements.Extra`, builds an
// UptoPermit2 authorization (with the witness carrying `to`, `facilitator`,
// `validAfter`), signs it over the Permit2 EIP-712 domain via the shared
// evm.SignPermit2Witness helper, and returns a wire-format PaymentPayload.
//
// Returns ErrMissingFacilitatorAddress when the server has not propagated the
// facilitator address into requirements.Extra. Returns ErrInvalidTimeWindow
// when the resolved deadline is not strictly after the resolved validAfter
// (which would otherwise produce an unsignable / unenforceable authorization).
func CreateUptoPermit2Payload(
	ctx context.Context,
	signer evm.ClientEvmSigner,
	requirements types.PaymentRequirements,
) (types.PaymentPayload, error) {
	// Read facilitator address from requirements.Extra. The key MUST match the
	// one written by upto/server/scheme.go EnhancePaymentRequirements (and
	// upto/facilitator/scheme.go GetExtra).
	facilitatorAddress, _ := requirements.Extra[ExtraFacilitatorAddressKey].(string)
	if facilitatorAddress == "" {
		return types.PaymentPayload{}, fmt.Errorf(
			"%s: upto scheme requires %q in paymentRequirements.extra; ensure the server is configured with an upto facilitator that provides getExtra()",
			ErrMissingFacilitatorAddress,
			ExtraFacilitatorAddressKey,
		)
	}

	networkStr := requirements.Network

	// Get chain ID
	chainID, err := evm.GetEvmChainId(networkStr)
	if err != nil {
		return types.PaymentPayload{}, err
	}

	// Create nonce (uint256)
	nonce, err := evm.CreatePermit2Nonce()
	if err != nil {
		return types.PaymentPayload{}, err
	}

	// validAfter has a 10 min clock-skew buffer back from now;
	// deadline is now + maxTimeoutSeconds.
	now := time.Now().Unix()
	validAfter := fmt.Sprintf("%d", now-600)
	deadline := fmt.Sprintf("%d", now+int64(requirements.MaxTimeoutSeconds))

	// Guard against deadline <= validAfter, which stops a 0/negative
	// MaxTimeoutSeconds from silently producing an authorization that fails
	// on-chain.
	deadlineBig, _ := new(big.Int).SetString(deadline, 10)
	validAfterBig, _ := new(big.Int).SetString(validAfter, 10)
	if deadlineBig != nil && validAfterBig != nil && deadlineBig.Cmp(validAfterBig) <= 0 {
		return types.PaymentPayload{}, fmt.Errorf(
			"%s: deadline (%s) must be strictly after validAfter (%s); check that maxTimeoutSeconds (%d) is positive",
			ErrInvalidTimeWindow,
			deadline,
			validAfter,
			requirements.MaxTimeoutSeconds,
		)
	}

	// Validate addresses before signing — malformed values would otherwise be
	// committed into the Permit2 witness as the wrong (or zero) address.
	if !evm.IsValidAddress(requirements.Asset) {
		return types.PaymentPayload{}, fmt.Errorf("%s: asset %q is not a valid EVM address",
			ErrInvalidAddress, requirements.Asset)
	}
	if !evm.IsValidAddress(requirements.PayTo) {
		return types.PaymentPayload{}, fmt.Errorf("%s: payTo %q is not a valid EVM address",
			ErrInvalidAddress, requirements.PayTo)
	}
	if !evm.IsValidAddress(facilitatorAddress) {
		return types.PaymentPayload{}, fmt.Errorf("%s: facilitator %q is not a valid EVM address",
			ErrInvalidAddress, facilitatorAddress)
	}

	// Normalize addresses to EIP-55 checksummed form.
	tokenAddress := evm.NormalizeAddress(requirements.Asset)
	payTo := evm.NormalizeAddress(requirements.PayTo)
	facilitator := evm.NormalizeAddress(facilitatorAddress)

	// Build the upto Permit2 authorization. The spender is the upto proxy.
	authorization := evm.UptoPermit2Authorization{
		From: signer.Address(),
		Permitted: evm.Permit2TokenPermissions{
			Token:  tokenAddress,
			Amount: requirements.Amount,
		},
		Spender:  evm.X402UptoPermit2ProxyAddress,
		Nonce:    nonce,
		Deadline: deadline,
		Witness: evm.UptoPermit2Witness{
			To:          payTo,
			Facilitator: facilitator,
			ValidAfter:  validAfter,
		},
	}

	// Sign via the shared Permit2 typed-data helper. The Authorization value
	// itself implements the evm.Witness interface (via UptoPermit2Authorization
	// methods in mechanisms/evm/permit2.go), so the helper builds the upto
	// witness EIP-712 typed data and signs it.
	signature, _, err := evm.SignPermit2Witness(
		ctx,
		signer,
		authorization,
		evm.DefaultPermit2Domain(),
		chainID,
	)
	if err != nil {
		return types.PaymentPayload{}, fmt.Errorf("%s: %w", ErrFailedToSignPermit2Authorization, err)
	}

	// Marshal into the wire-format PaymentPayload. The map produced by ToMap()
	// matches the TS upto/client output byte-for-byte (signature + nested
	// permit2Authorization with witness.facilitator).
	uptoPayload := &evm.UptoPermit2Payload{
		Signature:            evm.BytesToHex(signature),
		Permit2Authorization: authorization,
	}

	return types.PaymentPayload{
		X402Version: 2,
		Payload:     uptoPayload.ToMap(),
	}, nil
}
