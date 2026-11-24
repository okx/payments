// x402 period-subscription demo — serves the period/subscription scheme.
//
//	GET  /health                            — health check
//	GET  /weather                           — Basic plan gated resource
//	GET  /premium                           — Pro plans gated resource
//	GET  /subscription/change               — plan-change offers
//	POST /subscription/cancel               — cancel a subscription
//	POST /subscription/cancel-pending       — cancel a scheduled downgrade
//	POST /subscription/merchant-deny-access — merchant veto hook
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"os"
	"sync"
	"time"

	"github.com/okx/payments/go/x402"
	x402http "github.com/okx/payments/go/x402/http"
	nethttpmw "github.com/okx/payments/go/x402/http/nethttp"
	"github.com/okx/payments/go/x402/subscription"
)

const subscriptionNetwork = "eip155:196"

func boolPtr(b bool) *bool { return &b }

func writeJSON(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(body) //nolint:errcheck
}

// demoPlans returns the demo's subscription catalog: a basic monthly plan and
// two pro tiers (monthly and a prepaid-year). Tiers are unique across plans —
// plan-change derives its direction from the tier and the contract rejects a
// same-tier change.
func demoPlans(payTo string) (basic, pro, proYearly subscription.SubscriptionPlan) {
	basic = subscription.SubscriptionPlan{
		ID:                   "basic_monthly",
		Tier:                 1,
		Network:              subscriptionNetwork,
		PayTo:                payTo,
		Price:                "$0.005",
		AmountPerPeriod:      "5000",
		PeriodSec:            2_592_000, // 30 days
		PeriodMode:           subscription.PeriodModeFixed,
		MaxPeriods:           12,
		InitialChargePeriods: 1,
		InitialChargeAmount:  "5000",
		MaxTimeoutSeconds:    600,
		Name:                 "Basic Monthly",
		Features:             []string{"api_basic"},
	}

	pro = basic
	pro.ID = "pro_monthly"
	pro.Tier = 2
	pro.Price = "$0.02"
	pro.AmountPerPeriod = "20000"
	pro.InitialChargeAmount = "20000"
	pro.Name = "Pro Monthly"
	pro.Features = []string{"api_basic", "api_pro"}

	// Yearly Pro: cheaper per period, the whole year prepaid (the initial charge
	// covers all 12 periods).
	proYearly = basic
	proYearly.ID = "pro_yearly"
	proYearly.Tier = 3
	proYearly.Price = "$0.192"
	proYearly.AmountPerPeriod = "16000"
	proYearly.InitialChargePeriods = 12
	proYearly.InitialChargeAmount = "192000"
	proYearly.Name = "Pro Yearly"
	proYearly.Features = []string{"api_basic", "api_pro"}
	return basic, pro, proYearly
}

// planOption turns a plan into a period payment option for a route.
func planOption(plan subscription.SubscriptionPlan) x402http.PaymentOption {
	return x402http.PaymentOption{
		Scheme:            subscription.SchemePeriod,
		Price:             plan.Price,
		Network:           x402.Network(plan.Network),
		PayTo:             plan.PayTo,
		MaxTimeoutSeconds: plan.MaxTimeoutSeconds,
		Extra:             plan.BuildExtra(),
	}
}

