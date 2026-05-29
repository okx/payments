package evm

import (
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/math"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/signer/core/apitypes"
)

// Package-level ABI type variables shared across voucher and proof encoding.
var (
	addressType, _ = abi.NewType("address", "", nil)
	bytes32Type, _ = abi.NewType("bytes32", "", nil)
	uint256Type, _ = abi.NewType("uint256", "", nil)
)

// secp256k1HalfN is the half-order of the secp256k1 curve for low-s validation.
var secp256k1HalfN, _ = new(big.Int).SetString("7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0", 16)

// ValidateVoucherSignature checks that sig is exactly 65 bytes and has a low-s value.
func ValidateVoucherSignature(sig []byte) error {
	if len(sig) != 65 {
		return fmt.Errorf("signature must be 65 bytes, got %d", len(sig))
	}
	s := new(big.Int).SetBytes(sig[32:64])
	if s.Cmp(secp256k1HalfN) > 0 {
		return fmt.Errorf("signature has high-s value (malleable)")
	}
	return nil
}

// ComputeChannelID computes the deterministic channel identifier as the
// keccak256 hash of the ABI-encoded tuple
// (payer, payee, token, salt, authorizedSigner, escrowContract, chainID).
func ComputeChannelID(
	payer, payee, token common.Address,
	salt [32]byte,
	authorizedSigner, escrowContract common.Address,
	chainID uint64,
) [32]byte {
	args := abi.Arguments{
		{Type: addressType}, // payer
		{Type: addressType}, // payee
		{Type: addressType}, // token
		{Type: bytes32Type}, // salt
		{Type: addressType}, // authorizedSigner
		{Type: addressType}, // escrowContract
		{Type: uint256Type}, // chainID
	}
	encoded, _ := args.Pack(
		payer, payee, token,
		salt,
		authorizedSigner, escrowContract,
		new(big.Int).SetUint64(chainID),
	)
	return crypto.Keccak256Hash(encoded)
}

// voucherTypedData builds the EIP-712 TypedData for a voucher.
func voucherTypedData(
	channelID [32]byte,
	cumulativeAmount *big.Int,
	escrowContract common.Address,
	chainID uint64,
	domainName, domainVersion string,
) apitypes.TypedData {
	if domainName == "" {
		domainName = DefaultDomainName
	}
	if domainVersion == "" {
		domainVersion = DefaultDomainVersion
	}
	return apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": {
				{Name: "name", Type: "string"},
				{Name: "version", Type: "string"},
				{Name: "chainId", Type: "uint256"},
				{Name: "verifyingContract", Type: "address"},
			},
			"Voucher": {
				{Name: "channelId", Type: "bytes32"},
				{Name: "cumulativeAmount", Type: "uint128"},
			},
		},
		PrimaryType: "Voucher",
		Domain: apitypes.TypedDataDomain{
			Name:              domainName,
			Version:           domainVersion,
			ChainId:           (*math.HexOrDecimal256)(new(big.Int).SetUint64(chainID)),
			VerifyingContract: escrowContract.Hex(),
		},
		Message: apitypes.TypedDataMessage{
			"channelId":        channelID[:],
			"cumulativeAmount": cumulativeAmount,
		},
	}
}

// voucherSigningHash returns the EIP-712 signing hash for a voucher:
//
//	keccak256("\x19\x01" ++ domainSeparator ++ structHash)
func voucherSigningHash(
	channelID [32]byte,
	cumulativeAmount *big.Int,
	escrowContract common.Address,
	chainID uint64,
	domainName, domainVersion string,
) ([]byte, error) {
	td := voucherTypedData(channelID, cumulativeAmount, escrowContract, chainID, domainName, domainVersion)

	domainSep, err := td.HashStruct("EIP712Domain", td.Domain.Map())
	if err != nil {
		return nil, fmt.Errorf("domain separator: %w", err)
	}
	msgHash, err := td.HashStruct(td.PrimaryType, td.Message)
	if err != nil {
		return nil, fmt.Errorf("struct hash: %w", err)
	}

	raw := make([]byte, 0, 2+len(domainSep)+len(msgHash))
	raw = append(raw, 0x19, 0x01)
	raw = append(raw, domainSep...)
	raw = append(raw, msgHash...)
	return crypto.Keccak256(raw), nil
}

// SignVoucher signs a payment channel voucher using the provided Signer.
// Returns the 65-byte signature with v encoded as 27 or 28.
// domainName and domainVersion default to DefaultDomainName / DefaultDomainVersion when empty.
func SignVoucher(
	signer Signer,
	channelID [32]byte,
	cumulativeAmount *big.Int,
	escrowContract common.Address,
	chainID uint64,
	domainName, domainVersion string,
) ([]byte, error) {
	hash, err := voucherSigningHash(channelID, cumulativeAmount, escrowContract, chainID, domainName, domainVersion)
	if err != nil {
		return nil, err
	}
	return signer.Sign(hash)
}

// VerifyVoucher verifies a voucher signature and returns true when the recovered
// address matches expectedSigner.  The signature may use v = 27/28 or 0/1.
// domainName and domainVersion default to DefaultDomainName / DefaultDomainVersion when empty.
func VerifyVoucher(
	escrowContract common.Address,
	chainID uint64,
	channelID [32]byte,
	cumulativeAmount *big.Int,
	sig []byte,
	expectedSigner common.Address,
	domainName, domainVersion string,
) bool {
	hash, err := voucherSigningHash(channelID, cumulativeAmount, escrowContract, chainID, domainName, domainVersion)
	if err != nil {
		return false
	}

	sigCopy := make([]byte, len(sig))
	copy(sigCopy, sig)
	if sigCopy[64] >= 27 {
		sigCopy[64] -= 27
	}

	pubkey, err := crypto.Ecrecover(hash, sigCopy)
	if err != nil {
		return false
	}
	// Derive address from uncompressed public key: keccak256(pubkey[1:])[12:]
	recovered := common.BytesToAddress(crypto.Keccak256(pubkey[1:])[12:])
	return recovered == expectedSigner
}
