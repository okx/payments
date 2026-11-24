// Package evm — shared Permit2 + EIP-2612 signing/verifying helpers.
//
// This file holds the proxy-agnostic helpers used by both the exact and upto
// schemes for:
//   - building EIP-712 typed data for a Permit2 PermitWitnessTransferFrom
//     message (exact and upto witness shapes),
//   - signing that typed data via a ClientEvmSigner,
//   - verifying that signature via a FacilitatorEvmSigner (EOA + universal),
//   - building EIP-712 typed data for an EIP-2612 Permit message and signing
//     it via a ClientEvmSigner.
//
// The helpers in this file are intentionally proxy-agnostic — neither
// X402ExactPermit2ProxyAddress nor X402UptoPermit2ProxyAddress is referenced
// here. The caller supplies the spender as part of the authorization struct
// (Permit2Authorization.Spender / UptoPermit2Authorization.Spender).
//
// EIP-712 hash output is locked to known vectors by permit2_test.go.

package evm

import (
	"context"
	"errors"
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/math"
	"github.com/ethereum/go-ethereum/signer/core/apitypes"
)

// ---------------------------------------------------------------------------
// Permit2 domain + witness interface
// ---------------------------------------------------------------------------

// Permit2Domain captures the two stable values of the Permit2 EIP-712 domain:
// the contract name ("Permit2") and the verifying contract address
// (PERMIT2Address, identical on all EVM chains via CREATE2).
//
// chainId is intentionally NOT part of this struct — callers resolve it
// per-network at sign-time and pass it through the BuildPermit2TypedData /
// SignPermit2Witness / VerifyPermit2Witness arg list. The on-the-wire Go
// domain is `TypedDataDomain` and chainID is resolved separately via
// GetEvmChainId.
type Permit2Domain struct {
	// Name is the EIP-712 domain name. For Permit2 this is always "Permit2".
	Name string
	// VerifyingContract is the EIP-712 verifying contract address. For
	// Permit2 this is always PERMIT2Address. Exported so tests can override
	// it for parity checks.
	VerifyingContract common.Address
}

// DefaultPermit2Domain returns the canonical Permit2 EIP-712 domain
// { name: "Permit2", verifyingContract: PERMIT2Address }.
func DefaultPermit2Domain() Permit2Domain {
	return Permit2Domain{
		Name:              "Permit2",
		VerifyingContract: common.HexToAddress(PERMIT2Address),
	}
}

// Witness is the small interface implemented by Permit2Authorization (exact)
// and UptoPermit2Authorization (upto) so that BuildPermit2TypedData and the
// sign/verify wrappers can work over either authorization shape.
//
// The interface contract:
//   - WitnessTypeString returns the EIP-712 sub-type literal for the witness
//     ("Witness(address to,uint256 validAfter)" or
//     "Witness(address to,address facilitator,uint256 validAfter)"). This is
//     what the on-chain proxy hashes to derive its WITNESS_TYPEHASH.
//   - permit2TypedDataMessage returns the full PermitWitnessTransferFrom
//     message map (permitted/spender/nonce/deadline/witness) ready for
//     apitypes.TypedData.HashStruct.
//   - permit2EIP712Types returns the EIP-712 type map (with EIP712Domain
//     injected) that matches the witness shape (exact or upto).
//
// The lowercase helper methods keep the public surface small while still
// allowing the two concrete authorizations to share the same Build/Sign/Verify
// machinery.
type Witness interface {
	WitnessTypeString() string
	// permit2TypedDataMessage returns the full message map for the
	// PermitWitnessTransferFrom typed data. Lowercase: package-private.
	permit2TypedDataMessage() (map[string]interface{}, error)
	// permit2EIP712Types returns the EIP-712 types map (with EIP712Domain)
	// that matches this witness shape. Lowercase: package-private.
	permit2EIP712Types() map[string][]TypedDataField
}

// permit2TypedDataMessage / permit2EIP712Types — Permit2Authorization (exact)
// implementation. Defined on Permit2Authorization (not Permit2Witness) so the
// `Witness` interface receives a value that carries permitted/spender/nonce/
// deadline as well — without those the typed data cannot be built.

