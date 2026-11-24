package subscription

import (
	"context"
	"fmt"

	"github.com/okx/payments/go/x402"
	"github.com/okx/payments/go/x402/mechanisms/evm"
	exactserver "github.com/okx/payments/go/x402/mechanisms/evm/exact/server"
	"github.com/okx/payments/go/x402/types"
)

// domainName is the EIP-712 domain name buyers sign subscription terms under.
const domainName = "A2APaySubscription"
const domainVersion = "1"

// PeriodScheme builds 402 payment requirements for the period scheme. It reuses
// the exact scheme's price parsing and injects the subscription contracts, the
// facilitator EOA, and the EIP-712 domain into each accept's extra. Merchant
// overrides take precedence over whatever /supported advertises.
type PeriodScheme struct {
	exact                *exactserver.ExactEvmScheme
	facilitator          string
	subscriptionContract string
	permit2Contract      string
}

// NewPeriodScheme creates a PeriodScheme with no merchant overrides.
func NewPeriodScheme() *PeriodScheme {
	return &PeriodScheme{exact: exactserver.NewExactEvmScheme()}
}

// WithFacilitator sets a merchant override for the facilitator EOA.
func (s *PeriodScheme) WithFacilitator(addr string) *PeriodScheme {
	s.facilitator = addr
	return s
}

// WithSubscriptionContract sets a merchant override for the subscription contract.
func (s *PeriodScheme) WithSubscriptionContract(addr string) *PeriodScheme {
	s.subscriptionContract = addr
	return s
}

// WithPermit2Contract sets a merchant override for the Permit2 contract.
func (s *PeriodScheme) WithPermit2Contract(addr string) *PeriodScheme {
	s.permit2Contract = addr
	return s
}

// Scheme returns the wire scheme string.
func (s *PeriodScheme) Scheme() string { return SchemePeriod }

// ParsePrice parses a plan price into an atomic asset amount.
func (s *PeriodScheme) ParsePrice(price x402.Price, network x402.Network) (x402.AssetAmount, error) {
	return s.exact.ParsePrice(price, network)
}

// EnhancePaymentRequirements injects the subscription contracts, facilitator EOA
// and EIP-712 domain into the requirements' extra, each only if absent (a
// merchant-supplied value is preserved).
func (s *PeriodScheme) EnhancePaymentRequirements(
	_ context.Context,
	requirements types.PaymentRequirements,
	supportedKind types.SupportedKind,
	_ []string,
) (types.PaymentRequirements, error) {
	network := requirements.Network
	if requirements.Extra == nil {
		requirements.Extra = make(map[string]interface{})
	}

	if _, ok := requirements.Extra["facilitator"]; !ok {
		addr, err := s.resolveFacilitator(supportedKind)
		if err != nil {
			return requirements, err
		}
		requirements.Extra["facilitator"] = addr
	}

	if _, ok := requirements.Extra["contracts"]; !ok {
		subscription, err := s.resolveAddr(s.subscriptionContract, supportedKind, "subscriptionContract", "subscription contract", network)
		if err != nil {
			return requirements, err
		}
		permit2, err := s.resolveAddr(s.permit2Contract, supportedKind, "permit2Contract", "permit2 contract", network)
		if err != nil {
			return requirements, err
		}
		requirements.Extra["contracts"] = map[string]interface{}{
			"subscription": subscription,
			"permit2":      permit2,
		}
	}

	if _, ok := requirements.Extra["domain"]; !ok {
		subscription, err := s.resolveAddr(s.subscriptionContract, supportedKind, "subscriptionContract", "subscription contract", network)
		if err != nil {
			return requirements, err
		}
		chainID, err := evm.GetEvmChainId(network)
		if err != nil {
			return requirements, err
		}
		requirements.Extra["domain"] = map[string]interface{}{
			"name":              domainName,
			"version":           domainVersion,
			"chainId":           chainID.Uint64(),
			"verifyingContract": subscription,
		}
	}

	return requirements, nil
}

// resolveFacilitator sources the facilitator EOA: merchant override, then the
// facilitatorAddress key other schemes use, then period's legacy facilitator
// key. The 402 offer always writes it under "facilitator".
func (s *PeriodScheme) resolveFacilitator(kind types.SupportedKind) (string, error) {
	if s.facilitator != "" {
		return s.facilitator, nil
	}
	if v, ok := extraStr(kind.Extra, "facilitatorAddress"); ok {
		return v, nil
	}
	if v, ok := extraStr(kind.Extra, "facilitator"); ok {
		return v, nil
	}
	return "", fmt.Errorf("period: missing facilitator address; /supported advertised neither `facilitatorAddress` nor `facilitator` and no merchant override is set")
}

// resolveAddr sources a contract address: merchant override, else the named
// /supported key.
func (s *PeriodScheme) resolveAddr(override string, kind types.SupportedKind, key, what, network string) (string, error) {
	if override != "" {
		return override, nil
	}
	if v, ok := extraStr(kind.Extra, key); ok {
		return v, nil
	}
	return "", fmt.Errorf("period: missing %s for network %s; /supported did not advertise `%s` and no merchant override is set", what, network, key)
}

// Compile-time assertion that PeriodScheme satisfies the server scheme contract.
var _ x402.SchemeNetworkServer = (*PeriodScheme)(nil)
