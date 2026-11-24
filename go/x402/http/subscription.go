package http

import (
	"context"

	"github.com/okx/payments/go/x402/subscription"
	"github.com/okx/payments/go/x402/types"
)

// SubscriptionOperation marks a route as a subscription operation endpoint.
type SubscriptionOperation string

const (
	// OperationChange serves a 402 with change offers for the caller's plan.
	OperationChange SubscriptionOperation = "change"
	// OperationCancel relays a signed cancel authorization.
	OperationCancel SubscriptionOperation = "cancel"
	// OperationCancelPendingChange relays a signed cancel of a pending downgrade.
	OperationCancelPendingChange SubscriptionOperation = "cancel-pending-change"
)

// Header names used by the period flows.
const (
	// SubscriptionAccessHeader carries the buyer's base64(JSON) AccessProof.
	SubscriptionAccessHeader = subscription.AccessHeader
	// PaymentResponseHeader carries the create/change result on a served route.
	PaymentResponseHeader = "PAYMENT-RESPONSE"
)

// WithSubscription attaches subscription support, enabling the period flows.
// Returns the server for chaining.
func (s *x402HTTPResourceServer) WithSubscription(support *subscription.SubscriptionSupport) *x402HTTPResourceServer {
	s.subscription = support
	return s
}

// SubscriptionSupport returns the attached subscription support, or nil.
func (s *x402HTTPResourceServer) SubscriptionSupport() *subscription.SubscriptionSupport {
	return s.subscription
}

// SubscriptionEnabled reports whether subscription support is attached.
func (s *x402HTTPResourceServer) SubscriptionEnabled() bool {
	return s.subscription != nil
}

// ResolveRouteConfig returns the matched route configuration for a path/method,
// or (nil, false) when no route matches.
func (s *x402HTTPResourceServer) ResolveRouteConfig(path, method string) (*RouteConfig, bool) {
	config, _ := s.getRouteConfig(path, method)
	if config == nil {
		return nil, false
	}
	return config, true
}

// BuildRouteAccepts builds the payment requirements for a route's accepts,
// resolving dynamic payTo/price and applying scheme enhancements.
func (s *x402HTTPResourceServer) BuildRouteAccepts(ctx context.Context, routeConfig *RouteConfig, reqCtx HTTPRequestContext) ([]types.PaymentRequirements, error) {
	return s.BuildPaymentRequirementsFromOptions(ctx, routeConfig.Accepts, reqCtx)
}

// DecodeSubscriptionPayload decodes a PAYMENT-SIGNATURE into a payment payload,
// returning (nil, false) when the header is absent or malformed.
func (s *x402HTTPResourceServer) DecodeSubscriptionPayload(reqCtx HTTPRequestContext) (*types.PaymentPayload, bool) {
	payload, err := s.extractPaymentV2(reqCtx.Adapter)
	if err != nil || payload == nil {
		return nil, false
	}
	return payload, true
}

// BuildSubscription402 assembles a 402 response carrying the given accepts as
// the offer, with an optional error message.
func (s *x402HTTPResourceServer) BuildSubscription402(accepts []types.PaymentRequirements, resource *types.ResourceInfo, errMsg string) (*HTTPResponseInstructions, error) {
	paymentRequired := s.CreatePaymentRequiredResponse(accepts, resource, errMsg, nil)
	return s.createHTTPResponseV2(paymentRequired, false, nil, "", nil)
}

// RouteHasPeriodAccept reports whether any of the route's accepts use the period
// scheme.
func RouteHasPeriodAccept(routeConfig *RouteConfig) bool {
	for _, opt := range routeConfig.Accepts {
		if opt.Scheme == subscription.SchemePeriod {
			return true
		}
	}
	return false
}