func (a Permit2Authorization) permit2TypedDataMessage() (map[string]interface{}, error) {
	amount, ok := new(big.Int).SetString(a.Permitted.Amount, 10)
	if !ok || amount.Sign() < 0 {
		return nil, fmt.Errorf("invalid permitted amount: %s", a.Permitted.Amount)
	}
	nonce, ok := new(big.Int).SetString(a.Nonce, 10)
	if !ok || nonce.Sign() < 0 {
		return nil, fmt.Errorf("invalid nonce: %s", a.Nonce)
	}
	deadline, ok := new(big.Int).SetString(a.Deadline, 10)
	if !ok || deadline.Sign() < 0 {
		return nil, fmt.Errorf("invalid deadline: %s", a.Deadline)
	}
	validAfter, ok := new(big.Int).SetString(a.Witness.ValidAfter, 10)
	if !ok || validAfter.Sign() < 0 {
		return nil, fmt.Errorf("invalid validAfter: %s", a.Witness.ValidAfter)
	}
	// Reject malformed addresses before common.HexToAddress silently coerces
	// (pad/truncate) them — matches TS viem getAddress / Rust Address parsing.
	if !IsValidAddress(a.Permitted.Token) || !IsValidAddress(a.Spender) || !IsValidAddress(a.Witness.To) {
		return nil, fmt.Errorf("invalid address in permit2 authorization (token=%q spender=%q to=%q)",
			a.Permitted.Token, a.Spender, a.Witness.To)
	}
	return map[string]interface{}{
		"permitted": map[string]interface{}{
			"token":  common.HexToAddress(a.Permitted.Token).Hex(),
			"amount": amount,
		},
		"spender":  common.HexToAddress(a.Spender).Hex(),
		"nonce":    nonce,
		"deadline": deadline,
		"witness":  BuildPermit2WitnessMap(common.HexToAddress(a.Witness.To).Hex(), validAfter),
	}, nil
}

func (a Permit2Authorization) permit2EIP712Types() map[string][]TypedDataField {
	return GetPermit2EIP712Types()
}

// WitnessTypeString on Permit2Authorization mirrors the witness sub-type. Lets
// the Permit2Authorization value itself satisfy the Witness interface
// (callers don't have to extract `.Witness` to pass through Build/Sign/Verify).
func (a Permit2Authorization) WitnessTypeString() string {
	return Permit2ExactWitnessTypeString
}

func (a UptoPermit2Authorization) permit2TypedDataMessage() (map[string]interface{}, error) {
	amount, ok := new(big.Int).SetString(a.Permitted.Amount, 10)
	if !ok || amount.Sign() < 0 {
		return nil, fmt.Errorf("invalid permitted amount: %s", a.Permitted.Amount)
	}
	nonce, ok := new(big.Int).SetString(a.Nonce, 10)
	if !ok || nonce.Sign() < 0 {
		return nil, fmt.Errorf("invalid nonce: %s", a.Nonce)
	}
	deadline, ok := new(big.Int).SetString(a.Deadline, 10)
	if !ok || deadline.Sign() < 0 {
		return nil, fmt.Errorf("invalid deadline: %s", a.Deadline)
	}
	validAfter, ok := new(big.Int).SetString(a.Witness.ValidAfter, 10)
	if !ok || validAfter.Sign() < 0 {
		return nil, fmt.Errorf("invalid validAfter: %s", a.Witness.ValidAfter)
	}
	// Reject malformed addresses before common.HexToAddress silently coerces
	// (pad/truncate) them — matches TS viem getAddress / Rust Address parsing.
	if !IsValidAddress(a.Permitted.Token) || !IsValidAddress(a.Spender) ||
		!IsValidAddress(a.Witness.To) || !IsValidAddress(a.Witness.Facilitator) {
		return nil, fmt.Errorf("invalid address in upto permit2 authorization (token=%q spender=%q to=%q facilitator=%q)",
			a.Permitted.Token, a.Spender, a.Witness.To, a.Witness.Facilitator)
	}
	return map[string]interface{}{
		"permitted": map[string]interface{}{
			"token":  common.HexToAddress(a.Permitted.Token).Hex(),
			"amount": amount,
		},
		"spender":  common.HexToAddress(a.Spender).Hex(),
		"nonce":    nonce,
		"deadline": deadline,
		"witness": map[string]interface{}{
			"to":          common.HexToAddress(a.Witness.To).Hex(),
			"facilitator": common.HexToAddress(a.Witness.Facilitator).Hex(),
			"validAfter":  validAfter,
		},
	}, nil
}

