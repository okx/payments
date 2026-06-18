package client

// Client error constants for the upto EVM scheme (V2). The strings used in
// `Errorf` calls below are the canonical reasons the upto client can return.
const (
	// ErrFailedToSignPermit2Authorization is returned when the EIP-712 typed
	// data signing call for the upto Permit2 PermitWitnessTransferFrom
	// message fails.
	ErrFailedToSignPermit2Authorization = "invalid_upto_evm_client_failed_to_sign_permit2_authorization"

	// ErrMissingFacilitatorAddress is returned when the server-provided
	// PaymentRequirements.Extra does not include `facilitatorAddress`. The
	// upto witness signs over the facilitator address so the client cannot
	// produce a valid payload without it.
	ErrMissingFacilitatorAddress = "invalid_upto_evm_client_missing_facilitator_address"

	// ErrInvalidTimeWindow is returned when the computed deadline is not
	// strictly after the computed validAfter, which would otherwise produce
	// an unsignable / unenforceable authorization.
	ErrInvalidTimeWindow = "invalid_upto_evm_client_invalid_time_window"

	// ErrInvalidAddress is returned when an address field on the incoming
	// PaymentRequirements (asset, payTo) or the facilitator address fails
	// EIP-55 / hex-shape validation. Signing a malformed address would commit
	// the wrong token, recipient, or facilitator into the Permit2 witness.
	ErrInvalidAddress = "invalid_upto_evm_client_invalid_address"
)
