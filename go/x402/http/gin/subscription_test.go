package gin

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/okx/payments/go/x402"
	x402http "github.com/okx/payments/go/x402/http"
	"github.com/okx/payments/go/x402/subscription"
)

func boolPtr(b bool) *bool { return &b }

// captureSubscriptionFacilitator records the syncSettle flag the middleware
// passes down when settling a period subscribe, and the subIds it cancels. It
// returns an already-active subscription so settlement does not poll.
type captureSubscriptionFacilitator struct {
	createSyncSettle      *bool
	cancelledSubID        string
	cancelledPendingSubID string
}

func (c *captureSubscriptionFacilitator) CreateSubscription(_ context.Context, req *subscription.CreateSubscriptionRequest) (*subscription.CreateSubscriptionResponse, error) {
	v := req.SyncSettle
	c.createSyncSettle = &v
	return &subscription.CreateSubscriptionResponse{SubID: "0xnew", State: subscription.StateActive}, nil
}
func (c *captureSubscriptionFacilitator) Charge(_ context.Context, _ *subscription.ChargeRequest) (*subscription.ChargeResponse, error) {
	return &subscription.ChargeResponse{}, nil
}
func (c *captureSubscriptionFacilitator) ChangeSubscription(_ context.Context, _ *subscription.ChangeSubscriptionRequest) (*subscription.ChangeResponse, error) {
	return &subscription.ChangeResponse{NewSubID: "0xnew", State: subscription.StateActive}, nil
}
func (c *captureSubscriptionFacilitator) CancelSubscription(_ context.Context, req *subscription.CancelSubscriptionRequest) (*subscription.TxResultResponse, error) {
	c.cancelledSubID = req.SubID
	return &subscription.TxResultResponse{SubID: req.SubID}, nil
}
func (c *captureSubscriptionFacilitator) CancelPendingChange(_ context.Context, req *subscription.CancelPendingChangeRequest) (*subscription.TxResultResponse, error) {
	c.cancelledPendingSubID = req.SubID
	return &subscription.TxResultResponse{SubID: req.SubID}, nil
}
func (c *captureSubscriptionFacilitator) FinalizeExpired(_ context.Context, req *subscription.FinalizeExpiredRequest) (*subscription.TxResultResponse, error) {
	return &subscription.TxResultResponse{SubID: req.SubID}, nil
}
func (c *captureSubscriptionFacilitator) GetSubscription(_ context.Context, _ string) (*subscription.SubscriptionStatus, error) {
	return nil, nil
}
func (c *captureSubscriptionFacilitator) GetCharges(_ context.Context, _ string, _, _ int) (*subscription.ChargesResponse, error) {
	return &subscription.ChargesResponse{}, nil
}
func (c *captureSubscriptionFacilitator) GetPendingChange(_ context.Context, _ string) (*subscription.PendingPlanChange, error) {
	return nil, nil
}

// periodSubscribeHeader builds a base64 PAYMENT-SIGNATURE for a period subscribe
// whose signed terms match the pro plan advertised by periodSubscribeRoutes.
func periodSubscribeHeader(t *testing.T) string {
	t.Helper()
	terms := subscription.SubscriptionTerms{
		Merchant:             "0xmerchant",
		Token:                "0xtok",
		AmountPerPeriod:      "5000",
		PeriodSec:            2592000,
		MaxPeriods:           12,
		PlanID:               "pro",
		PlanTier:             2,
		PeriodMode:           subscription.PeriodModeFixed,
		InitialChargePeriods: 1,
		InitialChargeAmount:  "5000",
		StartAt:              0,
		ChangeFromSubID:      "0x0000000000000000000000000000000000000000000000000000000000000000",
	}
	inner := subscription.SubscriptionPayloadInner{
		Terms:                 terms,
		TermsSignature:        "0xtermsig",
		PermitSingleSignature: "0xpermitsig",
	}
	innerJSON, err := json.Marshal(inner)
	if err != nil {
		t.Fatalf("marshal inner: %v", err)
	}
	var innerMap map[string]any
	if err := json.Unmarshal(innerJSON, &innerMap); err != nil {
		t.Fatalf("decode inner map: %v", err)
	}

	payload := x402.PaymentPayload{
		X402Version: 2,
		Payload:     innerMap,
		Accepted: x402.PaymentRequirements{
			Scheme:            subscription.SchemePeriod,
			Network:           "eip155:196",
			Asset:             "0xtok",
			PayTo:             "0xmerchant",
			Amount:            "5000",
			MaxTimeoutSeconds: 300,
		},
	}
	payloadJSON, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	return base64.StdEncoding.EncodeToString(payloadJSON)
}

