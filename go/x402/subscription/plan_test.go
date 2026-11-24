package subscription

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestBuildExtraOmitsInitialChargeWhenZero(t *testing.T) {
	plan := SubscriptionPlan{ID: "basic", Tier: 1, AmountPerPeriod: "5000", PeriodSec: 100, MaxPeriods: 12}
	extra := plan.BuildExtra()
	_, hasInitial := extra["initialCharge"]
	assert.False(t, hasInitial)

	planObj := extra["plan"].(map[string]interface{})
	assert.Equal(t, "basic", planObj["id"])
	assert.Equal(t, uint8(1), planObj["tier"])
}

func TestBuildExtraIncludesInitialCharge(t *testing.T) {
	plan := SubscriptionPlan{
		ID: "pro_yearly", Tier: 3, AmountPerPeriod: "16000", PeriodSec: 100, MaxPeriods: 12,
		InitialChargePeriods: 12, InitialChargeAmount: "192000",
		Name: "Pro Yearly", Features: []string{"api_pro"},
	}
	extra := plan.BuildExtra()
	ic, ok := extra["initialCharge"].(map[string]interface{})
	require.True(t, ok)
	assert.Equal(t, uint32(12), ic["periodCount"])
	assert.Equal(t, "192000", ic["totalAmount"])
	assert.Equal(t, true, ic["coversFirstPeriods"])

	planObj := extra["plan"].(map[string]interface{})
	assert.Equal(t, "Pro Yearly", planObj["name"])
}
