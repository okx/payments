package subscription

// SubscriptionPlan describes a seller's subscription tier. It is the source for
// a route's period accept: BuildExtra assembles the plan-specific fields the
// buyer signs into terms and the SDK later binds against.
type SubscriptionPlan struct {
	ID                   string
	Tier                 uint8
	Network              string
	PayTo                string
	Price                string
	AmountPerPeriod      string
	PeriodSec            uint64
	PeriodMode           uint8
	MaxPeriods           uint32
	StartAt              uint64
	InitialChargePeriods uint32
	InitialChargeAmount  string
	MaxTimeoutSeconds    int
	Name                 string
	Features             []string
}

// BuildExtra assembles the plan's payment-requirements extra. initialCharge is
// present only when the plan charges upfront periods.
func (p *SubscriptionPlan) BuildExtra() map[string]interface{} {
	plan := map[string]interface{}{
		"id":   p.ID,
		"tier": p.Tier,
	}
	if p.Name != "" {
		plan["name"] = p.Name
	}
	if len(p.Features) > 0 {
		plan["features"] = p.Features
	}

	extra := map[string]interface{}{
		"amountPerPeriod": p.AmountPerPeriod,
		"periodSec":       p.PeriodSec,
		"periodMode":      p.PeriodMode,
		"maxPeriods":      p.MaxPeriods,
		"startAt":         p.StartAt,
	}
	if p.InitialChargePeriods > 0 {
		extra["initialCharge"] = map[string]interface{}{
			"periodCount":        p.InitialChargePeriods,
			"totalAmount":        p.InitialChargeAmount,
			"coversFirstPeriods": true,
		}
	}
	extra["plan"] = plan
	return extra
}
