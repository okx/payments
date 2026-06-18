package facilitator

import (
	"context"
	"errors"
	"fmt"
	"math/big"
	"strings"

	"github.com/ethereum/go-ethereum/common"

	"github.com/okx/payments/go/x402"
	"github.com/okx/payments/go/x402/mechanisms/evm"
	uptoserver "github.com/okx/payments/go/x402/mechanisms/evm/upto/server"
	"github.com/okx/payments/go/x402/types"
)

// VerifyUptoPermit2Options controls optional behaviour for VerifyUptoPermit2.
type VerifyUptoPermit2Options struct {
	// Simulate enables onchain simulation. Defaults to true when nil.
	Simulate *bool
}

func (o *VerifyUptoPermit2Options) shouldSimulate() bool {
	if o == nil || o.Simulate == nil {
		return true
	}
	return *o.Simulate
}

// UptoPermit2FacilitatorConfig holds optional settlement-time configuration.
type UptoPermit2FacilitatorConfig struct {
	// SimulateInSettle re-runs simulation (eth_call pre-flight) during settle.
	// Defaults to true when nil, matching TS settleUptoPermit2
	// (`config?.simulateInSettle ?? true`). Set to a pointer to false to skip
	// the pre-flight (e.g. when verify already simulated).
	SimulateInSettle *bool
}

// UptoPermit2SettleArgs holds the parsed and typed arguments for
// x402UptoPermit2Proxy.settle(...) / settleWithPermit(...).
//
// Field order matches the on-chain ABI: (permit, amount, owner, witness, signature).
type UptoPermit2SettleArgs struct {
	Permit struct {
		Permitted struct {
			Token  common.Address
			Amount *big.Int
		}
		Nonce    *big.Int
		Deadline *big.Int
	}
	// SettlementAmount is the actual amount to settle; may be strictly less
	// than Permit.Permitted.Amount (the signed cap). The on-chain proxy enforces
	// SettlementAmount ≤ Permit.Permitted.Amount; we re-check that invariant
	// before broadcasting.
	SettlementAmount *big.Int
	Owner            common.Address
	Witness          struct {
		To          common.Address
		Facilitator common.Address
		ValidAfter  *big.Int
	}
	Signature []byte
}

// permitStruct returns the ABI-compatible tuple for the permit parameter.
func (a *UptoPermit2SettleArgs) permitStruct() interface{} {
	return struct {
		Permitted struct {
			Token  common.Address
			Amount *big.Int
		}
		Nonce    *big.Int
		Deadline *big.Int
	}{
		Permitted: struct {
			Token  common.Address
			Amount *big.Int
		}{
			Token:  a.Permit.Permitted.Token,
			Amount: a.Permit.Permitted.Amount,
		},
		Nonce:    a.Permit.Nonce,
		Deadline: a.Permit.Deadline,
	}
}

// witnessStruct returns the ABI-compatible tuple for the upto witness parameter.
// Includes the `facilitator` field that distinguishes upto from exact.
func (a *UptoPermit2SettleArgs) witnessStruct() interface{} {
	return struct {
		To          common.Address
		Facilitator common.Address
		ValidAfter  *big.Int
	}{
		To:          a.Witness.To,
		Facilitator: a.Witness.Facilitator,
		ValidAfter:  a.Witness.ValidAfter,
	}
}

