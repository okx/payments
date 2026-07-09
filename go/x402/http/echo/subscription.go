package echo

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"time"

	"github.com/labstack/echo/v4"
	x402http "github.com/okx/payments/go/x402/http"
	"github.com/okx/payments/go/x402/subscription"
	"github.com/okx/payments/go/x402/types"
)

// cancelBodyLimit bounds the cancel/cancel-pending request body read.
const cancelBodyLimit = 64 << 10

// handleSubscription dispatches period-scheme requests: cancel/cancel-pending
// operations, APP-Access access gating (normal + change routes), and
// PAYMENT-SIGNATURE subscribe/change settlement. It reports handled=true when
// it has fully handled the request; handled=false lets the generic pipeline run
// (e.g. a fresh subscribe 402, or a mixed route paid via another scheme).
func handleSubscription(c echo.Context, next echo.HandlerFunc, server *x402http.HTTPServer, reqCtx x402http.HTTPRequestContext, ctx context.Context) (bool, error) {
	support := server.SubscriptionSupport()
	routeConfig, ok := server.ResolveRouteConfig(reqCtx.Path, reqCtx.Method)
	if !ok {
		return false, nil
	}

	switch routeConfig.Operation {
	case x402http.OperationCancel:
		return true, handleCancel(c, ctx, support)
	case x402http.OperationCancelPendingChange:
		return true, handleCancelPending(c, ctx, support)
	}

	if header := c.Request().Header.Get(x402http.SubscriptionAccessHeader); header != "" {
		accepts, err := server.BuildRouteAccepts(ctx, routeConfig, reqCtx)
		if err != nil {
			return true, writeBare402(c, "failed to build subscription offers")
		}
		resource := resourceInfo(reqCtx, routeConfig)
		now := time.Now().Unix()
		if routeConfig.Operation == x402http.OperationChange {
			return true, handleChangeAccess(c, server, support, header, accepts, resource, now, ctx)
		}
		return true, handleNormalAccess(c, next, server, support, header, accepts, resource, now, ctx)
	}

	if payload, ok := server.DecodeSubscriptionPayload(reqCtx); ok && payload.Accepted.Scheme == subscription.SchemePeriod {
		return true, handleSubscribeOrChange(c, next, server, support, routeConfig, reqCtx, payload, ctx)
	}

	return false, nil
}

// handleNormalAccess gates a protected route on a verified AccessProof, the
// veto hook, fail-closed planId gating and the period gate.
func handleNormalAccess(c echo.Context, next echo.HandlerFunc, server *x402http.HTTPServer, support *subscription.SubscriptionSupport, header string, accepts []types.PaymentRequirements, resource *types.ResourceInfo, now int64, ctx context.Context) error {
	result, aerr := support.VerifyAccess(ctx, header, accepts, now)
	if aerr != nil {
		switch aerr.Kind {
		case subscription.AccessNotActive:
			return write402WithOffer(c, server, accepts, resource, aerr.Message)
		default:
			return writeBare402(c, aerr.Message)
		}
	}

	accepted := subscription.AcceptedPlanIDs(accepts)
	if len(accepted) > 0 {
		if result.PlanID == "" {
			return write402WithOffer(c, server, accepts, resource, "subscription plan could not be determined for this resource")
		}
		if !subscription.PlanIDAccepted(accepted, result.PlanID) {
			return write402WithOffer(c, server, accepts, resource, "subscription plan "+result.PlanID+" is not accepted for this resource")
		}
	}

	return next(c)
}

// handleChangeAccess returns change offers for the caller's current plan.
func handleChangeAccess(c echo.Context, server *x402http.HTTPServer, support *subscription.SubscriptionSupport, header string, accepts []types.PaymentRequirements, resource *types.ResourceInfo, now int64, ctx context.Context) error {
	result, aerr := support.VerifyAccess(ctx, header, accepts, now)
	if aerr != nil {
		if aerr.Kind == subscription.AccessDenied {
			return writeBare402(c, aerr.Message)
		}
		// Unauthorized / NotActive degrade to a plain subscribe offer.
		return write402WithOffer(c, server, accepts, resource, "")
	}

	offers := subscription.BuildChangeAccepts(accepts, result.SubID, result.PlanID, result.PlanTier)
	if len(offers) == 0 {
		return writeBare402(c, "no change-plan options available for the current plan")
	}
	return write402WithOffer(c, server, offers, resource, "")
}