func (a UptoPermit2Authorization) permit2EIP712Types() map[string][]TypedDataField {
	return GetUptoPermit2EIP712Types()
}

// WitnessTypeString on UptoPermit2Authorization mirrors the upto witness
// sub-type and lets the auth value itself satisfy the Witness interface.
func (a UptoPermit2Authorization) WitnessTypeString() string {
	return Permit2UptoWitnessTypeString
}

// ---------------------------------------------------------------------------
// EIP-2612 token info
// ---------------------------------------------------------------------------

// Eip2612TokenInfo collects the ERC-20 token metadata required to construct
// the EIP-712 domain for an EIP-2612 `permit` signature.
//
// Note: the `Decimals` field is intentionally NOT included — EIP-2612 permit
// signing does not need decimals; only name/version/chainId/verifyingContract
// participate in the EIP-712 domain.
type Eip2612TokenInfo struct {
	// Name is the ERC-20 token name as exposed by the on-chain `name()` view
	// (e.g. "USD Coin", "MegaUSD"). Used as the EIP-712 domain `name`.
	Name string
	// Version is the EIP-712 contract version string (e.g. "1", "2"). Used as
	// the EIP-712 domain `version`. Required because EIP-2612 domains include
	// the version field (unlike Permit2).
	Version string
	// Address is the ERC-20 token contract address. Used as the EIP-712 domain
	// `verifyingContract`.
	Address common.Address
}

// ---------------------------------------------------------------------------
// Permit2 typed-data construction
// ---------------------------------------------------------------------------

// BuildPermit2TypedData builds the apitypes.TypedData envelope for the given
// Permit2 witness (exact or upto, depending on the concrete Witness type) under
// the supplied Permit2 domain + chain ID.
//
// The returned TypedData is ready for apitypes.HashStruct / HashTypedData via
// the existing evm.HashTypedData helper, or for direct use with go-ethereum's
// signer/core/apitypes.
//
// chainID is required and MUST match the network the signature is intended
// for. The Permit2 domain has no `version` field by design.
func BuildPermit2TypedData(witness Witness, domain Permit2Domain, chainID *big.Int) (apitypes.TypedData, error) {
	if witness == nil {
		return apitypes.TypedData{}, errors.New("BuildPermit2TypedData: witness is nil")
	}
	if chainID == nil {
		return apitypes.TypedData{}, errors.New("BuildPermit2TypedData: chainID is nil")
	}

	message, err := witness.permit2TypedDataMessage()
	if err != nil {
		return apitypes.TypedData{}, fmt.Errorf("BuildPermit2TypedData: %w", err)
	}

	types := witness.permit2EIP712Types()

	td := apitypes.TypedData{
		Types:       make(apitypes.Types),
		PrimaryType: "PermitWitnessTransferFrom",
		Domain: apitypes.TypedDataDomain{
			Name:              domain.Name,
			ChainId:           (*math.HexOrDecimal256)(chainID),
			VerifyingContract: domain.VerifyingContract.Hex(),
		},
		Message: message,
	}
	for typeName, fields := range types {
		typedFields := make([]apitypes.Type, len(fields))
		for i, field := range fields {
			typedFields[i] = apitypes.Type{Name: field.Name, Type: field.Type}
		}
		td.Types[typeName] = typedFields
	}
	// Ensure EIP712Domain is present (Permit2 omits `version`, so it MUST
	// list only name/chainId/verifyingContract — using the existing
	// EIP712DomainTypes table guarantees this).
	if _, ok := td.Types["EIP712Domain"]; !ok {
		domainFields := make([]apitypes.Type, len(EIP712DomainTypes))
		for i, f := range EIP712DomainTypes {
			domainFields[i] = apitypes.Type{Name: f.Name, Type: f.Type}
		}
		td.Types["EIP712Domain"] = domainFields
	}
	return td, nil
}

// hashPermit2TypedData computes the 32-byte EIP-712 digest for the given
// envelope (`0x1901 || domainSeparator || hashStruct(message)`).
func hashPermit2TypedData(td apitypes.TypedData) ([]byte, error) {
	// Delegate to the shared evm.HashTypedData. Convert apitypes shape back
	// to the internal TypedDataDomain/TypedDataField representation so the
	// behavior matches every other call-site in the package.
	domain := TypedDataDomain{
		Name:              td.Domain.Name,
		Version:           td.Domain.Version,
		ChainID:           (*big.Int)(td.Domain.ChainId),
		VerifyingContract: td.Domain.VerifyingContract,
	}
	types := make(map[string][]TypedDataField, len(td.Types))
	for typeName, fields := range td.Types {
		out := make([]TypedDataField, len(fields))
		for i, f := range fields {
			out[i] = TypedDataField{Name: f.Name, Type: f.Type}
		}
		types[typeName] = out
	}
	return HashTypedData(domain, types, td.PrimaryType, td.Message)
}

