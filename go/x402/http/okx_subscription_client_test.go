package http

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/okx/payments/go/x402/subscription"
)

func TestOKXSubscriptionCreateSignsAndUnwraps(t *testing.T) {
	var gotPath, gotMethod string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		gotMethod = r.Method
		if r.Header.Get("OK-ACCESS-SIGN") == "" {
			t.Error("missing OK-ACCESS-SIGN header")
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"code":0,"data":{"subId":"0xnew","state":1,"txHash":"0xtx"}}`))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	resp, err := client.CreateSubscription(context.Background(), &subscription.CreateSubscriptionRequest{ChainIndex: 196})
	if err != nil {
		t.Fatalf("CreateSubscription: %v", err)
	}
	if gotMethod != "POST" || gotPath != "/api/v6/pay/x402/subscriptions" {
		t.Errorf("unexpected request: %s %s", gotMethod, gotPath)
	}
	if resp.SubID != "0xnew" || resp.State != 1 {
		t.Errorf("unexpected response: %+v", resp)
	}
}

func TestOKXSubscriptionGetDetailUsesQuery(t *testing.T) {
	var gotQuery string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.RawQuery
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"code":0,"data":{"subId":"0xabc","state":1,"planId":"pro"}}`))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	status, err := client.GetSubscription(context.Background(), "0xabc")
	if err != nil {
		t.Fatalf("GetSubscription: %v", err)
	}
	if gotQuery != "subId=0xabc" {
		t.Errorf("unexpected query: %s", gotQuery)
	}
	if status.PlanID != "pro" {
		t.Errorf("unexpected planId: %s", status.PlanID)
	}
}

func TestOKXSubscriptionErrorSurfacesMsg(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"code":51000,"msg":"insufficient allowance"}`))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	_, err := client.Charge(context.Background(), &subscription.ChargeRequest{SubID: "0xabc", SyncSettle: true})
	if err == nil {
		t.Fatal("expected error for non-zero code")
	}
}

func TestOKXSubscriptionPendingNullReturnsNil(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"code":0,"data":null}`))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	pending, err := client.GetPendingChange(context.Background(), "0xabc")
	if err != nil {
		t.Fatalf("GetPendingChange: %v", err)
	}
	if pending != nil {
		t.Errorf("expected nil pending change, got %+v", pending)
	}
}

func TestOKXSubscriptionFinalizeExpiredParsesTxResult(t *testing.T) {
	var gotPath, gotMethod string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		gotMethod = r.Method
		if r.Header.Get("OK-ACCESS-SIGN") == "" {
			t.Error("missing OK-ACCESS-SIGN header")
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"code":0,"data":{"subId":"0xexp","txHash":"0xfin","state":4}}`))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	resp, err := client.FinalizeExpired(context.Background(), &subscription.FinalizeExpiredRequest{SubID: "0xexp"})
	if err != nil {
		t.Fatalf("FinalizeExpired: %v", err)
	}
	if gotMethod != "POST" || gotPath != "/api/v6/pay/x402/subscriptions/finalize-expired" {
		t.Errorf("unexpected request: %s %s", gotMethod, gotPath)
	}
	if resp.SubID != "0xexp" || resp.TxHash == nil || *resp.TxHash != "0xfin" {
		t.Errorf("unexpected tx result: %+v", resp)
	}
	if resp.State == nil || *resp.State != 4 {
		t.Errorf("unexpected state: %+v", resp.State)
	}
}

func TestOKXSubscriptionFinalizeExpiredMapsErrorCode(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"code":51001,"msg":"subscription not expired"}`))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	_, err := client.FinalizeExpired(context.Background(), &subscription.FinalizeExpiredRequest{SubID: "0xexp"})
	if err == nil {
		t.Fatal("expected error for non-zero code")
	}
}

func TestOKXSubscriptionGetChargesParsesLedger(t *testing.T) {
	var gotQuery string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.RawQuery
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"code":0,"data":{"charges":[` +
			`{"subId":"0xabc","period":1,"chargeType":0,"amount":"5000","state":2,"txHash":"0xc1"},` +
			`{"subId":"0xabc","period":2,"chargeType":1,"amount":"5000","state":2,"planChangeTriggered":true,"newSubId":"0xdef"}` +
			`]}}`))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	resp, err := client.GetCharges(context.Background(), "0xabc", 10, 0)
	if err != nil {
		t.Fatalf("GetCharges: %v", err)
	}
	if gotQuery != "limit=10&offset=0&subId=0xabc" {
		t.Errorf("unexpected query: %s", gotQuery)
	}
	if len(resp.Charges) != 2 {
		t.Fatalf("expected 2 charges, got %d", len(resp.Charges))
	}
	if resp.Charges[0].Period != 1 || resp.Charges[0].Amount != "5000" {
		t.Errorf("unexpected first charge: %+v", resp.Charges[0])
	}
	if !resp.Charges[1].PlanChangeTriggered || resp.Charges[1].NewSubID == nil || *resp.Charges[1].NewSubID != "0xdef" {
		t.Errorf("unexpected second charge: %+v", resp.Charges[1])
	}
}

func TestOKXSubscriptionGetChargesMapsErrorCode(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"code":51002,"msg":"subscription not found"}`))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	_, err := client.GetCharges(context.Background(), "0xabc", 10, 0)
	if err == nil {
		t.Fatal("expected error for non-zero code")
	}
}

func TestOKXSubscriptionGetPendingChangeParsesChange(t *testing.T) {
	var gotQuery string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.RawQuery
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"code":0,"data":{"subId":"0xabc","newSubId":"0xdef","effectiveFromPeriod":3,"state":1}}`))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	pending, err := client.GetPendingChange(context.Background(), "0xabc")
	if err != nil {
		t.Fatalf("GetPendingChange: %v", err)
	}
	if gotQuery != "subId=0xabc" {
		t.Errorf("unexpected query: %s", gotQuery)
	}
	if pending == nil {
		t.Fatal("expected a pending change, got nil")
	}
	if pending.SubID != "0xabc" || pending.NewSubID != "0xdef" || pending.EffectiveFromPeriod != 3 || pending.State != 1 {
		t.Errorf("unexpected pending change: %+v", pending)
	}
}

func TestOKXSubscriptionGetPendingChangeMapsErrorCode(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"code":51003,"msg":"lookup failed"}`))
	}))
	defer server.Close()

	client := newTestOKXClient(t, server.URL)
	_, err := client.GetPendingChange(context.Background(), "0xabc")
	if err == nil {
		t.Fatal("expected error for non-zero code")
	}
}
