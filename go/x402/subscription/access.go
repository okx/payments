package subscription

import (
	"encoding/hex"
	"fmt"
	"math/big"
	"strings"

	"github.com/ethereum/go-ethereum/accounts"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"

	"github.com/okx/payments/go/x402/mechanisms/evm"
)

// VerifiedAccess is the cryptographically bound (subId, payer) pair recovered
// from a valid AccessProof. It carries no subscription state.
type VerifiedAccess struct {
	SubID string
	Payer string
}

// VerifyAccessProof checks a decoded AccessProof: the kind marker, the replay
// window, and that the EIP-191 personal_sign signature recovers to the claimed
// payer. It binds (subId, payer) only — subscription state is gated separately.
func VerifyAccessProof(proof *AccessProof, nowSec, windowSecs int64) (*VerifiedAccess, error) {
	if proof.Kind != AccessProofKind {
		return nil, fmt.Errorf("AccessProof: unexpected kind %q", proof.Kind)
	}
	if absDiff(nowSec, int64(proof.Timestamp)) > windowSecs {
		return nil, fmt.Errorf("AccessProof: timestamp outside ±%ds window (replay guard)", windowSecs)
	}

	inner, err := accessProofInnerHash(proof.SubID, proof.Payer, proof.Timestamp)
	if err != nil {
		return nil, err
	}
	msgHash := accounts.TextHash(inner)

	sig, err := decodeSignature(proof.Signature)
	if err != nil {
		return nil, err
	}

	ok, err := evm.VerifyEOASignature(msgHash, sig, common.HexToAddress(proof.Payer))
	if err != nil {
		return nil, fmt.Errorf("AccessProof: signature recovery failed: %w", err)
	}
	if !ok {
		return nil, fmt.Errorf("AccessProof: signature does not match payer")
	}

	return &VerifiedAccess{SubID: proof.SubID, Payer: proof.Payer}, nil
}

// accessProofInnerHash packs keccak256(subId[32] ‖ payer[20] ‖ timestamp[32]).
// The timestamp is a big-endian uint256; the payer is the raw 20-byte address.
func accessProofInnerHash(subID, payer string, timestamp uint64) ([]byte, error) {
	sub, err := decodeBytes32(subID)
	if err != nil {
		return nil, fmt.Errorf("invalid subId bytes32: %s", subID)
	}
	trimmedPayer := strings.TrimSpace(payer)
	if !common.IsHexAddress(trimmedPayer) {
		return nil, fmt.Errorf("invalid payer address: %s", payer)
	}
	addr := common.HexToAddress(trimmedPayer)

	ts := make([]byte, accessProofTimestampBytes)
	new(big.Int).SetUint64(timestamp).FillBytes(ts)

	buf := make([]byte, 0, len(sub)+len(addr)+len(ts))
	buf = append(buf, sub...)
	buf = append(buf, addr.Bytes()...)
	buf = append(buf, ts...)

	return crypto.Keccak256(buf), nil
}

// decodeBytes32 parses a 0x-hex string into exactly 32 bytes.
func decodeBytes32(s string) ([]byte, error) {
	raw, err := hex.DecodeString(strings.TrimPrefix(strings.TrimSpace(s), "0x"))
	if err != nil {
		return nil, err
	}
	if len(raw) != 32 {
		return nil, fmt.Errorf("expected 32 bytes, got %d", len(raw))
	}
	return raw, nil
}

// decodeSignature parses a 0x-hex 65-byte secp256k1 signature.
func decodeSignature(s string) ([]byte, error) {
	raw, err := hex.DecodeString(strings.TrimPrefix(strings.TrimSpace(s), "0x"))
	if err != nil {
		return nil, fmt.Errorf("AccessProof: invalid signature hex: %w", err)
	}
	if len(raw) != 65 {
		return nil, fmt.Errorf("AccessProof: expected 65-byte signature, got %d", len(raw))
	}
	return raw, nil
}

// CurrentPeriodCharged reports whether the current billing period has been paid.
// It prefers the backend's elapsedPeriods and otherwise computes locally. There
// is no state gate: a paid period survives cancellation, and a prepaid
// COMPLETED subscription stays accessible while still inside its window.
func CurrentPeriodCharged(status *SubscriptionStatus, nowSec int64) bool {
	elapsed := int64(status.ElapsedPeriods)
	if elapsed <= 0 {
		elapsed = ElapsedPeriods(status.PeriodMode, int64(status.StartAt),
			int64(status.BillingAnchorAt), int64(status.PeriodSec), nowSec)
	}
	return elapsed > 0 && int64(status.LastChargedPeriod) >= elapsed
}

func absDiff(a, b int64) int64 {
	if a > b {
		return a - b
	}
	return b - a
}
