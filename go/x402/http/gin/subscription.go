package gin

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
	x402http "github.com/okx/payments/go/x402/http"
	"github.com/okx/payments/go/x402/subscription"
	"github.com/okx/payments/go/x402/types"
)

// cancelBodyLimit bounds the cancel/cancel-pending request body read.
const cancelBodyLimit = 64 << 10

// handleSubscription dispatches period-scheme requests: cancel/cancel-pending
// operations, APP-Access access gating (normal + change routes), and
// PAYMENT-SIGNATURE subscribe/change settlement. It returns true when it has
// fully handled the request; false lets the generic pipeline run (e.g. a fresh
// subscribe 402, or a mixed route paid via another scheme).
func handleSubscription(c *gin.Context, server *x402http.HTTPServer, reqCtx x402http.HTTPRequestContext, ctx context.Context) bool {
	support := server.SubscriptionSupport()
	routeConfig, ok := server.ResolveRouteConfig(reqCtx.Path, reqCtx.Method)
	if !ok {
		return false
	}

	switch routeConfig.Operation {
	case x402http.OperationCancel:
		handleCancel(c, ctx, support)
		return true
	case x402http.OperationCancelPendingChange:
		handleCancelPending(c, ctx, support)
		return true
	}

	if header := c.GetHeader(x402http.SubscriptionAccessHeader); header != "" {
		accepts, err := server.BuildRouteAccepts(ctx, routeConfig, reqCtx)
		if err != nil {
			writeBare402(c, "failed to build subscription offers")
			return true
		}
		resource := resourceInfo(reqCtx, routeConfig)
		now := time.Now().Unix()
		if routeConfig.Operation == x402http.OperationChange {
			handleChangeAccess(c, server, support, header, accepts, resource, now, ctx)
		} else {
			handleNormalAccess(c, server, support, header, accepts, resource, now, ctx)
		}
		return true
	}

	if payload, ok := server.DecodeSubscriptionPayload(reqCtx); ok && payload.Accepted.Scheme == subscription.SchemePeriod {
		handleSubscribeOrChange(c, server, support, routeConfig, reqCtx, payload, ctx)
		return true
	}

	return false
}

// handleNormalAccess gates a protected route on a verified AccessProof, the
// veto hook, fail-closed planId gating and the period gate.
func handleNormalAccess(c *gin.Context, server *x402http.HTTPServer, support *subscription.SubscriptionSupport, header string, accepts []types.PaymentRequirements, resource *types.ResourceInfo, now int64, ctx context.Context) {
	result, aerr := support.VerifyAccess(ctx, header, accepts, now)
	if aerr != nil {
		switch aerr.Kind {
		case subscription.AccessNotActive:
			write402WithOffer(c, server, accepts, resource, aerr.Message)
		default:
			writeBare402(c, aerr.Message)
		}
		return
	}

	accepted := subscription.AcceptedPlanIDs(accepts)
	if len(accepted) > 0 {
		if result.PlanID == "" {
			write402WithOffer(c, server, accepts, resource, "subscription plan could not be determined for this resource")
			return
		}
		if !subscription.PlanIDAccepted(accepted, result.PlanID) {
			write402WithOffer(c, server, accepts, resource, "subscription plan "+result.PlanID+" is not accepted for this resource")
			return
		}
	}

	c.Next()
}

// handleChangeAccess returns change offers for the caller's current plan.
func handleChangeAccess(c *gin.Context, server *x402http.HTTPServer, support *subscription.SubscriptionSupport, header string, accepts []types.PaymentRequirements, resource *types.ResourceInfo, now int64, ctx context.Context) {
	result, aerr := support.VerifyAccess(ctx, header, accepts, now)
	if aerr != nil {
		if aerr.Kind == subscription.AccessDenied {
			writeBare402(c, aerr.Message)
			return
		}
		// Unauthorized / NotActive degrade to a plain subscribe offer.
		write402WithOffer(c, server, accepts, resource, "")
		return
	}

	offers := subscription.BuildChangeAccepts(accepts, result.SubID, result.PlanID, result.PlanTier)
	if len(offers) == 0 {
		writeBare402(c, "no change-plan options available for the current plan")
		return
	}
	write402WithOffer(c, server, offers, resource, "")
}

