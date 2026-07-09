package subscription

import (
	"context"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/okx/payments/go/x402/types"
)

func supportedKind(extra map[string]interface{}) types.SupportedKind {
	return types.SupportedKind{X402Version: 2, Scheme: SchemePeriod, Network: "eip155:196", Extra: extra}
}

func TestEnhancePaymentRequirementsInjectsExtra(t *testing.T) {
	scheme := NewPeriodScheme()
	req := types.PaymentRequirements{Scheme: SchemePeriod, Network: "eip155:196", PayTo: "0xmerchant"}
	kind := supportedKind(map[string]interface{}{
		"facilitatorAddress":   "0xfacil",
		"subscriptionContract": "0xsub",
		"permit2Contract":      "0xpermit2",
	})

	out, err := scheme.EnhancePaymentRequirements(context.Background(), req, kind, nil)
	require.NoError(t, err)

	assert.Equal(t, "0xfacil", out.Extra["facilitator"])

	contracts := out.Extra["contracts"].(map[string]interface{})
	assert.Equal(t, "0xsub", contracts["subscription"])
	assert.Equal(t, "0xpermit2", contracts["permit2"])

	domain := out.Extra["domain"].(map[string]interface{})
	assert.Equal(t, domainName, domain["name"])
	assert.Equal(t, domainVersion, domain["version"])
	assert.Equal(t, uint64(196), domain["chainId"])
	assert.Equal(t, "0xsub", domain["verifyingContract"])
}

func TestEnhancePaymentRequirementsDualReadsLegacyFacilitatorKey(t *testing.T) {
	scheme := NewPeriodScheme()
	req := types.PaymentRequirements{Scheme: SchemePeriod, Network: "eip155:196", PayTo: "0xmerchant"}
	kind := supportedKind(map[string]interface{}{
		"facilitator":          "0xlegacy",
		"subscriptionContract": "0xsub",
		"permit2Contract":      "0xpermit2",
	})

	out, err := scheme.EnhancePaymentRequirements(context.Background(), req, kind, nil)
	require.NoError(t, err)
	assert.Equal(t, "0xlegacy", out.Extra["facilitator"])
}

func TestEnhancePaymentRequirementsMerchantOverrideWins(t *testing.T) {
	scheme := NewPeriodScheme().
		WithFacilitator("0xoverride").
		WithSubscriptionContract("0xsuboverride").
		WithPermit2Contract("0xp2override")
	req := types.PaymentRequirements{Scheme: SchemePeriod, Network: "eip155:196", PayTo: "0xmerchant"}
	kind := supportedKind(map[string]interface{}{"facilitatorAddress": "0xfacil", "subscriptionContract": "0xsub", "permit2Contract": "0xpermit2"})

	out, err := scheme.EnhancePaymentRequirements(context.Background(), req, kind, nil)
	require.NoError(t, err)
	assert.Equal(t, "0xoverride", out.Extra["facilitator"])
	contracts := out.Extra["contracts"].(map[string]interface{})
	assert.Equal(t, "0xsuboverride", contracts["subscription"])
}

func TestEnhancePaymentRequirementsErrorsWhenFacilitatorMissing(t *testing.T) {
	scheme := NewPeriodScheme()
	req := types.PaymentRequirements{Scheme: SchemePeriod, Network: "eip155:196", PayTo: "0xmerchant"}
	kind := supportedKind(map[string]interface{}{"subscriptionContract": "0xsub", "permit2Contract": "0xpermit2"})

	_, err := scheme.EnhancePaymentRequirements(context.Background(), req, kind, nil)
	assert.Error(t, err)
}

func TestSchemeIdentifier(t *testing.T) {
	assert.Equal(t, "period", NewPeriodScheme().Scheme())
}