// BuildUptoPermit2SettleArgs converts a raw UptoPermit2Payload + settlement
// amount + facilitator address into typed contract-call arguments for
// x402UptoPermit2Proxy.settle / settleWithPermit.
//
// The supplied settlementAmount is used as-is — callers are responsible for
// ensuring it is ≤ payload.permitted.amount before broadcast. The facilitator
// address is normalised via common.HexToAddress and replaces the
// witness.facilitator field in the on-chain args (callers may override the
// signed value here).
func BuildUptoPermit2SettleArgs(
	permit2Payload *evm.UptoPermit2Payload,
	settlementAmount *big.Int,
	facilitatorAddr string,
) (*UptoPermit2SettleArgs, error) {
	if permit2Payload == nil {
		return nil, errors.New("nil upto permit2 payload")
	}
	if settlementAmount == nil {
		return nil, errors.New("nil settlement amount")
	}

	auth := permit2Payload.Permit2Authorization

	amount, ok := new(big.Int).SetString(auth.Permitted.Amount, 10)
	if !ok {
		return nil, fmt.Errorf("invalid permitted amount: %s", auth.Permitted.Amount)
	}
	nonce, ok := new(big.Int).SetString(auth.Nonce, 10)
	if !ok {
		return nil, fmt.Errorf("invalid nonce: %s", auth.Nonce)
	}
	deadline, ok := new(big.Int).SetString(auth.Deadline, 10)
	if !ok {
		return nil, fmt.Errorf("invalid deadline: %s", auth.Deadline)
	}
	validAfter, ok := new(big.Int).SetString(auth.Witness.ValidAfter, 10)
	if !ok {
		return nil, fmt.Errorf("invalid validAfter: %s", auth.Witness.ValidAfter)
	}
	signatureBytes, err := evm.HexToBytes(permit2Payload.Signature)
	if err != nil {
		return nil, err
	}

	// Pick the facilitator address: explicit override (e.g. server-supplied)
	// trumps the payload's witness.facilitator. If the override is empty we
	// fall back to the witness field — both should always agree at this point.
	witnessFacilitator := auth.Witness.Facilitator
	if facilitatorAddr != "" {
		witnessFacilitator = facilitatorAddr
	}

	args := &UptoPermit2SettleArgs{}
	args.Permit.Permitted.Token = common.HexToAddress(auth.Permitted.Token)
	args.Permit.Permitted.Amount = amount
	args.Permit.Nonce = nonce
	args.Permit.Deadline = deadline
	args.SettlementAmount = new(big.Int).Set(settlementAmount)
	args.Owner = common.HexToAddress(auth.From)
	args.Witness.To = common.HexToAddress(auth.Witness.To)
	args.Witness.Facilitator = common.HexToAddress(witnessFacilitator)
	args.Witness.ValidAfter = validAfter
	args.Signature = signatureBytes
	return args, nil
}

// SimulateUptoPermit2Settle runs settle() via eth_call (ReadContract). Returns
// true if the simulation succeeded. Mirrors exact-scheme SimulatePermit2Settle.
func SimulateUptoPermit2Settle(
	ctx context.Context,
	signer evm.FacilitatorEvmSigner,
	args *UptoPermit2SettleArgs,
) (bool, error) {
	if args == nil {
		return false, errors.New("nil settle args")
	}
	_, err := signer.ReadContract(
		ctx,
		evm.X402UptoPermit2ProxyAddress,
		evm.X402UptoPermit2ProxySettleABI,
		evm.FunctionSettle,
		args.permitStruct(),
		args.SettlementAmount,
		args.Owner,
		args.witnessStruct(),
		args.Signature,
	)
	if err != nil {
		return false, err
	}
	return true, nil
}

// SimulateUptoPermit2SettleWithPermit runs settleWithPermit() via eth_call.
// Atomic permit + permit2 transfer simulation.
func SimulateUptoPermit2SettleWithPermit(
	ctx context.Context,
	signer evm.FacilitatorEvmSigner,
	args *UptoPermit2SettleArgs,
	eip2612Signature, eip2612Amount, eip2612DeadlineStr string,
) (bool, error) {
	if args == nil {
		return false, errors.New("nil settle args")
	}
	v, r, s, splitErr := splitEip2612Signature(eip2612Signature)
	if splitErr != nil {
		return false, splitErr
	}

	eip2612Value, ok := new(big.Int).SetString(eip2612Amount, 10)
	if !ok {
		return false, fmt.Errorf("invalid eip2612 amount: %s", eip2612Amount)
	}
	eip2612Deadline, ok := new(big.Int).SetString(eip2612DeadlineStr, 10)
	if !ok {
		return false, fmt.Errorf("invalid eip2612 deadline: %s", eip2612DeadlineStr)
	}

	permit2612Struct := struct {
		Value    *big.Int
		Deadline *big.Int
		R        [32]byte
		S        [32]byte
		V        uint8
	}{
		Value:    eip2612Value,
		Deadline: eip2612Deadline,
		R:        r,
		S:        s,
		V:        v,
	}

	_, err := signer.ReadContract(
		ctx,
		evm.X402UptoPermit2ProxyAddress,
		evm.X402UptoPermit2ProxySettleWithPermitABI,
		evm.FunctionSettleWithPermit,
		permit2612Struct,
		args.permitStruct(),
		args.SettlementAmount,
		args.Owner,
		args.witnessStruct(),
		args.Signature,
	)
	if err != nil {
		return false, err
	}
	return true, nil
}

