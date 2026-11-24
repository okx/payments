package subscription

import (
	"encoding/json"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/okx/payments/go/x402/types"
)

func sampleInner() *SubscriptionPayloadInner {
	return &SubscriptionPayloadInner{
		PermitSingle: PermitSingle{
			Details:     PermitDetails{Token: "0xtoken", Amount: "1000", Expiration: 111, Nonce: 7},
			Spender:     "0xspender",
			SigDeadline: "222",
		},
		PermitSingleSignature: "0xpermitsig",
		Terms: SubscriptionTerms{
			Payer:           "0xpayer",
			Merchant:        "0xmerchant",
			AmountPerPeriod: "5000",
			PeriodSec:       2592000,
			MaxPeriods:      12,
			PlanID:          "pro_monthly",
			PlanTier:        2,
			ChangeFromSubID: "0x0000000000000000000000000000000000000000000000000000000000000000",
		},
		TermsSignature: "0xtermssig",
	}
}

func payloadFromInner(t *testing.T, inner *SubscriptionPayloadInner) *types.PaymentPayload {
	raw, err := json.Marshal(inner)
	require.NoError(t, err)
	var m map[string]interface{}
	require.NoError(t, json.Unmarshal(raw, &m))
	return &types.PaymentPayload{
		X402Version: 2,
		Payload:     m,
		Accepted:    types.PaymentRequirements{Scheme: SchemePeriod, Network: "eip155:196"},
	}
}

func TestUnpackSubscriptionPayload(t *testing.T) {
	inner := sampleInner()
	payload := payloadFromInner(t, inner)

	got, err := UnpackSubscriptionPayload(payload)
	require.NoError(t, err)
	assert.Equal(t, "0xtermssig", got.TermsSignature)
	assert.Equal(t, "pro_monthly", got.Terms.PlanID)
	assert.Equal(t, uint64(7), got.PermitSingle.Details.Nonce)
}

func TestToCreateRequestRenamesKeys(t *testing.T) {
	inner := sampleInner()
	req := ToCreateRequest(196, inner, true)

	raw, err := json.Marshal(req)
	require.NoError(t, err)
	var obj map[string]json.RawMessage
	require.NoError(t, json.Unmarshal(raw, &obj))

	for _, key := range []string{"chainIndex", "terms", "permit", "termsSig", "permitSig", "syncSettle"} {
		_, ok := obj[key]
		assert.Truef(t, ok, "expected %q in create request", key)
	}
	// The buyer's permitSingle / *Signature keys must not survive.
	_, hasPermitSingle := obj["permitSingle"]
	assert.False(t, hasPermitSingle)
}

func TestToChangeRequestCarriesOldSubID(t *testing.T) {
	inner := sampleInner()
	inner.Terms.ChangeFromSubID = "0xdead"
	req := ToChangeRequest(196, "0xdead", inner, true)

	raw, err := json.Marshal(req)
	require.NoError(t, err)
	var obj map[string]json.RawMessage
	require.NoError(t, json.Unmarshal(raw, &obj))

	_, hasOld := obj["oldSubId"]
	assert.True(t, hasOld)
	_, hasNewTerms := obj["newTerms"]
	assert.True(t, hasNewTerms)
	assert.Equal(t, "0xdead", req.OldSubID)
}

func TestChainIndexFromNetwork(t *testing.T) {
	idx, ok := ChainIndexFromNetwork("eip155:196")
	assert.True(t, ok)
	assert.Equal(t, uint64(196), idx)

	_, ok = ChainIndexFromNetwork("solana:mainnet")
	assert.False(t, ok)
}

func TestDecodeAccessProofRejectsBadBase64(t *testing.T) {
	_, err := DecodeAccessProof("!!!not base64!!!")
	assert.Error(t, err)
}