// handleSubscribeOrChange verifies a buyer PAYMENT-SIGNATURE against the route's
// offers, relays it to the facilitator (create or change), then serves the
// resource with a PAYMENT-RESPONSE header.
func handleSubscribeOrChange(c *gin.Context, server *x402http.HTTPServer, support *subscription.SubscriptionSupport, routeConfig *x402http.RouteConfig, reqCtx x402http.HTTPRequestContext, payload *types.PaymentPayload, ctx context.Context) {
	chainIndex, ok := subscription.ChainIndexFromNetwork(payload.Accepted.Network)
	if !ok {
		writeBare402(c, "unsupported network for subscription: "+payload.Accepted.Network)
		return
	}

	accepts, err := server.BuildRouteAccepts(ctx, routeConfig, reqCtx)
	if err != nil {
		writeBare402(c, "failed to build subscription offers")
		return
	}

	verified, err := support.VerifySubscription(payload, accepts, payload.Accepted.Asset)
	if err != nil {
		writeBare402(c, "subscription verify failed: "+err.Error())
		return
	}

	syncSettle := true
	if routeConfig.SyncSettle != nil {
		syncSettle = *routeConfig.SyncSettle
	}

	outcome, err := support.SettleSubscription(ctx, verified, chainIndex, syncSettle)
	if err != nil {
		writeBare402(c, "subscription settle failed: "+err.Error())
		return
	}

	if hdr := subscription.EncodeResponseHeader(outcome); hdr != "" {
		c.Header(x402http.PaymentResponseHeader, hdr)
	}
	c.Next()
}

// handleCancel relays a signed cancel authorization.
func handleCancel(c *gin.Context, ctx context.Context, support *subscription.SubscriptionSupport) {
	var auth subscription.CancelAuth
	if !readAuth(c, &auth) {
		return
	}
	if err := support.VerifyCancel(&auth); err != nil {
		writeError(c, http.StatusBadRequest, err.Error())
		return
	}
	resp, err := support.SettleCancel(ctx, &auth)
	if err != nil {
		writeError(c, http.StatusBadGateway, err.Error())
		return
	}
	writeJSON(c, http.StatusOK, resp)
}

// handleCancelPending relays a signed cancel of a pending downgrade.
func handleCancelPending(c *gin.Context, ctx context.Context, support *subscription.SubscriptionSupport) {
	var auth subscription.PendingChangeCancelAuth
	if !readAuth(c, &auth) {
		return
	}
	if err := support.VerifyCancelPending(&auth); err != nil {
		writeError(c, http.StatusBadRequest, err.Error())
		return
	}
	resp, err := support.SettleCancelPending(ctx, &auth)
	if err != nil {
		writeError(c, http.StatusBadGateway, err.Error())
		return
	}
	writeJSON(c, http.StatusOK, resp)
}

// readAuth reads and decodes a JSON auth body, writing a 400 on failure.
func readAuth(c *gin.Context, dst any) bool {
	body, err := io.ReadAll(io.LimitReader(c.Request.Body, cancelBodyLimit))
	if err != nil {
		writeError(c, http.StatusBadRequest, "failed to read request body")
		return false
	}
	if err := json.Unmarshal(body, dst); err != nil {
		writeError(c, http.StatusBadRequest, "invalid authorization payload")
		return false
	}
	return true
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
func write402WithOffer(c *gin.Context, server *x402http.HTTPServer, accepts []types.PaymentRequirements, resource *types.ResourceInfo, errMsg string) {
	instr, err := server.BuildSubscription402(accepts, resource, errMsg)
	if err != nil {
		writeBare402(c, "failed to build subscription offer")
		return
	}
	handlePaymentError(c, instr, nil)
}

// writeBare402 writes a 402 with just an error message and no offer header.
func writeBare402(c *gin.Context, msg string) {
	writeError(c, http.StatusPaymentRequired, msg)
}

// writeError writes a JSON error body with the given status and aborts.
func writeError(c *gin.Context, status int, msg string) {
	c.AbortWithStatusJSON(status, map[string]string{"error": msg})
}

// writeJSON writes a JSON body with the given status and aborts.
func writeJSON(c *gin.Context, status int, body any) {
	c.JSON(status, body)
	c.Abort()
}