// DiagnoseUptoPermit2SimulationFailure runs a multicall diagnostic to return
// the most specific error reason after a simulation failure. Mirrors the exact
// DiagnosePermit2SimulationFailure but targets X402UptoPermit2ProxyAddress.
func DiagnoseUptoPermit2SimulationFailure(
	ctx context.Context,
	signer evm.FacilitatorEvmSigner,
	tokenAddress string,
	permit2Payload *evm.UptoPermit2Payload,
	amountRequired string,
) *x402.VerifyResponse {
	payer := permit2Payload.Permit2Authorization.From

	results, err := evm.Multicall(ctx, signer, []evm.MulticallCall{
		{
			Address:      evm.X402UptoPermit2ProxyAddress,
			ABI:          evm.X402UptoPermit2ProxyPermit2ABI,
			FunctionName: "PERMIT2",
		},
		{
			Address:      tokenAddress,
			ABI:          evm.ERC20BalanceOfABI,
			FunctionName: "balanceOf",
			Args:         []interface{}{common.HexToAddress(payer)},
		},
		{
			Address:      tokenAddress,
			ABI:          evm.ERC20AllowanceABI,
			FunctionName: "allowance",
			Args:         []interface{}{common.HexToAddress(payer), common.HexToAddress(evm.PERMIT2Address)},
		},
	})
	if err != nil || len(results) < 3 {
		return &x402.VerifyResponse{IsValid: false, InvalidReason: ErrPermit2SimulationFailed, Payer: payer}
	}

	if !results[0].Success() {
		return &x402.VerifyResponse{IsValid: false, InvalidReason: ErrPermit2ProxyNotDeployed, Payer: payer}
	}

	reqAmount, ok := new(big.Int).SetString(amountRequired, 10)
	if !ok {
		return &x402.VerifyResponse{IsValid: false, InvalidReason: ErrPermit2SimulationFailed, Payer: payer}
	}

	if results[1].Success() {
		if balance := asBigInt(results[1].Result); balance != nil && balance.Cmp(reqAmount) < 0 {
			return &x402.VerifyResponse{IsValid: false, InvalidReason: ErrPermit2InsufficientBalance, Payer: payer}
		}
	}

	if results[2].Success() {
		if allowance := asBigInt(results[2].Result); allowance != nil && allowance.Cmp(reqAmount) < 0 {
			return &x402.VerifyResponse{IsValid: false, InvalidReason: ErrPermit2AllowanceRequired, Payer: payer}
		}
	}

	return &x402.VerifyResponse{IsValid: false, InvalidReason: ErrPermit2SimulationFailed, Payer: payer}
}