// handleSubscribeOrChange verifies a buyer PAYMENT-SIGNATURE against the route's
// offers, relays it to the facilitator (create or change), then serves the
// resource with a PAYMENT-RESPONSE header.
func handleSubscribeOrChange(c echo.Context, next echo.HandlerFunc, server *x402http.HTTPServer, support *subscription.SubscriptionSupport, routeConfig *x402http.RouteConfig, reqCtx x402http.HTTPRequestContext, payload *types.PaymentPayload, ctx context.Context) error {
	chainIndex, ok := subscription.ChainIndexFromNetwork(payload.Accepted.Network)
	if !ok {
		return writeBare402(c, "unsupported network for subscription: "+payload.Accepted.Network)
	}

	accepts, err := server.BuildRouteAccepts(ctx, routeConfig, reqCtx)
	if err != nil {
		return writeBare402(c, "failed to build subscription offers")
	}

	verified, err := support.VerifySubscription(payload, accepts, payload.Accepted.Asset)
	if err != nil {
		return writeBare402(c, "subscription verify failed: "+err.Error())
	}

	syncSettle := true
	if routeConfig.SyncSettle != nil {
		syncSettle = *routeConfig.SyncSettle
	}

	outcome, err := support.SettleSubscription(ctx, verified, chainIndex, syncSettle)
	if err != nil {
		return writeBare402(c, "subscription settle failed: "+err.Error())
	}

	if hdr := subscription.EncodeResponseHeader(outcome); hdr != "" {
		c.Response().Header().Set(x402http.PaymentResponseHeader, hdr)
	}
	return next(c)
}

// handleCancel relays a signed cancel authorization.
func handleCancel(c echo.Context, ctx context.Context, support *subscription.SubscriptionSupport) error {
	var auth subscription.CancelAuth
	if err, ok := readAuth(c, &auth); !ok {
		return err
	}
	if err := support.VerifyCancel(&auth); err != nil {
		return writeError(c, http.StatusBadRequest, err.Error())
	}
	resp, err := support.SettleCancel(ctx, &auth)
	if err != nil {
		return writeError(c, http.StatusBadGateway, err.Error())
	}
	return writeJSON(c, http.StatusOK, resp)
}

// handleCancelPending relays a signed cancel of a pending downgrade.
func handleCancelPending(c echo.Context, ctx context.Context, support *subscription.SubscriptionSupport) error {
	var auth subscription.PendingChangeCancelAuth
	if err, ok := readAuth(c, &auth); !ok {
		return err
	}
	if err := support.VerifyCancelPending(&auth); err != nil {
		return writeError(c, http.StatusBadRequest, err.Error())
	}
	resp, err := support.SettleCancelPending(ctx, &auth)
	if err != nil {
		return writeError(c, http.StatusBadGateway, err.Error())
	}
	return writeJSON(c, http.StatusOK, resp)
}

// readAuth reads and decodes a JSON auth body. It reports ok=false with a 400
// already written when the body cannot be read or decoded.
func readAuth(c echo.Context, dst any) (error, bool) {
	body, err := io.ReadAll(io.LimitReader(c.Request().Body, cancelBodyLimit))
	if err != nil {
		return writeError(c, http.StatusBadRequest, "failed to read request body"), false
	}
	if err := json.Unmarshal(body, dst); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid authorization payload"), false
	}
	return nil, true
}

// resourceInfo builds the 402 resource descriptor from the request + route.
func resourceInfo(reqCtx x402http.HTTPRequestContext, routeConfig *x402http.RouteConfig) *types.ResourceInfo {
	return &types.ResourceInfo{
		URL:         reqCtx.Adapter.GetURL(),
		Description: routeConfig.Description,
		MimeType:    routeConfig.MimeType,
	}
}

// write402WithOffer renders a 402 carrying the given accepts as the offer.
func write402WithOffer(c echo.Context, server *x402http.HTTPServer, accepts []types.PaymentRequirements, resource *types.ResourceInfo, errMsg string) error {
	instr, err := server.BuildSubscription402(accepts, resource, errMsg)
	if err != nil {
		return writeBare402(c, "failed to build subscription offer")
	}
	return handlePaymentError(c, instr)
}

// writeBare402 writes a 402 with just an error message and no offer header.
func writeBare402(c echo.Context, msg string) error {
	return writeError(c, http.StatusPaymentRequired, msg)
}

// writeError writes a JSON error body with the given status.
func writeError(c echo.Context, status int, msg string) error {
	return c.JSON(status, map[string]string{"error": msg})
}

// writeJSON writes a JSON body with the given status.
func writeJSON(c echo.Context, status int, body any) error {
	return c.JSON(status, body)
}