// ---------------------------------------------------------------------------
// Permit2 signing / verifying
// ---------------------------------------------------------------------------

// SignPermit2Witness signs the Permit2 PermitWitnessTransferFrom message for
// the given witness (exact or upto) under the supplied Permit2 domain + chain
// ID. Returns the 65-byte r,s,v signature and the 32-byte EIP-712 digest that
// was signed (the latter is useful for diagnostics and cross-language vector
// tests).
//
// Reuses the existing ClientEvmSigner interface — no new signer type is
// introduced. The signer must implement SignTypedData (every concrete signer
// in signers/evm already does).
func SignPermit2Witness(
	ctx context.Context,
	signer ClientEvmSigner,
	witness Witness,
	domain Permit2Domain,
	chainID *big.Int,
) (signature []byte, hash []byte, err error) {
	if signer == nil {
		return nil, nil, errors.New("SignPermit2Witness: signer is nil")
	}
	td, err := BuildPermit2TypedData(witness, domain, chainID)
	if err != nil {
		return nil, nil, err
	}
	// Convert types back to the internal shape for the signer call. The
	// signer interface uses TypedDataDomain/TypedDataField (not apitypes)
	// throughout the package.
	internalTypes := make(map[string][]TypedDataField, len(td.Types))
	for typeName, fields := range td.Types {
		out := make([]TypedDataField, len(fields))
		for i, f := range fields {
			out[i] = TypedDataField{Name: f.Name, Type: f.Type}
		}
		internalTypes[typeName] = out
	}
	internalDomain := TypedDataDomain{
		Name:              td.Domain.Name,
		Version:           td.Domain.Version,
		ChainID:           (*big.Int)(td.Domain.ChainId),
		VerifyingContract: td.Domain.VerifyingContract,
	}
	sig, err := signer.SignTypedData(ctx, internalDomain, internalTypes, td.PrimaryType, td.Message)
	if err != nil {
		return nil, nil, fmt.Errorf("SignPermit2Witness: signTypedData failed: %w", err)
	}
	digest, err := hashPermit2TypedData(td)
	if err != nil {
		// Signature is already computed; return it so callers that don't
		// care about the digest aren't blocked, but wrap the error.
		return sig, nil, fmt.Errorf("SignPermit2Witness: hashTypedData failed: %w", err)
	}
	return sig, digest, nil
}

// VerifyPermit2Witness performs an EOA (ECDSA-recover) verification of a
// Permit2 EIP-712 signature against the expected signer address. Returns
// (true, nil) only if the recovered address exactly matches expectedSigner.
//
// Returns (false, nil) — NOT an error — when:
//   - the signature does not match (recovered address differs),
//   - the signature length is not exactly 65 bytes,
//   - signature recovery fails (e.g. malformed v).
//
// Returns (false, err) only for hash construction errors (malformed witness
// fields). A "bad signature" is not an error — it returns a false result.
//
// For ERC-1271 / ERC-6492 verification (smart contract wallets), the
// caller should use evm.VerifyUniversalSignature against a
// FacilitatorEvmSigner — that path requires an on-chain GetCode lookup and
// is outside the scope of this proxy-agnostic helper. The exact-scheme
// facilitator continues to use VerifyUniversalSignature; see
// exact/facilitator/permit2.go:verifyPermit2Signature.
func VerifyPermit2Witness(
	expectedSigner common.Address,
	signature []byte,
	witness Witness,
	domain Permit2Domain,
	chainID *big.Int,
) (bool, error) {
	td, err := BuildPermit2TypedData(witness, domain, chainID)
	if err != nil {
		return false, err
	}
	hash, err := hashPermit2TypedData(td)
	if err != nil {
		return false, err
	}
	if len(signature) != 65 {
		// Documented behavior: malformed signature length returns (false, nil)
		// rather than an error.
		return false, nil
	}
	ok, recoverErr := VerifyEOASignature(hash, signature, expectedSigner)
	if recoverErr != nil {
		// Recovery error (e.g. invalid r/s/v) — treat as not-valid, no error.
		return false, nil
	}
	return ok, nil
}

