package evm

import (
	"bytes"
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"
)

func TestSignAuthorization_RoundTrip(t *testing.T) {
	key, err := crypto.GenerateKey()
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	signer := NewPrivateKeySigner(key)

	channelID := [32]byte{0x01, 0x02, 0x03}
	cumulativeAmount := big.NewInt(500_000)
	nonce := big.NewInt(1)
	deadline := big.NewInt(9999999999)
	escrow := common.HexToAddress("0x1234567890abcdef1234567890abcdef12345678")
	chainID := uint64(196)

	auth, err := SignAuthorization(signer, "SettleAuthorization", channelID, cumulativeAmount, nonce, deadline, escrow, chainID, "", "")
	if err != nil {
		t.Fatalf("SignAuthorization: %v", err)
	}

	if auth.ChannelID != channelID {
		t.Errorf("ChannelID mismatch: got %x, want %x", auth.ChannelID, channelID)
	}
	if auth.CumulativeAmount.Cmp(cumulativeAmount) != 0 {
		t.Errorf("CumulativeAmount = %s, want %s", auth.CumulativeAmount, cumulativeAmount)
	}
	if auth.Nonce.Cmp(nonce) != 0 {
		t.Errorf("Nonce = %s, want %s", auth.Nonce, nonce)
	}
	if auth.Deadline.Cmp(deadline) != 0 {
		t.Errorf("Deadline = %s, want %s", auth.Deadline, deadline)
	}
	if len(auth.Signature) != 65 {
		t.Fatalf("Signature length = %d, want 65", len(auth.Signature))
	}

	v := auth.Signature[64]
	if v != 27 && v != 28 {
		t.Errorf("v = %d, want 27 or 28", v)
	}
}

func TestSignAuthorization_SettleAndCloseUseSameStruct(t *testing.T) {
	key, err := crypto.GenerateKey()
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	signer := NewPrivateKeySigner(key)

	channelID := [32]byte{0xaa, 0xbb}
	amount := big.NewInt(1_000_000)
	nonce := big.NewInt(42)
	deadline := big.NewInt(9999999999)
	escrow := common.HexToAddress("0xabcdefabcdefabcdefabcdefabcdefabcdefabcd")
	chainID := uint64(1)

	settle, err := SignAuthorization(signer, "SettleAuthorization", channelID, amount, nonce, deadline, escrow, chainID, "", "")
	if err != nil {
		t.Fatalf("SignAuthorization(Settle): %v", err)
	}

	close_, err := SignAuthorization(signer, "CloseAuthorization", channelID, amount, nonce, deadline, escrow, chainID, "", "")
	if err != nil {
		t.Fatalf("SignAuthorization(Close): %v", err)
	}

	if bytes.Equal(settle.Signature, close_.Signature) {
		t.Error("expected different signatures for SettleAuthorization and CloseAuthorization (different type hashes)")
	}

	if len(settle.Signature) != 65 || len(close_.Signature) != 65 {
		t.Errorf("signature lengths: settle=%d close=%d, both want 65", len(settle.Signature), len(close_.Signature))
	}
}

func TestAuthorizationTypedData_InvalidTypeName(t *testing.T) {
	channelID := [32]byte{}
	amount := big.NewInt(0)
	nonce := big.NewInt(0)
	deadline := big.NewInt(0)
	escrow := common.Address{}
	chainID := uint64(1)

	_, err := authorizationTypedData("InvalidType", channelID, amount, nonce, deadline, escrow, chainID, "", "")
	if err == nil {
		t.Fatal("expected error for invalid type name, got nil")
	}
}