// setupSubscriptions registers the period scheme, its routes, the access/veto
// support and a background charge loop on the shared mux.
func setupSubscriptions(mux *http.ServeMux, client *x402http.OKXFacilitatorClient, payTo string) {
	basic, pro, proYearly := demoPlans(payTo)

	periodScheme := subscription.NewPeriodScheme()
	if c := os.Getenv("SUBSCRIPTION_CONTRACT"); c != "" {
		periodScheme.WithSubscriptionContract(c)
	}

	store := subscription.NewInMemoryStore()
	// denied holds subIds the merchant refuses access to; the veto hook rejects
	// them before the period gate so a canceled buyer's remaining paid access is
	// cut off immediately instead of running to the period end.
	var denied sync.Map // subId -> struct{}
	support := subscription.NewSupport(client, subscription.AccessWindowSecs).
		WithStore(store).
		OnBeforeAccess(func(_ context.Context, access subscription.AccessContext) subscription.BeforeAccessResult {
			if _, ok := denied.Load(access.SubID); ok {
				return subscription.BeforeAccessResult{Abort: true, Reason: "access denied by merchant for subscription " + access.SubID}
			}
			return subscription.BeforeAccessResult{}
		})

	routes := x402http.RoutesConfig{
		"GET /weather": {
			Accepts:     x402http.PaymentOptions{planOption(basic)},
			Description: "Weather data (Basic plan)",
			MimeType:    "application/json",
			SyncSettle:  boolPtr(true),
		},
		"GET /premium": {
			Accepts:     x402http.PaymentOptions{planOption(pro), planOption(proYearly)},
			Description: "Premium weather analytics (Pro plans only)",
			MimeType:    "application/json",
			SyncSettle:  boolPtr(true),
		},
		"GET /subscription/change": {
			Accepts:     x402http.PaymentOptions{planOption(basic), planOption(pro), planOption(proYearly)},
			Description: "Change your subscription plan",
			MimeType:    "application/json",
			Operation:   x402http.OperationChange,
			SyncSettle:  boolPtr(true),
		},
		"POST /subscription/cancel": {
			Description: "Cancel a subscription",
			Operation:   x402http.OperationCancel,
		},
		"POST /subscription/cancel-pending": {
			Description: "Cancel a scheduled downgrade",
			Operation:   x402http.OperationCancelPendingChange,
		},
	}

	subMux := http.NewServeMux()
	subMux.HandleFunc("GET /weather", weatherHandler)
	subMux.HandleFunc("GET /premium", weatherHandler)
	subMux.HandleFunc("GET /subscription/change", func(w http.ResponseWriter, _ *http.Request) {
		writeJSON(w, http.StatusOK, map[string]any{"result": "subscription plan changed"})
	})
	subMux.HandleFunc("POST /subscription/merchant-deny-access", func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			SubID string `json:"subId"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil || body.SubID == "" {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": "subId required"})
			return
		}
		denied.Store(body.SubID, struct{}{})
		writeJSON(w, http.StatusOK, map[string]any{"denied": body.SubID})
	})

	handler := nethttpmw.X402Payment(nethttpmw.Config{
		Routes:       routes,
		Facilitator:  client,
		Schemes:      []nethttpmw.SchemeConfig{{Network: subscriptionNetwork, Server: periodScheme}},
		Subscription: support,
		Timeout:      300 * time.Second,
	})(subMux)

	for _, pattern := range []string{
		"GET /weather",
		"GET /premium",
		"GET /subscription/change",
		"POST /subscription/cancel",
		"POST /subscription/cancel-pending",
		"POST /subscription/merchant-deny-access",
	} {
		mux.Handle(pattern, handler)
	}

	startChargeLoop(support)
}

// weatherHandler serves the gated weather report after access is granted.
func weatherHandler(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{"report": map[string]any{"weather": "sunny", "temperature": 23}})
}

// startChargeLoop runs a background scheduler that, every 60s, charges due
// subscriptions via ChargeAndRecord so the store follows plan-change successors.
func startChargeLoop(support *subscription.SubscriptionSupport) {
	go func() {
		ticker := time.NewTicker(60 * time.Second)
		defer ticker.Stop()
		for range ticker.C {
			ctx := context.Background()
			due, err := support.DueSubscriptions(ctx, uint64(time.Now().Unix()))
			if err != nil {
				continue
			}
			for _, rec := range due {
				outcome, err := support.ChargeAndRecord(ctx, rec.SubID, true)
				switch {
				case err != nil:
					fmt.Printf("charge %s failed (dunning): %v\n", rec.SubID, err)
				case outcome.PlanChangeTriggered && outcome.NewSubID != nil:
					fmt.Printf("downgrade activated: %s now %s\n", rec.SubID, *outcome.NewSubID)
				default:
					fmt.Printf("charged %s period %d state %d\n", rec.SubID, outcome.Period, outcome.State)
				}
			}
		}
	}()
}

func main() {
	payTo := os.Getenv("PAY_TO_ADDRESS")
	if payTo == "" {
		fmt.Println("PAY_TO_ADDRESS environment variable is required")
		os.Exit(1)
	}

	mux := http.NewServeMux()

	mux.HandleFunc("GET /health", func(w http.ResponseWriter, _ *http.Request) {
		writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
	})

	if baseURL := os.Getenv("OKX_BASE_URL"); baseURL != "" {
		client, err := x402http.NewOKXFacilitatorClient(&x402http.OKXFacilitatorConfig{
			Auth: x402http.OKXAuthConfig{
				APIKey:     os.Getenv("OKX_API_KEY"),
				SecretKey:  os.Getenv("OKX_SECRET_KEY"),
				Passphrase: os.Getenv("OKX_PASSPHRASE"),
			},
			BaseURL:    baseURL,
			SyncSettle: boolPtr(true),
		})
		if err != nil {
			log.Fatalf("Failed to create subscription client: %v", err)
		}

		setupSubscriptions(mux, client, payTo)

		fmt.Println("Subscription routes enabled: GET /weather, GET /premium, GET /subscription/change, POST /subscription/cancel, POST /subscription/cancel-pending, POST /subscription/merchant-deny-access")
	} else {
		fmt.Println("Subscription routes disabled (OKX_BASE_URL not set)")
	}

	port := os.Getenv("PORT")
	if port == "" {
		port = "4002"
	}
	fmt.Printf("x402 period-subscription server listening on :%s\n", port)
	if err := http.ListenAndServe(":"+port, mux); err != nil {
		fmt.Printf("Error starting server: %v\n", err)
		os.Exit(1)
	}
}
