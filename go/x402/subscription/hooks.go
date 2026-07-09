package subscription

import "context"

// AccessContext is passed to the OnBeforeAccess hook. It carries the verified
// subscriber identity plus the route's accepted plans, so merchant policy can
// gate on tier ceilings or per-plan flags without an external catalog join.
type AccessContext struct {
	SubID   string
	Payer   string
	Accepts []AcceptedPlan
}

// AcceptedPlan is a route-accepted plan surfaced to the OnBeforeAccess hook.
type AcceptedPlan struct {
	PlanID   string
	PlanTier uint8
}

// BeforeAccessResult is the hook's verdict. The hook is veto-only: Abort denies
// access hard (no offer); it cannot force-grant.
type BeforeAccessResult struct {
	Abort  bool
	Reason string
}

// OnBeforeAccessHook runs after AccessProof verification and before the period
// gate. Returning Abort=true denies the request.
type OnBeforeAccessHook func(ctx context.Context, access AccessContext) BeforeAccessResult