// proPlanAccepts advertises the single pro plan shared by the subscribe and
// change routes.
func proPlanAccepts() x402http.PaymentOptions {
	plan := subscription.SubscriptionPlan{
		ID:                   "pro",
		Tier:                 2,
		Network:              "eip155:196",
		PayTo:                "0xmerchant",
		AmountPerPeriod:      "5000",
		PeriodSec:            2592000,
		PeriodMode:           subscription.PeriodModeFixed,
		MaxPeriods:           12,
		InitialChargePeriods: 1,
		InitialChargeAmount:  "5000",
	}
	return x402http.PaymentOptions{
		{
			Scheme:  subscription.SchemePeriod,
			Price:   map[string]any{"amount": "5000", "asset": "0xtok"},
			Network: "eip155:196",
			PayTo:   "0xmerchant",
			Extra:   plan.BuildExtra(),
		},
	}
}

// periodSubscribeRoutes advertises a single pro plan on POST /subscribe with the
// given per-route SyncSettle setting.
func periodSubscribeRoutes(syncSettle *bool) x402http.RoutesConfig {
	return x402http.RoutesConfig{
		"POST /subscribe": x402http.RouteConfig{
			Accepts:    proPlanAccepts(),
			SyncSettle: syncSettle,
		},
	}
}

// periodChangeRoutes advertises the pro plan on a change-operation route.
func periodChangeRoutes() x402http.RoutesConfig {
	return x402http.RoutesConfig{
		"POST /subscription/change": x402http.RouteConfig{
			Operation: x402http.OperationChange,
			Accepts:   proPlanAccepts(),
		},
	}
}

func newPeriodScheme() *subscription.PeriodScheme {
	return subscription.NewPeriodScheme().
		WithFacilitator("0xfacilitator").
		WithSubscriptionContract("0xsubscription").
		WithPermit2Contract("0xpermit2")
}

// TestPeriodSubscribeSettlesWithRouteSyncSettle verifies the gin period subscribe
// path derives the settlement mode from the route's SyncSettle: a nil pointer
// stays synchronous, and an explicit value is honored.
func TestPeriodSubscribeSettlesWithRouteSyncSettle(t *testing.T) {
	cases := []struct {
		name     string
		route    *bool
		expected bool
	}{
		{"nil defaults to synchronous", nil, true},
		{"explicit sync", boolPtr(true), true},
		{"explicit async", boolPtr(false), false},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			facilitator := &captureSubscriptionFacilitator{}
			support := subscription.NewSupport(facilitator, subscription.AccessWindowSecs)

			nextCalled := false
			router := createTestRouter()
			router.Use(X402Payment(Config{
				Routes:       periodSubscribeRoutes(tc.route),
				Schemes:      []SchemeConfig{{Network: "eip155:196", Server: newPeriodScheme()}},
				Subscription: support,
				Timeout:      5 * time.Second,
			}))
			router.POST("/subscribe", func(c *gin.Context) {
				nextCalled = true
				c.JSON(http.StatusOK, map[string]string{"data": "granted"})
			})

			req := httptest.NewRequest("POST", "/subscribe", nil)
			req.Header.Set("PAYMENT-SIGNATURE", periodSubscribeHeader(t))
			req.Host = "example.com"

			w := httptest.NewRecorder()
			router.ServeHTTP(w, req)

			if w.Code != http.StatusOK {
				t.Fatalf("expected status 200, got %d. Body: %s", w.Code, w.Body.String())
			}
			if !nextCalled {
				t.Error("expected the protected handler to be served after settlement")
			}
			if facilitator.createSyncSettle == nil {
				t.Fatal("expected CreateSubscription to be called")
			}
			if *facilitator.createSyncSettle != tc.expected {
				t.Errorf("expected syncSettle %v, got %v", tc.expected, *facilitator.createSyncSettle)
			}
			if w.Header().Get("PAYMENT-RESPONSE") == "" {
				t.Error("expected PAYMENT-RESPONSE header after subscribe settlement")
			}
		})
	}
}