// VerifyUptoPermit2 verifies an upto Permit2 payment payload.
//
// Order of checks:
//  1. Off-chain structural / cross-field gate via
//     uptoserver.ValidateUptoPayload — covers scheme, network, spender,
//     witness.to, witness.facilitator (vs requirements.Extra), token, amount
//     cap, deadline / validAfter, EOA signature.
//  2. ERC-1271 fallback for smart-account signatures: if the EOA verify
//     failed inside ValidateUptoPayload, check whether the payer has bytecode
//     (deployed smart wallet) and accept the signature as a contract
//     signature, deferring the actual validity to on-chain simulation.
//  3. On-chain simulation via SimulateUptoPermit2Settle (when enabled).
//     Failure routes to DiagnoseUptoPermit2SimulationFailure for a specific
//     invalid reason (insufficient balance / insufficient allowance / proxy
//     not deployed).
//  4. Witness.facilitator must match one of the facilitator signer's
//     addresses — enforced after the structural gate to surface a precise
//     ErrUptoFacilitatorMismatch.
//
// `opts.Simulate = &false` skips step 3 entirely (used by settle when
// SimulateInSettle is false).
func VerifyUptoPermit2(
	ctx context.Context,
	signer evm.FacilitatorEvmSigner,
	payload types.PaymentPayload,
	requirements types.PaymentRequirements,
	permit2Payload *evm.UptoPermit2Payload,
	_ *x402.FacilitatorContext,
	opts *VerifyUptoPermit2Options,
) (*x402.VerifyResponse, error) {
	payer := permit2Payload.Permit2Authorization.From

	// Re-publish facilitator address in requirements.Extra so the off-chain
	// validator (uptoserver.ValidateUptoPayload) has something to compare to.
	// We use the witness facilitator: the facilitator-signer check below
	// validates that it is one of OUR addresses.
	enforcedReq := requirements
	if enforcedReq.Extra == nil {
		enforcedReq.Extra = make(map[string]interface{})
	} else {
		// Shallow-copy so we don't mutate the caller's map.
		extraCopy := make(map[string]interface{}, len(enforcedReq.Extra)+1)
		for k, v := range enforcedReq.Extra {
			extraCopy[k] = v
		}
		enforcedReq.Extra = extraCopy
	}
	if _, ok := enforcedReq.Extra[uptoserver.ExtraFacilitatorAddressKey].(string); !ok {
		enforcedReq.Extra[uptoserver.ExtraFacilitatorAddressKey] = permit2Payload.Permit2Authorization.Witness.Facilitator
	}

	// Step 1: structural / cross-field validation. This is the off-chain gate
	// that short-circuits on scheme, network, spender, recipient, token,
	// amount cap, deadline, validAfter, and EOA sig.
	if err := uptoserver.ValidateUptoPayload(payload, enforcedReq); err != nil {
		// Translate validation error reason into a VerifyError.
		// uptoserver's sentinels include the upto reasons AND the shared
		// permit2 reasons we redeclared here. Either way the error string
		// carries the canonical reason string; extract it.
		reason := extractReason(err.Error())
		// If the failure is an EOA signature mismatch, check whether the payer
		// is a deployed smart contract and, if so, fall through to simulation
		// (ERC-1271 / ERC-6492 verification at the contract level).
		if reason == ErrPermit2InvalidSignature {
			code, codeErr := signer.GetCode(ctx, payer)
			if codeErr != nil || len(code) == 0 {
				return nil, x402.NewVerifyError(ErrPermit2InvalidSignature, payer, err.Error())
			}
			// Smart contract — fall through to facilitator-match + simulation.
		} else {
			return nil, x402.NewVerifyError(reason, payer, err.Error())
		}
	}

	// Step 4 (early): witness.facilitator must match one of OUR signer addresses.
	witnessFacilitator := permit2Payload.Permit2Authorization.Witness.Facilitator
	facilitatorAddrs := signer.GetAddresses()
	matched := false
	for _, addr := range facilitatorAddrs {
		if strings.EqualFold(addr, witnessFacilitator) {
			matched = true
			break
		}
	}
	if !matched {
		return nil, x402.NewVerifyError(ErrUptoFacilitatorMismatch, payer,
			fmt.Sprintf("witness.facilitator %q not in facilitator addresses", witnessFacilitator))
	}

	// Early return when simulation is disabled.
	if !opts.shouldSimulate() {
		return &x402.VerifyResponse{IsValid: true, Payer: payer}, nil
	}

	// Step 3: on-chain simulation. Per spec §Phase 3 Step 7, simulation uses
	// requirements.amount (= worst-case at verify, = actual at settle).
	settlementAmount, ok := new(big.Int).SetString(requirements.Amount, 10)
	if !ok {
		return nil, x402.NewVerifyError(ErrInvalidRequiredAmount, payer,
			fmt.Sprintf("requirements.amount %q is not a decimal integer", requirements.Amount))
	}

	tokenAddress := evm.NormalizeAddress(requirements.Asset)
	uptoSettleArgs, buildErr := BuildUptoPermit2SettleArgs(permit2Payload, settlementAmount, witnessFacilitator)
	if buildErr != nil {
		return nil, x402.NewVerifyError(ErrInvalidPayload, payer, buildErr.Error())
	}

	simOk, simErr := SimulateUptoPermit2Settle(ctx, signer, uptoSettleArgs)
	if simErr != nil || !simOk {
		resp := DiagnoseUptoPermit2SimulationFailure(ctx, signer, tokenAddress, permit2Payload, requirements.Amount)
		return nil, x402.NewVerifyError(resp.InvalidReason, payer, "simulation failed")
	}

	return &x402.VerifyResponse{IsValid: true, Payer: payer}, nil
}

