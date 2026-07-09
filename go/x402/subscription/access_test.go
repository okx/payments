package subscription

import (
	"encoding/hex"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/accounts"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// signedProof builds a valid AccessProof for a freshly generated key.
func signedProof(t *testing.T, subID string, timestamp uint64) (*AccessProof, string) {
	t.Helper()
	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	payer := crypto.PubkeyToAddress(key.PublicKey).Hex()

	inner, err := accessProofInnerHash(subID, payer, timestamp)
	require.NoError(t, err)
	msgHash := accounts.TextHash(inner)
	sig, err := crypto.Sign(msgHash, key)
	require.NoError(t, err)
	sig[64] += 27

	return &AccessProof{
		Kind:      AccessProofKind,
		SubID:     subID,
		Payer:     payer,
		Timestamp: timestamp,
		Signature: "0x" + hex.EncodeToString(sig),
	}, payer
}

func TestVerifyAccessProofRoundTrip(t *testing.T) {
	subID := "0x" + strings.Repeat("ab", 32)
	now := int64(1_700_000_000)
	proof, payer := signedProof(t, subID, uint64(now))

	verified, err := VerifyAccessProof(proof, now, AccessWindowSecs)
	require.NoError(t, err)
	assert.Equal(t, subID, verified.SubID)
	assert.Equal(t, payer, verified.Payer)
}

func TestVerifyAccessProofRejectsWrongKind(t *testing.T) {
	subID := "0x" + strings.Repeat("cd", 32)
	now := int64(1_700_000_000)
	proof, _ := signedProof(t, subID, uint64(now))
	proof.Kind = "something-else"

	_, err := VerifyAccessProof(proof, now, AccessWindowSecs)
	assert.Error(t, err)
}

func TestVerifyAccessProofRejectsReplayWindow(t *testing.T) {
	subID := "0x" + strings.Repeat("ef", 32)
	signedAt := int64(1_700_000_000)
	proof, _ := signedProof(t, subID, uint64(signedAt))

	// 301s of skew is outside the ±300s window.
	_, err := VerifyAccessProof(proof, signedAt+301, AccessWindowSecs)
	assert.Error(t, err)
}

func TestVerifyAccessProofRejectsTamperedPayer(t *testing.T) {
	subID := "0x" + strings.Repeat("12", 32)
	now := int64(1_700_000_000)
	proof, _ := signedProof(t, subID, uint64(now))
	proof.Payer = "0x0000000000000000000000000000000000000001"

	_, err := VerifyAccessProof(proof, now, AccessWindowSecs)
	assert.Error(t, err)
}

func TestCurrentPeriodCharged(t *testing.T) {
	now := int64(1_700_000_000)

	// Backend elapsed preferred: charged current period → granted.
	paid := &SubscriptionStatus{ElapsedPeriods: 3, LastChargedPeriod: 3}
	assert.True(t, CurrentPeriodCharged(paid, now))

	// Unpaid current period → denied.
	unpaid := &SubscriptionStatus{ElapsedPeriods: 3, LastChargedPeriod: 2}
	assert.False(t, CurrentPeriodCharged(unpaid, now))

	// Prepaid COMPLETED still inside window (lastCharged covers elapsed) → granted.
	prepaid := &SubscriptionStatus{State: StateCompleted, ElapsedPeriods: 5, LastChargedPeriod: 12}
	assert.True(t, CurrentPeriodCharged(prepaid, now))

	// Expired window: elapsed beyond what was ever charged → denied.
	expired := &SubscriptionStatus{ElapsedPeriods: 13, LastChargedPeriod: 12}
	assert.False(t, CurrentPeriodCharged(expired, now))

	// Local compute path (no backend elapsed): fixed mode.
	local := &SubscriptionStatus{
		PeriodMode: PeriodModeFixed, StartAt: 1000, PeriodSec: 100,
		LastChargedPeriod: 2,
	}
	// now within period 2 (1100..1199): elapsed=2, lastCharged=2 → granted.
	assert.True(t, CurrentPeriodCharged(local, 1150))
}
