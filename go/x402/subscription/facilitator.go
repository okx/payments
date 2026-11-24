package subscription

import "context"

// SubscriptionFacilitatorClient is the seller's view of the nine OKX
// subscription endpoints. Write methods carry subId in the request body; read
// methods carry it (and paging) in the query string. It is deliberately
// separate from the base verify/settle facilitator client so one-shot schemes
// need not implement it.
type SubscriptionFacilitatorClient interface {
	// CreateSubscription creates a subscription and force-charges the first
	// period. POST /subscriptions.
	CreateSubscription(ctx context.Context, req *CreateSubscriptionRequest) (*CreateSubscriptionResponse, error)
	// Charge pulls the next periodic charge. POST /subscriptions/charge.
	Charge(ctx context.Context, req *ChargeRequest) (*ChargeResponse, error)
	// ChangeSubscription upgrades or downgrades a subscription.
	// POST /subscriptions/change.
	ChangeSubscription(ctx context.Context, req *ChangeSubscriptionRequest) (*ChangeResponse, error)
	// CancelSubscription relays a signed cancel. POST /subscriptions/cancel.
	CancelSubscription(ctx context.Context, req *CancelSubscriptionRequest) (*TxResultResponse, error)
	// CancelPendingChange relays a signed cancel of a pending downgrade.
	// POST /subscriptions/cancel-pending-change.
	CancelPendingChange(ctx context.Context, req *CancelPendingChangeRequest) (*TxResultResponse, error)
	// FinalizeExpired settles the expiry marker for a lapsed subscription.
	// POST /subscriptions/finalize-expired.
	FinalizeExpired(ctx context.Context, req *FinalizeExpiredRequest) (*TxResultResponse, error)
	// GetSubscription reads a subscription's detail.
	// GET /subscriptions/detail?subId=.
	GetSubscription(ctx context.Context, subID string) (*SubscriptionStatus, error)
	// GetCharges reads a subscription's charge ledger.
	// GET /subscriptions/charges?subId=&limit=&offset=.
	GetCharges(ctx context.Context, subID string, limit, offset int) (*ChargesResponse, error)
	// GetPendingChange reads a subscription's pending downgrade, or (nil, nil)
	// when there is none. GET /subscriptions/pending?subId=.
	GetPendingChange(ctx context.Context, subID string) (*PendingPlanChange, error)
}
