package http

import (
	"context"
	"encoding/json"
	"fmt"
	"net/url"
	"strconv"

	"github.com/okx/payments/go/x402/subscription"
)

// Subscription endpoints, all relative to okxBasePath.
const (
	subEndpointCreate        = "/subscriptions"
	subEndpointCharge        = "/subscriptions/charge"
	subEndpointChange        = "/subscriptions/change"
	subEndpointCancel        = "/subscriptions/cancel"
	subEndpointCancelPending = "/subscriptions/cancel-pending-change"
	subEndpointFinalize      = "/subscriptions/finalize-expired"
	subEndpointDetail        = "/subscriptions/detail"
	subEndpointCharges       = "/subscriptions/charges"
	subEndpointPending       = "/subscriptions/pending"
)

// postSubscription marshals a body, POSTs it, and decodes the response.
func postSubscription[T any](ctx context.Context, c *OKXFacilitatorClient, endpoint string, req any) (*T, error) {
	body, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("failed to encode request for %s: %w", endpoint, err)
	}
	data, err := c.doRequest(ctx, "POST", endpoint, body)
	if err != nil {
		return nil, err
	}
	var result T
	if err := json.Unmarshal(data, &result); err != nil {
		return nil, fmt.Errorf("failed to parse response from %s: %w", endpoint, err)
	}
	return &result, nil
}

// CreateSubscription creates a subscription and force-charges the first period.
func (c *OKXFacilitatorClient) CreateSubscription(ctx context.Context, req *subscription.CreateSubscriptionRequest) (*subscription.CreateSubscriptionResponse, error) {
	return postSubscription[subscription.CreateSubscriptionResponse](ctx, c, subEndpointCreate, req)
}

// Charge pulls the next periodic charge for a subscription.
func (c *OKXFacilitatorClient) Charge(ctx context.Context, req *subscription.ChargeRequest) (*subscription.ChargeResponse, error) {
	return postSubscription[subscription.ChargeResponse](ctx, c, subEndpointCharge, req)
}

// ChangeSubscription upgrades or downgrades a subscription.
func (c *OKXFacilitatorClient) ChangeSubscription(ctx context.Context, req *subscription.ChangeSubscriptionRequest) (*subscription.ChangeResponse, error) {
	return postSubscription[subscription.ChangeResponse](ctx, c, subEndpointChange, req)
}

// CancelSubscription relays a signed cancel authorization.
func (c *OKXFacilitatorClient) CancelSubscription(ctx context.Context, req *subscription.CancelSubscriptionRequest) (*subscription.TxResultResponse, error) {
	return postSubscription[subscription.TxResultResponse](ctx, c, subEndpointCancel, req)
}

// CancelPendingChange relays a signed cancel of a not-yet-effective downgrade.
func (c *OKXFacilitatorClient) CancelPendingChange(ctx context.Context, req *subscription.CancelPendingChangeRequest) (*subscription.TxResultResponse, error) {
	return postSubscription[subscription.TxResultResponse](ctx, c, subEndpointCancelPending, req)
}

// FinalizeExpired settles the expiry marker for a lapsed subscription.
func (c *OKXFacilitatorClient) FinalizeExpired(ctx context.Context, req *subscription.FinalizeExpiredRequest) (*subscription.TxResultResponse, error) {
	return postSubscription[subscription.TxResultResponse](ctx, c, subEndpointFinalize, req)
}

// GetSubscription reads a subscription's detail. subId rides in the (signed) query.
func (c *OKXFacilitatorClient) GetSubscription(ctx context.Context, subID string) (*subscription.SubscriptionStatus, error) {
	endpoint := subEndpointDetail + "?subId=" + url.QueryEscape(subID)
	data, err := c.doRequest(ctx, "GET", endpoint, nil)
	if err != nil {
		return nil, err
	}
	var result subscription.SubscriptionStatus
	if err := json.Unmarshal(data, &result); err != nil {
		return nil, fmt.Errorf("failed to parse subscription detail: %w", err)
	}
	return &result, nil
}

// GetCharges reads a subscription's charge ledger.
func (c *OKXFacilitatorClient) GetCharges(ctx context.Context, subID string, limit, offset int) (*subscription.ChargesResponse, error) {
	q := url.Values{}
	q.Set("subId", subID)
	q.Set("limit", strconv.Itoa(limit))
	q.Set("offset", strconv.Itoa(offset))
	endpoint := subEndpointCharges + "?" + q.Encode()
	data, err := c.doRequest(ctx, "GET", endpoint, nil)
	if err != nil {
		return nil, err
	}
	var result subscription.ChargesResponse
	if err := json.Unmarshal(data, &result); err != nil {
		return nil, fmt.Errorf("failed to parse charges: %w", err)
	}
	return &result, nil
}

// GetPendingChange reads a subscription's pending downgrade, returning (nil, nil)
// when the facilitator reports none.
func (c *OKXFacilitatorClient) GetPendingChange(ctx context.Context, subID string) (*subscription.PendingPlanChange, error) {
	endpoint := subEndpointPending + "?subId=" + url.QueryEscape(subID)
	data, err := c.doRequest(ctx, "GET", endpoint, nil)
	if err != nil {
		return nil, err
	}
	var result subscription.PendingPlanChange
	if err := json.Unmarshal(data, &result); err != nil {
		return nil, fmt.Errorf("failed to parse pending change: %w", err)
	}
	if result.SubID == "" && result.NewSubID == "" {
		return nil, nil
	}
	return &result, nil
}

// Compile-time assertion that the OKX client satisfies the subscription client.
var _ subscription.SubscriptionFacilitatorClient = (*OKXFacilitatorClient)(nil)
