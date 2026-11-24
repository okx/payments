package server

// Server error constants for the upto EVM scheme (V2).
//
// Two groups of constants live here:
//
//  1. Server-side price / requirements parsing errors (`invalid_upto_evm_server_*`)
//     — parallel to the exact/server `invalid_exact_evm_server_*` constants used
//     by ParsePrice + EnhancePaymentRequirements.
//
//  2. Wire-contract validation sentinels (`invalid_upto_evm_*` / `upto_*`).
//     The Go upto server uses these when validating an incoming X-PAYMENT
//     payload against requirements (see validate.go). The same strings are
//     re-used by the upto facilitator so error reasons stay stable on the wire.

const (
	// ---- Group 1: server-side parsing errors ----

	ErrAmountMustBeString    = "invalid_upto_evm_server_amount_must_be_string"
	ErrAssetAddressRequired  = "invalid_upto_evm_server_asset_address_required"
	ErrFailedToParsePrice    = "invalid_upto_evm_server_failed_to_parse_price"
	ErrUnsupportedPriceType  = "invalid_upto_evm_server_unsupported_price_type"
	ErrFailedToConvertAmount = "invalid_upto_evm_server_failed_to_convert_amount"
	ErrNoAssetSpecified      = "invalid_upto_evm_server_no_asset_specified"
	ErrFailedToParseAmount   = "invalid_upto_evm_server_failed_to_parse_amount"
	ErrInvalidPayToAddress   = "invalid_upto_evm_server_invalid_payto_address"
	ErrAmountRequired        = "invalid_upto_evm_server_amount_required"
	ErrInvalidAmount         = "invalid_upto_evm_server_invalid_amount"
	ErrInvalidAsset          = "invalid_upto_evm_server_invalid_asset"
	ErrInvalidTokenAmount    = "invalid_upto_evm_server_invalid_token_amount"

	// ---- Group 2: wire-contract validation sentinels ----

	// ErrUptoInvalidScheme — payload.scheme (or accepted.scheme) is not "upto".
	ErrUptoInvalidScheme = "invalid_upto_evm_scheme"

	// ErrUptoNetworkMismatch — payload network does not match requirements network.
	ErrUptoNetworkMismatch = "invalid_upto_evm_network_mismatch"

	// ErrUptoSettlementExceedsAmount — settlement amount exceeds the
	// authorization's permitted amount (`requirements.amount > permitted.amount`).
	ErrUptoSettlementExceedsAmount = "invalid_upto_evm_payload_settlement_exceeds_amount"

	// ErrUptoAmountExceedsPermitted — same numeric semantics as
	// ErrUptoSettlementExceedsAmount but uses the verbatim on-chain revert
	// reason.
	ErrUptoAmountExceedsPermitted = "upto_amount_exceeds_permitted"

	// ErrUptoUnauthorizedFacilitator — on-chain revert reason for a settle
	// attempt by a signer that is not the witness.facilitator.
	ErrUptoUnauthorizedFacilitator = "upto_unauthorized_facilitator"

	// ErrUptoFacilitatorMismatch — payload witness.facilitator does not match
	// the requirements.extra.facilitatorAddress published by the server.
	ErrUptoFacilitatorMismatch = "upto_facilitator_mismatch"

	// ---- Group 2 (cont.): shared Permit2 errors re-used by upto validation ----
	// These share the same-named constants in exact/facilitator/errors.go so
	// validate.go returns the canonical wire reason strings directly (no Go-only
	// prefix). They are declared here — rather than imported — to keep the upto
	// server package free of an upward dependency on exact/facilitator.
	//
	// Permit2 nonce replay is enforced on-chain by Permit2's unordered-nonce
	// bitmap, so there is intentionally no server-side ErrPermit2InvalidNonce
	// here (the facilitator surfaces the on-chain revert reason instead).

	ErrPermit2InvalidSpender     = "invalid_permit2_spender"
	ErrPermit2RecipientMismatch  = "invalid_permit2_recipient_mismatch"
	ErrPermit2DeadlineExpired    = "permit2_deadline_expired"
	ErrPermit2NotYetValid        = "permit2_not_yet_valid"
	ErrPermit2AmountMismatch     = "permit2_amount_mismatch"
	ErrPermit2TokenMismatch      = "permit2_token_mismatch"
	ErrPermit2InvalidSignature   = "invalid_permit2_signature"
	ErrPermit2InvalidAmount      = "permit2_invalid_amount"
	ErrPermit2InvalidDestination = "permit2_invalid_destination"
	ErrPermit2InvalidOwner       = "permit2_invalid_owner"

	// ErrUptoInvalidPayloadFormat — the X-PAYMENT body is not a recognizable
	// upto Permit2 payload (missing fields, wrong types, witness.facilitator
	// absent). Server-side parsing error.
	ErrUptoInvalidPayloadFormat = "invalid_upto_evm_payload_format"
)