// TestSubscriptionCancelRelays verifies a cancel operation route relays the
// signed authorization to the facilitator and returns its result.
func TestSubscriptionCancelRelays(t *testing.T) {
	facilitator := &captureSubscriptionFacilitator{}
	support := subscription.NewSupport(facilitator, subscription.AccessWindowSecs)

	routes := x402http.RoutesConfig{
		"POST /cancel": x402http.RouteConfig{
			Operation: x402http.OperationCancel,
		},
	}

	router := createTestRouter()
	router.Use(X402Payment(Config{
		Routes:       routes,
		Schemes:      []SchemeConfig{{Network: "eip155:196", Server: newPeriodScheme()}},
		Subscription: support,
		Timeout:      5 * time.Second,
	}))
	router.POST("/cancel", func(c *gin.Context) {
		c.JSON(http.StatusTeapot, map[string]string{"unexpected": "handler"})
	})

	body := bytes.NewBufferString(`{"subId":"0xdeadbeef"}`)
	req := httptest.NewRequest("POST", "/cancel", body)
	req.Host = "example.com"

	w := httptest.NewRecorder()
	router.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("expected status 200, got %d. Body: %s", w.Code, w.Body.String())
	}
	if facilitator.cancelledSubID != "0xdeadbeef" {
		t.Errorf("expected cancel relayed for 0xdeadbeef, got %q", facilitator.cancelledSubID)
	}
}

// TestSubscriptionAccessRejectsBadProof verifies a malformed APP-Access proof on
// a period route is rejected with 402 and the protected handler is not served.
func TestSubscriptionAccessRejectsBadProof(t *testing.T) {
	facilitator := &captureSubscriptionFacilitator{}
	support := subscription.NewSupport(facilitator, subscription.AccessWindowSecs)

	served := false
	router := createTestRouter()
	router.Use(X402Payment(Config{
		Routes:       periodSubscribeRoutes(nil),
		Schemes:      []SchemeConfig{{Network: "eip155:196", Server: newPeriodScheme()}},
		Subscription: support,
		Timeout:      5 * time.Second,
	}))
	router.POST("/subscribe", func(c *gin.Context) {
		served = true
		c.JSON(http.StatusOK, map[string]string{"data": "granted"})
	})

	req := httptest.NewRequest("POST", "/subscribe", nil)
	req.Header.Set(x402http.SubscriptionAccessHeader, "not-a-valid-proof")
	req.Host = "example.com"

	w := httptest.NewRecorder()
	router.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Fatalf("expected status 402, got %d. Body: %s", w.Code, w.Body.String())
	}
	if served {
		t.Error("protected handler must not run on a rejected access proof")
	}
}

// TestSubscriptionDispatchNoOpForExactRoute verifies attaching subscription
// support leaves a plain exact route on the generic payment pipeline.
func TestSubscriptionDispatchNoOpForExactRoute(t *testing.T) {
	mockClient := &mockFacilitatorClient{supportedFunc: func(ctx context.Context) (x402.SupportedResponse, error) {
		return x402.SupportedResponse{
			Kinds:      []x402.SupportedKind{{X402Version: 2, Scheme: "exact", Network: "eip155:1"}},
			Extensions: []string{},
			Signers:    make(map[string][]string),
		}, nil
	}}
	support := subscription.NewSupport(&captureSubscriptionFacilitator{}, subscription.AccessWindowSecs)

	routes := x402http.RoutesConfig{
		"GET /api": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{
				{Scheme: "exact", PayTo: "0xtest", Price: "$1.00", Network: "eip155:1"},
			},
		},
	}

	router := createTestRouter()
	router.Use(X402Payment(Config{
		Routes:       routes,
		Facilitator:  mockClient,
		Schemes:      []SchemeConfig{{Network: "eip155:1", Server: &mockSchemeServer{scheme: "exact"}}},
		Subscription: support,
		Timeout:      5 * time.Second,
	}))
	router.GET("/api", func(c *gin.Context) {
		c.JSON(http.StatusOK, map[string]string{"data": "protected"})
	})

	req := httptest.NewRequest("GET", "/api", nil)
	req.Header.Set("Accept", "application/json")
	w := httptest.NewRecorder()
	router.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Errorf("expected status 402 for unpaid exact route, got %d", w.Code)
	}
	if w.Header().Get("PAYMENT-REQUIRED") == "" {
		t.Error("expected PAYMENT-REQUIRED header from the generic pipeline")
	}
}

