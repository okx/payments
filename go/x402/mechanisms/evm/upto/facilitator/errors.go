// Package facilitator implements the upto EVM facilitator scheme.
//
// The error reason constants in this file are part of the wire contract —
// clients compare against these strings, so they must not be reworded.
//
// Two groups:
//
//  1. Upto-specific sentinels (`invalid_upto_evm_*` / `upto_*`).
//
//  2. Permit2 / EIP-3009 / ERC-20-approval sentinels that the upto facilitator
//     surfaces verbatim, duplicated locally so this package does not import
//     `mechanisms/evm/exact/facilitator` (keeps the dependency graph flat).
package facilitator

const (
	// ---- Upto-specific sentinels ----

	// ErrUptoInvalidScheme — payload.accepted.scheme or requirements.scheme is
	// not "upto".
	ErrUptoInvalidScheme = "invalid_upto_evm_scheme"

	// ErrUptoNetworkMismatch — payload network does not match requirements network.
	ErrUptoNetworkMismatch = "invalid_upto_evm_network_mismatch"

	// ErrUptoSettlementExceedsAmount — settlement amount exceeds the
	// authorization's permitted amount.
	ErrUptoSettlementExceedsAmount = "invalid_upto_evm_payload_settlement_exceeds_amount"

	// ErrUptoAmountExceedsPermitted — on-chain revert reason for AmountExceedsPermitted.
	ErrUptoAmountExceedsPermitted = "upto_amount_exceeds_permitted"

	// ErrUptoUnauthorizedFacilitator — on-chain revert reason for a settle
	// attempt by a signer that is not the witness.facilitator.
	ErrUptoUnauthorizedFacilitator = "upto_unauthorized_facilitator"

	// ErrUptoFacilitatorMismatch — payload witness.facilitator does not match
	// any facilitator signer address.
	ErrUptoFacilitatorMismatch = "upto_facilitator_mismatch"

	// ---- Shared payload-parse errors (Go-only; structural guards) ----

	// ErrUptoInvalidPayloadFormat — the X-PAYMENT body is not a recognizable
	// upto Permit2 payload (missing fields, wrong types, witness.facilitator
	// absent). Server-side parsing error.
	ErrUptoInvalidPayloadFormat = "invalid_upto_evm_payload_format"

	// ErrUnsupportedPayloadType — the payload is not an upto payload (e.g. is
	// an EIP-3009 or exact Permit2 payload).
	ErrUnsupportedPayloadType = "unsupported_payload_type"

	// ErrInvalidPayload — payload could not be parsed.
	ErrInvalidPayload = "invalid_upto_evm_payload"

	// ErrInvalidRequiredAmount — requirements.amount is not a non-negative decimal integer.
	ErrInvalidRequiredAmount = "invalid_upto_evm_required_amount"

	// ErrFailedToGetNetworkConfig — could not resolve chain ID from requirements.network.
	ErrFailedToGetNetworkConfig = "invalid_upto_evm_failed_to_get_network_config"

	// ErrInvalidSignatureFormat — signature hex is malformed (not a valid 0x-prefixed bytes).
	ErrInvalidSignatureFormat = "invalid_upto_evm_signature_format"

	// ErrVerificationFailed — settle's internal verify call returned an error
	// not matchable to a more specific reason. Mirrors exact's
	// invalid_exact_evm_verification_failed.
	ErrVerificationFailed = "invalid_upto_evm_verification_failed"

	// ErrFailedToGetReceipt — failed to fetch transaction receipt after settle.
	ErrFailedToGetReceipt = "invalid_upto_evm_failed_to_get_receipt"

	// ErrTransactionFailed — settle's broadcast transaction reverted on-chain.
	ErrTransactionFailed = "invalid_upto_evm_transaction_failed"

	// ---- Permit2 sentinels (exact-scheme equivalents) ----
	// These are surfaced by the upto facilitator and must match
	// exact/facilitator/errors.go exactly. Duplicated here to keep the upto
	// package free of upward dependency on the exact package.

	ErrPermit2InvalidSpender     = "invalid_permit2_spender"
	ErrPermit2RecipientMismatch  = "invalid_permit2_recipient_mismatch"
	ErrPermit2DeadlineExpired    = "permit2_deadline_expired"
	ErrPermit2NotYetValid        = "permit2_not_yet_valid"
	ErrPermit2AmountMismatch     = "permit2_amount_mismatch"
	ErrPermit2TokenMismatch      = "permit2_token_mismatch"
	ErrPermit2InvalidSignature   = "invalid_permit2_signature"
	ErrPermit2AllowanceRequired  = "permit2_allowance_required"
	ErrPermit2InvalidAmount      = "permit2_invalid_amount"
	ErrPermit2InvalidDestination = "permit2_invalid_destination"
	ErrPermit2InvalidOwner       = "permit2_invalid_owner"
	ErrPermit2PaymentTooEarly    = "permit2_payment_too_early"
	ErrPermit2InvalidNonce       = "permit2_invalid_nonce"
	ErrPermit2612AmountMismatch  = "permit2_2612_amount_mismatch"

	// Permit2 simulation / diagnostic errors.
	ErrPermit2SimulationFailed    = "permit2_simulation_failed"
	ErrPermit2InsufficientBalance = "permit2_insufficient_balance"
	ErrPermit2ProxyNotDeployed    = "permit2_proxy_not_deployed"

	// ErrFailedToExecuteTransfer — generic on-chain settle failure when no
	// specific revert reason matched.
	ErrFailedToExecuteTransfer = "invalid_upto_evm_failed_to_execute_transfer"
)
