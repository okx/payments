package subscription

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/okx/payments/go/x402/types"
)

func planAccept(id string, tier uint8, withInitialCharge bool) types.PaymentRequirements {
	plan := SubscriptionPlan{
		ID:              id,
		Tier:            tier,
		Network:         "eip155:196",
		PayTo:           "0xmerchant",
		AmountPerPeriod: "5000",
		PeriodSec:       2592000,
		MaxPeriods:      12,
	}
	if withInitialCharge {
		plan.InitialChargePeriods = 1
		plan.InitialChargeAmount = "5000"
	}
	return types.PaymentRequirements{
		Scheme:  SchemePeriod,
		Network: "eip155:196",
		PayTo:   "0xmerchant",
		Extra:   plan.BuildExtra(),
	}
}

func TestBuildChangeAcceptsDropsOwnPlanAndSetsDirection(t *testing.T) {
	accepts := []types.PaymentRequirements{
		planAccept("basic", 1, true),
		planAccept("pro", 2, true),
		planAccept("pro_yearly", 3, true),
	}

	// Subscriber is on basic (tier 1): basic is dropped; pro/pro_yearly are upgrades.
	offers := BuildChangeAccepts(accepts, "0xsub", "basic", 1)
	require.Len(t, offers, 2)
	for _, offer := range offers {
		cf, ok := offer.Extra["changeFrom"].(map[string]interface{})
		require.True(t, ok)
		assert.Equal(t, directionUpgrade, cf["direction"])
		assert.Equal(t, effectiveImmediate, cf["effectiveAt"])
		assert.Equal(t, "basic", cf["fromPlanId"])
	}
}

func TestBuildChangeAcceptsDowngradeStripsInitialCharge(t *testing.T) {
	accepts := []types.PaymentRequirements{
		planAccept("basic", 1, true),
		planAccept("pro", 2, true),
	}

	// Subscriber is on pro (tier 2): basic is a downgrade, and its offer must
	// not carry initialCharge.
	offers := BuildChangeAccepts(accepts, "0xsub", "pro", 2)
	require.Len(t, offers, 1)
	assert.Equal(t, "basic", func() string { id, _ := planFields(offers[0].Extra); return id }())

	cf := offers[0].Extra["changeFrom"].(map[string]interface{})
	assert.Equal(t, directionDowngrade, cf["direction"])
	assert.Equal(t, effectivePeriodEnd, cf["effectiveAt"])
	_, hasInitial := offers[0].Extra["initialCharge"]
	assert.False(t, hasInitial, "downgrade offer must strip initialCharge")
}

func TestBuildChangeAcceptsDropsSameTier(t *testing.T) {
	accepts := []types.PaymentRequirements{
		planAccept("basic", 1, false),
		planAccept("basic_v2", 1, false), // same tier as current
		planAccept("pro", 2, false),
	}
	offers := BuildChangeAccepts(accepts, "0xsub", "basic", 1)
	// basic (same id) and basic_v2 (same tier) both dropped; only pro remains.
	require.Len(t, offers, 1)
	id, _ := planFields(offers[0].Extra)
	assert.Equal(t, "pro", id)
}

func TestAcceptedPlansReturnsIDTierPairsAndSkipsNonPeriod(t *testing.T) {
	accepts := []types.PaymentRequirements{
		planAccept("basic", 1, false),
		planAccept("pro", 2, false),
		{Scheme: "exact", Network: "eip155:196"}, // not a period accept
	}
	plans := AcceptedPlans(accepts)
	require.Len(t, plans, 2)
	assert.Equal(t, "basic", plans[0].PlanID)
	assert.Equal(t, uint8(1), plans[0].PlanTier)
	assert.Equal(t, "pro", plans[1].PlanID)
	assert.Equal(t, uint8(2), plans[1].PlanTier)
}

func TestPlanIDGatingHelpers(t *testing.T) {
	accepts := []types.PaymentRequirements{planAccept("basic", 1, false), planAccept("pro", 2, false)}
	ids := AcceptedPlanIDs(accepts)
	assert.ElementsMatch(t, []string{"basic", "pro"}, ids)
	assert.True(t, PlanIDAccepted(ids, "pro"))
	assert.False(t, PlanIDAccepted(ids, "enterprise"))
}
