package subscription

import "github.com/okx/payments/go/x402/types"

// Change direction and effective-at markers written into a change offer's
// extra.changeFrom.
const (
	directionUpgrade   = "upgrade"
	directionDowngrade = "downgrade"
	effectiveImmediate = "immediate"
	effectivePeriodEnd = "period_end"
)

// BuildChangeAccepts turns a route's period accepts into change offers for a
// subscriber currently on (fromPlanID, fromTier). The plan the buyer is already
// on — matched by planId OR tier — is dropped. A higher tier is an immediate
// upgrade; a lower tier is a period-end downgrade, and a downgrade offer strips
// initialCharge (the contract rejects a scheduled downgrade that charges
// upfront). Each offer records where the change comes from under changeFrom.
func BuildChangeAccepts(accepts []types.PaymentRequirements, fromSubID, fromPlanID string, fromTier uint8) []types.PaymentRequirements {
	out := make([]types.PaymentRequirements, 0, len(accepts))
	for _, accept := range accepts {
		if accept.Scheme != SchemePeriod || accept.Extra == nil {
			continue
		}
		planID, planTier := planFields(accept.Extra)
		if planID == fromPlanID || planTier == fromTier {
			continue
		}

		direction, effectiveAt := directionUpgrade, effectiveImmediate
		if fromTier >= planTier {
			direction, effectiveAt = directionDowngrade, effectivePeriodEnd
		}

		offer := accept
		offer.Extra = cloneExtra(accept.Extra)
		if direction == directionDowngrade {
			delete(offer.Extra, "initialCharge")
		}
		offer.Extra["changeFrom"] = map[string]interface{}{
			"fromSubId":    fromSubID,
			"fromPlanId":   fromPlanID,
			"fromPlanTier": fromTier,
			"direction":    direction,
			"effectiveAt":  effectiveAt,
		}
		out = append(out, offer)
	}
	return out
}

// AcceptedPlanIDs collects the plan ids of a route's period accepts.
func AcceptedPlanIDs(accepts []types.PaymentRequirements) []string {
	ids := make([]string, 0, len(accepts))
	for _, accept := range accepts {
		if accept.Scheme != SchemePeriod || accept.Extra == nil {
			continue
		}
		if id, _ := planFields(accept.Extra); id != "" {
			ids = append(ids, id)
		}
	}
	return ids
}

// AcceptedPlans collects (id, tier) for a route's period accepts, for the
// OnBeforeAccess hook context.
func AcceptedPlans(accepts []types.PaymentRequirements) []AcceptedPlan {
	plans := make([]AcceptedPlan, 0, len(accepts))
	for _, accept := range accepts {
		if accept.Scheme != SchemePeriod || accept.Extra == nil {
			continue
		}
		id, tier := planFields(accept.Extra)
		plans = append(plans, AcceptedPlan{PlanID: id, PlanTier: tier})
	}
	return plans
}

// PlanIDAccepted reports whether subPlanID is in the accepted list.
func PlanIDAccepted(accepted []string, subPlanID string) bool {
	for _, id := range accepted {
		if id == subPlanID {
			return true
		}
	}
	return false
}

// cloneExtra shallow-copies an extra map so change-offer mutations do not affect
// the source accept.
func cloneExtra(extra map[string]interface{}) map[string]interface{} {
	clone := make(map[string]interface{}, len(extra))
	for k, v := range extra {
		clone[k] = v
	}
	return clone
}