// TestSubscriptionCancelPendingRelays verifies a cancel-pending-change route
// relays the signed authorization to the facilitator and returns its result.
func TestSubscriptionCancelPendingRelays(t *testing.T) {
	facilitator := &captureSubscriptionFacilitator{}
	support := subscription.NewSupport(facilitator, subscription.AccessWindowSecs)

	routes := x402http.RoutesConfig{
		"POST /cancel-pending": x402http.RouteConfig{
			Operation: x402http.OperationCancelPendingChange,
		},
	}

	router := createTestRouter()
	router.Use(X402Payment(Config{
		Routes:       routes,
		Schemes:      []SchemeConfig{{Network: "eip155:196", Server: newPeriodScheme()}},
		Subscription: support,
		Timeout:      5 * time.Second,
	}))
	router.POST("/cancel-pending", func(c *gin.Context) {
		c.JSON(http.StatusTeapot, map[string]string{"unexpected": "handler"})
	})

	body := bytes.NewBufferString(`{"subId":"0xdeadbeef","newSubId":"0xfeedface"}`)
	req := httptest.NewRequest("POST", "/cancel-pending", body)
	req.Host = "example.com"

	w := httptest.NewRecorder()
	router.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("expected status 200, got %d. Body: %s", w.Code, w.Body.String())
	}
	if facilitator.cancelledPendingSubID != "0xdeadbeef" {
		t.Errorf("expected cancel-pending relayed for 0xdeadbeef, got %q", facilitator.cancelledPendingSubID)
	}
}

// TestSubscriptionCancelPendingRejectsIncompleteAuth verifies a cancel-pending
// authorization missing its target newSubId is rejected with 400 and never
// relayed to the facilitator.
func TestSubscriptionCancelPendingRejectsIncompleteAuth(t *testing.T) {
	facilitator := &captureSubscriptionFacilitator{}
	support := subscription.NewSupport(facilitator, subscription.AccessWindowSecs)

	routes := x402http.RoutesConfig{
		"POST /cancel-pending": x402http.RouteConfig{
			Operation: x402http.OperationCancelPendingChange,
		},
	}

	router := createTestRouter()
	router.Use(X402Payment(Config{
		Routes:       routes,
		Schemes:      []SchemeConfig{{Network: "eip155:196", Server: newPeriodScheme()}},
		Subscription: support,
		Timeout:      5 * time.Second,
	}))
	router.POST("/cancel-pending", func(c *gin.Context) {
		c.JSON(http.StatusTeapot, map[string]string{"unexpected": "handler"})
	})

	body := bytes.NewBufferString(`{"subId":"0xdeadbeef"}`)
	req := httptest.NewRequest("POST", "/cancel-pending", body)
	req.Host = "example.com"

	w := httptest.NewRecorder()
	router.ServeHTTP(w, req)

	if w.Code != http.StatusBadRequest {
		t.Fatalf("expected status 400, got %d. Body: %s", w.Code, w.Body.String())
	}
	if facilitator.cancelledPendingSubID != "" {
		t.Errorf("expected no relay on invalid auth, got %q", facilitator.cancelledPendingSubID)
	}
}

// TestSubscriptionChangeAccessDegradesToOffer verifies a change-operation route
// with an unverifiable access proof degrades to a 402 carrying a subscribe
// offer, rather than the bare 402 a normal route returns.
func TestSubscriptionChangeAccessDegradesToOffer(t *testing.T) {
	facilitator := &captureSubscriptionFacilitator{}
	support := subscription.NewSupport(facilitator, subscription.AccessWindowSecs)

	served := false
	router := createTestRouter()
	router.Use(X402Payment(Config{
		Routes:       periodChangeRoutes(),
		Schemes:      []SchemeConfig{{Network: "eip155:196", Server: newPeriodScheme()}},
		Subscription: support,
		Timeout:      5 * time.Second,
	}))
	router.POST("/subscription/change", func(c *gin.Context) {
		served = true
		c.JSON(http.StatusOK, map[string]string{"data": "granted"})
	})

	req := httptest.NewRequest("POST", "/subscription/change", nil)
	req.Header.Set(x402http.SubscriptionAccessHeader, "not-a-valid-proof")
	req.Host = "example.com"

	w := httptest.NewRecorder()
	router.ServeHTTP(w, req)

	if w.Code != http.StatusPaymentRequired {
		t.Fatalf("expected status 402, got %d. Body: %s", w.Code, w.Body.String())
	}
	if w.Header().Get("PAYMENT-REQUIRED") == "" {
		t.Error("expected a subscribe offer (PAYMENT-REQUIRED header) on the change degrade path")
	}
	if served {
		t.Error("protected handler must not run when access could not be verified")
	}
}
