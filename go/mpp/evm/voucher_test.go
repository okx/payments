package evm

import (
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"
)

func TestValidateVoucherSignature_WrongLength(t *testing.T) {
	// 64 bytes: too short
	short := make([]byte, 64)
	if err := ValidateVoucherSignature(short); err == nil {
		t.Error("expected error for 64-byte signature, got nil")
	}

	// 66 bytes: too long
	long := make([]byte, 66)
	if err := ValidateVoucherSignature(long); err == nil {
		t.Error("expected error for 66-byte signature, got nil")
	}
}

func TestValidateVoucherSignature_HighS(t *testing.T) {
	sig := make([]byte, 65)

	// Set s (bytes 32..63) to secp256k1HalfN + 1 (high-s).
	highS := new(big.Int).Add(secp256k1HalfN, big.NewInt(1))
	sBytes := highS.Bytes()
	// Right-align into sig[32:64]
	copy(sig[32+(32-len(sBytes)):64], sBytes)

	if err := ValidateVoucherSignature(sig); err == nil {
		t.Error("expected error for high-s signature, got nil")
	}
}

func TestValidateVoucherSignature_ValidSig(t *testing.T) {
	key, err := crypto.GenerateKey()
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	signer := NewPrivateKeySigner(key)

	channelID := [32]byte{1, 2, 3}
	amount := big.NewInt(1000)
	escrow := common.HexToAddress("0x1234567890abcdef1234567890abcdef12345678")
	chainID := uint64(1)

	sig, err := SignVoucher(signer, channelID, amount, escrow, chainID, "", "")
	if err != nil {
		t.Fatalf("SignVoucher: %v", err)
	}

	if err := ValidateVoucherSignature(sig); err != nil {
		t.Errorf("ValidateVoucherSignature on valid sig: %v", err)
	}
}

func TestSignAndVerifyVoucher_RoundTrip(t *testing.T) {
	key, err := crypto.GenerateKey()
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	signer := NewPrivateKeySigner(key)

	channelID := [32]byte{0xAA, 0xBB, 0xCC}
	amount := big.NewInt(5000)
	escrow := common.HexToAddress("0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
	chainID := uint64(196)

	sig, err := SignVoucher(signer, channelID, amount, escrow, chainID, "", "")
	if err != nil {
		t.Fatalf("SignVoucher: %v", err)
	}

	// Verify against the correct signer address -- should pass.
	correctAddr := crypto.PubkeyToAddress(key.PublicKey)
	if !VerifyVoucher(escrow, chainID, channelID, amount, sig, correctAddr, "", "") {
		t.Error("VerifyVoucher should succeed for correct signer")
	}

	// Verify against a different (wrong) signer address -- should fail.
	wrongKey, err := crypto.GenerateKey()
	if err != nil {
		t.Fatalf("GenerateKey (wrong): %v", err)
	}
	wrongAddr := crypto.PubkeyToAddress(wrongKey.PublicKey)
	if VerifyVoucher(escrow, chainID, channelID, amount, sig, wrongAddr, "", "") {
		t.Error("VerifyVoucher should fail for wrong signer")
	}
}
