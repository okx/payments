package evm

import (
	"crypto/ecdsa"
	"fmt"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/signer/core/apitypes"
)

// Signer signs arbitrary message hashes.
type Signer interface {
	Sign(hash []byte) ([]byte, error)
	SignTypedData(typedData apitypes.TypedData) ([]byte, error)
	Address() common.Address
}

// PrivateKeySigner implements Signer using an ECDSA private key.
type PrivateKeySigner struct {
	key     *ecdsa.PrivateKey
	address common.Address
}

func NewPrivateKeySigner(key *ecdsa.PrivateKey) *PrivateKeySigner {
	return &PrivateKeySigner{
		key:     key,
		address: crypto.PubkeyToAddress(key.PublicKey),
	}
}

// NewPrivateKeySignerFromHex creates a PrivateKeySigner from a hex-encoded
// private key string (with or without "0x" prefix).
func NewPrivateKeySignerFromHex(hexKey string) (*PrivateKeySigner, error) {
	if len(hexKey) > 2 && hexKey[:2] == "0x" {
		hexKey = hexKey[2:]
	}
	key, err := crypto.HexToECDSA(hexKey)
	if err != nil {
		return nil, fmt.Errorf("invalid private key hex: %w", err)
	}
	return NewPrivateKeySigner(key), nil
}

func (s *PrivateKeySigner) Sign(hash []byte) ([]byte, error) {
	sig, err := crypto.Sign(hash, s.key)
	if err != nil {
		return nil, err
	}
	// Convert v from 0/1 to 27/28 for EIP-155 compatibility
	if sig[64] < 27 {
		sig[64] += 27
	}
	return sig, nil
}

func (s *PrivateKeySigner) SignTypedData(td apitypes.TypedData) ([]byte, error) {
	domainSep, err := td.HashStruct("EIP712Domain", td.Domain.Map())
	if err != nil {
		return nil, fmt.Errorf("hash domain: %w", err)
	}
	msgHash, err := td.HashStruct(td.PrimaryType, td.Message)
	if err != nil {
		return nil, fmt.Errorf("hash message: %w", err)
	}

	// EIP-712 digest: keccak256("\x19\x01" || domainSep || msgHash)
	digest := crypto.Keccak256(
		append([]byte{0x19, 0x01}, append(domainSep, msgHash...)...),
	)

	sig, err := crypto.Sign(digest, s.key)
	if err != nil {
		return nil, fmt.Errorf("sign: %w", err)
	}
	// Convert v from 0/1 to 27/28
	if sig[64] < 27 {
		sig[64] += 27
	}
	return sig, nil
}

func (s *PrivateKeySigner) Address() common.Address {
	return s.address
}