// SettleUptoPermit2 settles an upto Permit2 payment by calling
// x402UptoPermit2Proxy.settle(...).
//
// Settlement semantics (per design §7.4):
//   - Re-verify the payload with `requirements.Amount` replaced by
//     `permitted.amount` (so the verify-time strict equality check passes
//     even when the actual settlement amount is lower than the signed cap).
//   - Reject when settlementAmount > permitted.amount.
//   - settlementAmount == 0 returns success with transaction="" (no tx).
//
// `requirements.Amount` is treated as the settlement amount. The resource
// server's SettlementOverrides.Amount lands here via x402ResourceServer.SettlePayment
// which overrides requirements.Amount before forwarding to the facilitator.
func SettleUptoPermit2(
	ctx context.Context,
	signer evm.FacilitatorEvmSigner,
	payload types.PaymentPayload,
	requirements types.PaymentRequirements,
	permit2Payload *evm.UptoPermit2Payload,
	facilCtx *x402.FacilitatorContext,
	config *UptoPermit2FacilitatorConfig,
) (*x402.SettleResponse, error) {
	network := x402.Network(payload.Accepted.Network)
	payer := permit2Payload.Permit2Authorization.From

	settlementAmount, ok := new(big.Int).SetString(requirements.Amount, 10)
	if !ok {
		return nil, x402.NewSettleError(ErrInvalidRequiredAmount, payer, network, "",
			fmt.Sprintf("requirements.amount %q is not a decimal integer", requirements.Amount))
	}
	if settlementAmount.Sign() < 0 {
		return nil, x402.NewSettleError(ErrInvalidRequiredAmount, payer, network, "",
			"requirements.amount must be non-negative")
	}

	permittedAmount, ok := new(big.Int).SetString(permit2Payload.Permit2Authorization.Permitted.Amount, 10)
	if !ok {
		return nil, x402.NewSettleError(ErrInvalidPayload, payer, network, "",
			fmt.Sprintf("permitted.amount %q is not a decimal integer",
				permit2Payload.Permit2Authorization.Permitted.Amount))
	}

	// Re-verify with permitted.amount so the verify-time strict equality holds.
	// The settlementAmount ≤ permitted.amount guard runs after verify.
	verifyRequirements := requirements
	verifyRequirements.Amount = permit2Payload.Permit2Authorization.Permitted.Amount

	simulate := true
	if config != nil && config.SimulateInSettle != nil {
		simulate = *config.SimulateInSettle
	}

	verifyResp, err := VerifyUptoPermit2(ctx, signer, payload, verifyRequirements, permit2Payload, facilCtx,
		&VerifyUptoPermit2Options{Simulate: &simulate})
	if err != nil {
		ve := &x402.VerifyError{}
		if errors.As(err, &ve) {
			return nil, x402.NewSettleError(ve.InvalidReason, ve.Payer, network, "", ve.InvalidMessage)
		}
		return nil, x402.NewSettleError(ErrVerificationFailed, payer, network, "", err.Error())
	}

	// Zero-settlement short circuit: no on-chain tx.
	if settlementAmount.Sign() == 0 {
		return &x402.SettleResponse{
			Success:     true,
			Transaction: "",
			Network:     network,
			Payer:       verifyResp.Payer,
			Amount:      "0",
		}, nil
	}

	if settlementAmount.Cmp(permittedAmount) > 0 {
		return nil, x402.NewSettleError(ErrUptoSettlementExceedsAmount, payer, network, "",
			fmt.Sprintf("settlement amount %s > permitted %s", settlementAmount, permittedAmount))
	}

	witnessFacilitator := permit2Payload.Permit2Authorization.Witness.Facilitator
	args, buildErr := BuildUptoPermit2SettleArgs(permit2Payload, settlementAmount, witnessFacilitator)
	if buildErr != nil {
		return nil, x402.NewSettleError(ErrInvalidPayload, payer, network, "", buildErr.Error())
	}

	permitStruct := args.permitStruct()
	witnessStruct := args.witnessStruct()

	// Direct settle path. EIP-2612 / ERC-20-approval gas-sponsoring branches
	// are NOT implemented in this stage — they require the upto extensions
	// surface to ship. Mirrors stage 6 task scope.
	txHash, err := signer.WriteContract(
		ctx,
		evm.X402UptoPermit2ProxyAddress,
		evm.X402UptoPermit2ProxySettleABI,
		evm.FunctionSettle,
		permitStruct,
		args.SettlementAmount,
		args.Owner,
		witnessStruct,
		args.Signature,
	)
	if err != nil {
		errorReason := parseUptoPermit2Error(err)
		return nil, x402.NewSettleError(errorReason, payer, network, "", err.Error())
	}

	receipt, err := signer.WaitForTransactionReceipt(ctx, txHash)
	if err != nil {
		return nil, x402.NewSettleError(ErrFailedToGetReceipt, payer, network, txHash, err.Error())
	}

	if receipt.Status != evm.TxStatusSuccess {
		return nil, x402.NewSettleError(ErrTransactionFailed, payer, network, txHash, "")
	}

	return &x402.SettleResponse{
		Success:     true,
		Transaction: txHash,
		Network:     network,
		Payer:       verifyResp.Payer,
		Amount:      settlementAmount.String(),
	}, nil
}

