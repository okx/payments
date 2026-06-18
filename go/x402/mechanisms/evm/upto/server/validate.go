package server

import (
	"errors"
	"fmt"
	"math/big"
	"strings"
	"time"

	"github.com/ethereum/go-ethereum/common"

	"github.com/okx/payments/go/x402/mechanisms/evm"
	"github.com/okx/payments/go/x402/types"
)

// ValidateUptoPayload performs structural / cross-field validation of an
// incoming X-PAYMENT body against the matched PaymentRequirements for the
// upto scheme. It is the server-side gate that runs before the payload is
// forwarded to the facilitator for on-chain simulation.
//
// Validation steps (in order):
//
//  1. Scheme name on the payload's accepted.scheme must equal "upto".
//  2. Network on accepted must equal requirements.Network.
//  3. Payload must structurally look like an upto Permit2 payload
//     (witness.facilitator is the discriminator).
//  4. Permit2 spender must be X402UptoPermit2ProxyAddress.
//  5. Witness `to` must equal requirements.PayTo.
//  6. Witness `facilitator` must equal requirements.Extra["facilitatorAddress"].
//  7. Token (permitted.token) must equal requirements.Asset.
//  8. Permitted.amount must EQUAL requirements.Amount. At verify time
//     requirements.Amount is the signed cap (permitted.amount); the settle
//     path re-verifies with requirements.Amount overridden to permitted.amount
//     and enforces the actual settlement amount ≤ cap separately.
//  9. Deadline (Permit2 deadline) must be at least now+buffer in the future
//     (buffer covers verify→settle clock skew) and validAfter must not be in
//     the future.
//  10. Signature must be a syntactically valid 65-byte EOA signature that
//     recovers to the payload's `from` address over the upto Permit2 EIP-712
//     typed data for the given chain.
//
// Returns nil on success. On failure, returns an error whose Error() contains
// one of the sentinel error reason constants declared in errors.go (so
// callers can compare with errors.Is via strings.Contains or by wrapping with
// a sentinel — equality is on the reason string, not Go error identity).
//
// This helper is intentionally facilitator-free: it does no RPC, no on-chain
// state probing. The facilitator (stage 6) will run the simulation + ERC-1271
// fallback on top of this.
func ValidateUptoPayload(
	payload types.PaymentPayload,
	requirements types.PaymentRequirements,
) error {
	// Step 1: scheme must be "upto".
	if payload.Accepted.Scheme != evm.SchemeUpto {
		return fmt.Errorf("%s: payload.accepted.scheme = %q, want %q",
			ErrUptoInvalidScheme, payload.Accepted.Scheme, evm.SchemeUpto)
	}
	if requirements.Scheme != evm.SchemeUpto {
		return fmt.Errorf("%s: requirements.scheme = %q, want %q",
			ErrUptoInvalidScheme, requirements.Scheme, evm.SchemeUpto)
	}

	// Step 2: network match.
	if payload.Accepted.Network != requirements.Network {
		return fmt.Errorf("%s: payload network %q != requirements network %q",
			ErrUptoNetworkMismatch, payload.Accepted.Network, requirements.Network)
	}

	// Step 3: structural upto payload guard. Returns the parse error
	// directly so callers can see which field was malformed.
	if payload.Payload == nil {
		return errors.New(ErrUptoInvalidPayloadFormat + ": payload.payload is nil")
	}
	if !evm.IsUptoPermit2Payload(payload.Payload) {
		return errors.New(ErrUptoInvalidPayloadFormat + ": payload does not match upto Permit2 shape (missing witness.facilitator?)")
	}
	upto, err := evm.UptoPermit2PayloadFromMap(payload.Payload)
	if err != nil {
		return fmt.Errorf("%s: %w", ErrUptoInvalidPayloadFormat, err)
	}

	auth := upto.Permit2Authorization

	// Step 4: spender must be the upto proxy.
	if !equalAddress(auth.Spender, evm.X402UptoPermit2ProxyAddress) {
		return fmt.Errorf("%s: spender %q != X402UptoPermit2ProxyAddress %q",
			ErrPermit2InvalidSpender, auth.Spender, evm.X402UptoPermit2ProxyAddress)
	}

	// Step 5: witness.to must equal requirements.PayTo.
	if !equalAddress(auth.Witness.To, requirements.PayTo) {
		return fmt.Errorf("%s: witness.to %q != requirements.payTo %q",
			ErrPermit2RecipientMismatch, auth.Witness.To, requirements.PayTo)
	}

	// Step 6: witness.facilitator must equal requirements.Extra["facilitatorAddress"].
	requiredFacilitator, _ := requirements.Extra[ExtraFacilitatorAddressKey].(string)
	if requiredFacilitator == "" {
		// The server should ALWAYS publish a facilitator address in upto
		// requirements. If we got here without one the server is misconfigured
		// — fail closed.
		return fmt.Errorf("%s: requirements.extra.%s is empty or missing",
			ErrUptoFacilitatorMismatch, ExtraFacilitatorAddressKey)
	}
	if !equalAddress(auth.Witness.Facilitator, requiredFacilitator) {
		return fmt.Errorf("%s: witness.facilitator %q != requirements.extra.facilitatorAddress %q",
			ErrUptoFacilitatorMismatch, auth.Witness.Facilitator, requiredFacilitator)
	}

	// Step 7: token (permitted.token) must equal requirements.Asset.
	if !equalAddress(auth.Permitted.Token, requirements.Asset) {
		return fmt.Errorf("%s: permitted.token %q != requirements.asset %q",
			ErrPermit2TokenMismatch, auth.Permitted.Token, requirements.Asset)
	}

	// Step 8: amount must match exactly. requirements.Amount is the signed cap
	// at verify time (and the settle path overrides it to permitted.amount
	// before calling this).
	permittedAmt, ok := new(big.Int).SetString(auth.Permitted.Amount, 10)
	if !ok || permittedAmt.Sign() < 0 {
		return fmt.Errorf("%s: permitted.amount %q is not a non-negative integer",
			ErrPermit2InvalidAmount, auth.Permitted.Amount)
	}
	reqAmt, ok := new(big.Int).SetString(requirements.Amount, 10)
	if !ok || reqAmt.Sign() < 0 {
		return fmt.Errorf("%s: requirements.amount %q is not a non-negative integer",
			ErrPermit2InvalidAmount, requirements.Amount)
	}
	if reqAmt.Cmp(permittedAmt) != 0 {
		return fmt.Errorf("%s: requirements.amount (%s) != permitted.amount (%s)",
			ErrPermit2AmountMismatch, reqAmt.String(), permittedAmt.String())
	}

	// Step 9: deadline / validAfter window.
	now := time.Now().Unix()
	deadlineBig, ok := new(big.Int).SetString(auth.Deadline, 10)
	if !ok {
		return fmt.Errorf("%s: deadline %q is not a decimal integer",
			ErrPermit2DeadlineExpired, auth.Deadline)
	}
	if deadlineBig.Cmp(big.NewInt(now+int64(evm.Permit2DeadlineBuffer))) < 0 {
		return fmt.Errorf("%s: deadline %s < now+buffer (%d+%ds)",
			ErrPermit2DeadlineExpired, deadlineBig.String(), now, evm.Permit2DeadlineBuffer)
	}
	validAfterBig, ok := new(big.Int).SetString(auth.Witness.ValidAfter, 10)
	if !ok {
		return fmt.Errorf("%s: witness.validAfter %q is not a decimal integer",
			ErrPermit2NotYetValid, auth.Witness.ValidAfter)
	}
	if validAfterBig.Cmp(big.NewInt(now)) > 0 {
		return fmt.Errorf("%s: witness.validAfter %s > now %d",
			ErrPermit2NotYetValid, validAfterBig.String(), now)
	}

	// Step 10: signature must recover to auth.From over the upto Permit2
	// typed data for the chain in requirements.Network. EOA-only here —
	// ERC-1271 / smart-account fallback runs on the facilitator side (stage 6).
	chainID, err := evm.GetEvmChainId(requirements.Network)
	if err != nil {
		return fmt.Errorf("%s: %s", ErrUptoNetworkMismatch, err.Error())
	}
	if !evm.IsValidAddress(auth.From) {
		return fmt.Errorf("%s: from %q is not a valid address",
			ErrPermit2InvalidOwner, auth.From)
	}
	sigBytes, err := evm.HexToBytes(upto.Signature)
	if err != nil {
		return fmt.Errorf("%s: signature hex decode: %s",
			ErrPermit2InvalidSignature, err.Error())
	}
	if len(sigBytes) != 65 {
		return fmt.Errorf("%s: signature length = %d, want 65",
			ErrPermit2InvalidSignature, len(sigBytes))
	}
	expected := common.HexToAddress(auth.From)
	ok2, err := evm.VerifyPermit2Witness(expected, sigBytes, auth, evm.DefaultPermit2Domain(), chainID)
	if err != nil {
		return fmt.Errorf("%s: %s", ErrPermit2InvalidSignature, err.Error())
	}
	if !ok2 {
		return fmt.Errorf("%s: signature does not recover to from=%s",
			ErrPermit2InvalidSignature, auth.From)
	}

	return nil
}

// equalAddress reports whether two hex addresses refer to the same Ethereum
// address. Comparison is case-insensitive on the hex characters and tolerates
// a missing 0x prefix. Empty strings are treated as unequal to any address
// to avoid an empty payload field silently passing validation.
func equalAddress(a, b string) bool {
	if a == "" || b == "" {
		return false
	}
	return strings.EqualFold(evm.NormalizeAddress(a), evm.NormalizeAddress(b))
}