// ---------------------------------------------------------------------------
// EIP-2612 typed-data construction + signing
// ---------------------------------------------------------------------------

// BuildEip2612PermitTypedData builds the apitypes.TypedData envelope for an
// EIP-2612 `permit(owner, spender, value, nonce, deadline)` message under the
// token's EIP-712 domain (name + version + chainId + verifyingContract).
//
// All large integers (value, nonce, deadline) are passed as *big.Int by the
// caller so the helper does not have to second-guess decimal-string parsing.
func BuildEip2612PermitTypedData(
	token Eip2612TokenInfo,
	owner, spender common.Address,
	value, nonce, deadline, chainID *big.Int,
) (apitypes.TypedData, error) {
	if value == nil || nonce == nil || deadline == nil || chainID == nil {
		return apitypes.TypedData{}, errors.New("BuildEip2612PermitTypedData: value, nonce, deadline and chainID are required")
	}
	// uint256 EIP-712 fields must be non-negative — reject negatives before encoding.
	if value.Sign() < 0 || nonce.Sign() < 0 || deadline.Sign() < 0 || chainID.Sign() < 0 {
		return apitypes.TypedData{}, errors.New("BuildEip2612PermitTypedData: value, nonce, deadline and chainID must be non-negative")
	}
	td := apitypes.TypedData{
		Types:       make(apitypes.Types),
		PrimaryType: "Permit",
		Domain: apitypes.TypedDataDomain{
			Name:              token.Name,
			Version:           token.Version,
			ChainId:           (*math.HexOrDecimal256)(chainID),
			VerifyingContract: token.Address.Hex(),
		},
		Message: map[string]interface{}{
			"owner":    owner.Hex(),
			"spender":  spender.Hex(),
			"value":    value,
			"nonce":    nonce,
			"deadline": deadline,
		},
	}
	for typeName, fields := range GetEIP2612EIP712Types() {
		typedFields := make([]apitypes.Type, len(fields))
		for i, f := range fields {
			typedFields[i] = apitypes.Type{Name: f.Name, Type: f.Type}
		}
		td.Types[typeName] = typedFields
	}
	return td, nil
}

// SignEip2612Permit signs the EIP-2612 permit typed data for the supplied
// token + (owner, spender, value, nonce, deadline) tuple under the supplied
// chain ID. Returns the 65-byte r,s,v signature.
//
// Reuses the existing ClientEvmSigner interface. The signer must implement
// SignTypedData; no new signer type is introduced.
//
// The exact-scheme helper (exact/client/eip2612.go:SignEip2612Permit)
// additionally queries `nonces()` on-chain and packages the result into an
// Eip2612GasSponsoringInfo struct — that flow is preserved for backward
// compatibility; this helper is the lower-level signing primitive that both
// exact and upto can call once the nonce is in hand.
func SignEip2612Permit(
	ctx context.Context,
	signer ClientEvmSigner,
	token Eip2612TokenInfo,
	owner, spender common.Address,
	value, nonce, deadline, chainID *big.Int,
) ([]byte, error) {
	if signer == nil {
		return nil, errors.New("SignEip2612Permit: signer is nil")
	}
	td, err := BuildEip2612PermitTypedData(token, owner, spender, value, nonce, deadline, chainID)
	if err != nil {
		return nil, err
	}
	internalTypes := make(map[string][]TypedDataField, len(td.Types))
	for typeName, fields := range td.Types {
		out := make([]TypedDataField, len(fields))
		for i, f := range fields {
			out[i] = TypedDataField{Name: f.Name, Type: f.Type}
		}
		internalTypes[typeName] = out
	}
	internalDomain := TypedDataDomain{
		Name:              td.Domain.Name,
		Version:           td.Domain.Version,
		ChainID:           (*big.Int)(td.Domain.ChainId),
		VerifyingContract: td.Domain.VerifyingContract,
	}
	sig, err := signer.SignTypedData(ctx, internalDomain, internalTypes, td.PrimaryType, td.Message)
	if err != nil {
		return nil, fmt.Errorf("SignEip2612Permit: signTypedData failed: %w", err)
	}
	return sig, nil
}