// parseUptoPermit2Error extracts a canonical reason string from a contract
// revert error. Adds upto-specific revert names (AmountExceedsPermitted,
// UnauthorizedFacilitator) to the exact-scheme switch.
func parseUptoPermit2Error(err error) string {
	if err == nil {
		return ErrFailedToExecuteTransfer
	}
	msg := err.Error()
	switch {
	case strings.Contains(msg, "AmountExceedsPermitted"):
		return ErrUptoAmountExceedsPermitted
	case strings.Contains(msg, "UnauthorizedFacilitator"):
		return ErrUptoUnauthorizedFacilitator
	case strings.Contains(msg, "Permit2612AmountMismatch"):
		return ErrPermit2612AmountMismatch
	case strings.Contains(msg, "InvalidAmount"):
		return ErrPermit2InvalidAmount
	case strings.Contains(msg, "InvalidDestination"):
		return ErrPermit2InvalidDestination
	case strings.Contains(msg, "InvalidOwner"):
		return ErrPermit2InvalidOwner
	case strings.Contains(msg, "PaymentTooEarly"):
		return ErrPermit2PaymentTooEarly
	case strings.Contains(msg, "InvalidSignature"), strings.Contains(msg, "SignatureExpired"):
		return ErrPermit2InvalidSignature
	case strings.Contains(msg, "InvalidNonce"):
		return ErrPermit2InvalidNonce
	case strings.Contains(msg, "TRANSFER_FROM_FAILED"), strings.Contains(msg, "transfer amount exceeds balance"):
		return ErrPermit2InsufficientBalance
	case strings.Contains(msg, "deadline"):
		return ErrPermit2DeadlineExpired
	default:
		return ErrFailedToExecuteTransfer
	}
}

// extractReason scans an error message for any of the canonical sentinel
// strings declared in errors.go and returns the matching canonical reason.
//
// The upto-server's `validate.go` formats errors using a Go-only prefixed
// alias for some Permit2 reasons (e.g. `invalid_exact_evm_payload_permit2_invalid_spender`).
// This helper recognises both forms and emits the canonical form
// (`invalid_permit2_spender`) so the wire contract stays portable.
//
// Returns ErrInvalidPayload if no sentinel is found.
func extractReason(msg string) string {
	for _, entry := range reasonTable {
		for _, alias := range entry.aliases {
			if strings.Contains(msg, alias) {
				return entry.canonical
			}
		}
	}
	return ErrInvalidPayload
}

// reasonAlias maps a canonical reason to its known surface forms. Server
// and facilitator now share the same canonical wire strings, so each entry's
// alias list is normally just the canonical itself.
type reasonAlias struct {
	canonical string
	aliases   []string
}

