package subscription

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestSubscriptionTermsGoldenRoundTrip(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("testdata", "terms.json"))
	require.NoError(t, err)

	var terms SubscriptionTerms
	require.NoError(t, json.Unmarshal(raw, &terms))

	assert.Equal(t, "pro_monthly", terms.PlanID)
	assert.Equal(t, uint8(2), terms.PlanTier)
	assert.Equal(t, "5000", terms.AmountPerPeriod)
	assert.Equal(t, uint64(2592000), terms.PeriodSec)

	// Re-marshal and confirm the object carries exactly the 18 wire keys.
	out, err := json.Marshal(terms)
	require.NoError(t, err)
	var obj map[string]json.RawMessage
	require.NoError(t, json.Unmarshal(out, &obj))
	assert.Len(t, obj, 18)

	for _, key := range []string{
		"payer", "merchant", "facilitator", "token", "amountPerPeriod",
		"periodSec", "maxPeriods", "startAt", "initialChargePeriods",
		"initialChargeAmount", "termsDeadline", "permitHash", "salt",
		"planId", "planTier", "changeFromSubId", "changeEffectiveAt", "periodMode",
	} {
		_, ok := obj[key]
		assert.Truef(t, ok, "expected key %q in marshaled terms", key)
	}
}

func TestSubscriptionTermsDefaultsForOlderPayloads(t *testing.T) {
	// A payload missing planId/periodMode decodes to their zero values.
	older := `{
		"payer":"0x1","merchant":"0x2","facilitator":"0x3","token":"0x4",
		"amountPerPeriod":"1","periodSec":100,"maxPeriods":1,"startAt":0,
		"initialChargePeriods":0,"initialChargeAmount":"0","termsDeadline":0,
		"permitHash":"0x5","salt":"0x6","planTier":0,
		"changeFromSubId":"0x0","changeEffectiveAt":0
	}`
	var terms SubscriptionTerms
	require.NoError(t, json.Unmarshal([]byte(older), &terms))
	assert.Equal(t, "", terms.PlanID)
	assert.Equal(t, uint8(0), terms.PeriodMode)
}

func TestTxResultResponseNullState(t *testing.T) {
	// A nil state serializes as JSON null (matching the facilitator wire shape).
	resp := TxResultResponse{SubID: "0xabc"}
	out, err := json.Marshal(resp)
	require.NoError(t, err)
	assert.JSONEq(t, `{"subId":"0xabc","state":null}`, string(out))
}