// reasonTable is ordered so more-specific aliases match first. No canonical
// reason string is a substring of another, so Contains-based extraction is
// unambiguous regardless of ordering, but keep specific entries early as a
// guard against future additions.
var reasonTable = []reasonAlias{
	{ErrUptoSettlementExceedsAmount, []string{ErrUptoSettlementExceedsAmount}},
	{ErrFailedToGetNetworkConfig, []string{ErrFailedToGetNetworkConfig}},
	{ErrUptoNetworkMismatch, []string{ErrUptoNetworkMismatch}},
	{ErrUptoInvalidPayloadFormat, []string{ErrUptoInvalidPayloadFormat}},
	{ErrUptoFacilitatorMismatch, []string{ErrUptoFacilitatorMismatch}},
	{ErrUptoAmountExceedsPermitted, []string{ErrUptoAmountExceedsPermitted}},
	{ErrUptoUnauthorizedFacilitator, []string{ErrUptoUnauthorizedFacilitator}},
	{ErrInvalidSignatureFormat, []string{ErrInvalidSignatureFormat}},
	{ErrInvalidRequiredAmount, []string{ErrInvalidRequiredAmount}},
	{ErrPermit2612AmountMismatch, []string{ErrPermit2612AmountMismatch}},
	// Permit2 reasons. The upto server now emits the canonical short reason
	// strings directly (no Go-only prefix), so each entry maps the canonical
	// form to itself. Kept so extractReason strips the trailing context off a
	// validate.go error and returns the bare reason.
	{ErrPermit2RecipientMismatch, []string{ErrPermit2RecipientMismatch}},
	{ErrPermit2InvalidDestination, []string{ErrPermit2InvalidDestination}},
	{ErrPermit2InvalidSignature, []string{ErrPermit2InvalidSignature}},
	{ErrPermit2DeadlineExpired, []string{ErrPermit2DeadlineExpired}},
	{ErrPermit2InsufficientBalance, []string{ErrPermit2InsufficientBalance}},
	{ErrPermit2ProxyNotDeployed, []string{ErrPermit2ProxyNotDeployed}},
	{ErrFailedToExecuteTransfer, []string{ErrFailedToExecuteTransfer}},
	{ErrPermit2NotYetValid, []string{ErrPermit2NotYetValid}},
	{ErrPermit2AmountMismatch, []string{ErrPermit2AmountMismatch}},
	{ErrPermit2InvalidSpender, []string{ErrPermit2InvalidSpender}},
	{ErrPermit2InvalidAmount, []string{ErrPermit2InvalidAmount}},
	{ErrPermit2InvalidOwner, []string{ErrPermit2InvalidOwner}},
	{ErrPermit2TokenMismatch, []string{ErrPermit2TokenMismatch}},
	{ErrPermit2AllowanceRequired, []string{ErrPermit2AllowanceRequired}},
	{ErrPermit2SimulationFailed, []string{ErrPermit2SimulationFailed}},
	{ErrPermit2PaymentTooEarly, []string{ErrPermit2PaymentTooEarly}},
	{ErrUptoInvalidScheme, []string{ErrUptoInvalidScheme}},
	{ErrUnsupportedPayloadType, []string{ErrUnsupportedPayloadType}},
	{ErrVerificationFailed, []string{ErrVerificationFailed}},
	{ErrTransactionFailed, []string{ErrTransactionFailed}},
	{ErrFailedToGetReceipt, []string{ErrFailedToGetReceipt}},
	{ErrInvalidPayload, []string{ErrInvalidPayload}},
}

// splitEip2612Signature splits a 65-byte hex signature into v, r, s.
// Duplicates exact/facilitator helper to keep this package free of upward
// dependency on the exact facilitator.
func splitEip2612Signature(signature string) (uint8, [32]byte, [32]byte, error) {
	sigBytes, err := evm.HexToBytes(signature)
	if err != nil {
		return 0, [32]byte{}, [32]byte{}, err
	}
	if len(sigBytes) != 65 {
		return 0, [32]byte{}, [32]byte{}, fmt.Errorf("signature must be 65 bytes, got %d", len(sigBytes))
	}
	var r, s [32]byte
	copy(r[:], sigBytes[0:32])
	copy(s[:], sigBytes[32:64])
	v := sigBytes[64]
	return v, r, s, nil
}

// asBigInt extracts a *big.Int from a heterogeneous multicall return value.
// Mirrors the exact-scheme helper. Supports both *big.Int and big.Int.
func asBigInt(v interface{}) *big.Int {
	if v == nil {
		return nil
	}
	switch x := v.(type) {
	case *big.Int:
		return x
	case big.Int:
		return new(big.Int).Set(&x)
	default:
		return nil
	}
}
