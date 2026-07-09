package subscription

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/okx/payments/go/x402/types"
)

func strptr(s string) *string { return &s }

// fakeFacilitator is a configurable SubscriptionFacilitatorClient for tests.
type fakeFacilitator struct {
	chargeResp *ChargeResponse
	chargeErr  error
	createResp *CreateSubscriptionResponse
	changeResp *ChangeResponse
	statuses   map[string]*SubscriptionStatus
	getCalls   []string
}

func (f *fakeFacilitator) CreateSubscription(_ context.Context, _ *CreateSubscriptionRequest) (*CreateSubscriptionResponse, error) {
	if f.createResp != nil {
		return f.createResp, nil
	}
	return &CreateSubscriptionResponse{SubID: "created", State: StateActive}, nil
}
func (f *fakeFacilitator) Charge(_ context.Context, _ *ChargeRequest) (*ChargeResponse, error) {
	return f.chargeResp, f.chargeErr
}
func (f *fakeFacilitator) ChangeSubscription(_ context.Context, _ *ChangeSubscriptionRequest) (*ChangeResponse, error) {
	if f.changeResp != nil {
		return f.changeResp, nil
	}
	return &ChangeResponse{NewSubID: "changed", State: StateActive}, nil
}
func (f *fakeFacilitator) CancelSubscription(_ context.Context, req *CancelSubscriptionRequest) (*TxResultResponse, error) {
	return &TxResultResponse{SubID: req.SubID}, nil
}
func (f *fakeFacilitator) CancelPendingChange(_ context.Context, req *CancelPendingChangeRequest) (*TxResultResponse, error) {
	return &TxResultResponse{SubID: req.SubID}, nil
}
func (f *fakeFacilitator) FinalizeExpired(_ context.Context, req *FinalizeExpiredRequest) (*TxResultResponse, error) {
	return &TxResultResponse{SubID: req.SubID}, nil
}
func (f *fakeFacilitator) GetSubscription(_ context.Context, subID string) (*SubscriptionStatus, error) {
	f.getCalls = append(f.getCalls, subID)
	if st, ok := f.statuses[subID]; ok {
		return st, nil
	}
	return nil, errors.New("not found")
}
func (f *fakeFacilitator) GetCharges(_ context.Context, _ string, _, _ int) (*ChargesResponse, error) {
	return &ChargesResponse{}, nil
}
func (f *fakeFacilitator) GetPendingChange(_ context.Context, _ string) (*PendingPlanChange, error) {
	return nil, nil
}

func honestTerms() SubscriptionTerms {
	return SubscriptionTerms{
		Merchant:             "0xmerchant",
		Token:                "0xtok",
		AmountPerPeriod:      "5000",
		PeriodSec:            2592000,
		MaxPeriods:           12,
		PlanID:               "pro",
		PlanTier:             2,
		PeriodMode:           PeriodModeFixed,
		InitialChargePeriods: 1,
		InitialChargeAmount:  "5000",
		StartAt:              0,
		ChangeFromSubID:      "0x0000000000000000000000000000000000000000000000000000000000000000",
	}
}

func TestVerifyTermsMatchOfferAcceptsHonest(t *testing.T) {
	support := NewSupport(&fakeFacilitator{}, AccessWindowSecs)
	accepts := []types.PaymentRequirements{planAccept("pro", 2, true)}
	terms := honestTerms()
	assert.NoError(t, support.VerifyTermsMatchOffer(&terms, accepts, "0xtok"))
}

func TestVerifyTermsMatchOfferRejections(t *testing.T) {
	support := NewSupport(&fakeFacilitator{}, AccessWindowSecs)
	accepts := []types.PaymentRequirements{planAccept("pro", 2, true)}

	mutations := map[string]func(*SubscriptionTerms){
		"unknown_plan":     func(tm *SubscriptionTerms) { tm.PlanID = "enterprise" },
		"wrong_merchant":   func(tm *SubscriptionTerms) { tm.Merchant = "0xattacker" },
		"underpay":         func(tm *SubscriptionTerms) { tm.AmountPerPeriod = "1" },
		"wrong_period_sec": func(tm *SubscriptionTerms) { tm.PeriodSec = 60 },
		"wrong_max":        func(tm *SubscriptionTerms) { tm.MaxPeriods = 99 },
		"wrong_mode":       func(tm *SubscriptionTerms) { tm.PeriodMode = PeriodModeCalendarMonth },
		"wrong_tier":       func(tm *SubscriptionTerms) { tm.PlanTier = 3 },
		"token_mismatch":   func(tm *SubscriptionTerms) { tm.Token = "0xother" },
		"initial_charge":   func(tm *SubscriptionTerms) { tm.InitialChargeAmount = "1" },
		"start_at_pinned":  func(tm *SubscriptionTerms) { tm.StartAt = 5 },
	}
	for name, mutate := range mutations {
		t.Run(name, func(t *testing.T) {
			terms := honestTerms()
			mutate(&terms)
			assert.Error(t, support.VerifyTermsMatchOffer(&terms, accepts, "0xtok"))
		})
	}
}

func TestVerifyTermsMatchOfferSkipsInitialChargeOnChange(t *testing.T) {
	support := NewSupport(&fakeFacilitator{}, AccessWindowSecs)
	accepts := []types.PaymentRequirements{planAccept("pro", 2, true)}
	terms := honestTerms()
	terms.ChangeFromSubID = "0xdead" // a change: initialCharge not bound
	terms.InitialChargeAmount = "999999"
	terms.InitialChargePeriods = 7
	assert.NoError(t, support.VerifyTermsMatchOffer(&terms, accepts, "0xtok"))
}

func TestChargeAndRecordSwitchesToSuccessorOnPlanChange(t *testing.T) {
	ctx := context.Background()
	fake := &fakeFacilitator{
		chargeResp: &ChargeResponse{SubID: "old", State: StateActive, Period: 4, PlanChangeTriggered: true, NewSubID: strptr("new")},
		statuses: map[string]*SubscriptionStatus{
			"old": {SubID: "old", State: StateActive, ElapsedPeriods: 1, LastChargedPeriod: 1},
			"new": {SubID: "new", State: StateActive, ElapsedPeriods: 1, LastChargedPeriod: 1, PlanID: "basic", PlanTier: 1, StartAt: 100},
		},
	}
	store := NewInMemoryStore()
	support := NewSupport(fake, AccessWindowSecs).WithStore(store)
	support.pollInterval = 0

	out, err := support.ChargeAndRecord(ctx, "old", true)
	require.NoError(t, err)
	assert.True(t, out.PlanChangeTriggered)
	require.NotNil(t, out.NewSubID)
	assert.Equal(t, "new", *out.NewSubID)

	rec, _ := store.Get(ctx, "new")
	require.NotNil(t, rec, "successor subscription should be recorded")
}

func TestChargeAndRecordPollsPending(t *testing.T) {
	ctx := context.Background()
	fake := &fakeFacilitator{
		chargeResp: &ChargeResponse{SubID: "s", State: StatePending, Period: 2},
		statuses: map[string]*SubscriptionStatus{
			"s": {SubID: "s", State: StateActive, ElapsedPeriods: 2, LastChargedPeriod: 2},
		},
	}
	support := NewSupport(fake, AccessWindowSecs).WithStore(NewInMemoryStore())
	support.pollInterval = 0

	out, err := support.ChargeAndRecord(ctx, "s", true)
	require.NoError(t, err)
	assert.Equal(t, StateActive, out.State)
}

func accessHeader(t *testing.T, proof *AccessProof) string {
	t.Helper()
	raw, err := json.Marshal(proof)
	require.NoError(t, err)
	return base64.StdEncoding.EncodeToString(raw)
}

func TestVerifyAccessGrantsChargedSubscription(t *testing.T) {
	subID := "0x" + strings.Repeat("aa", 32)
	now := int64(1_700_000_000)
	proof, payer := signedProof(t, subID, uint64(now))

	fake := &fakeFacilitator{
		statuses: map[string]*SubscriptionStatus{
			subID: {SubID: subID, State: StateActive, Payer: payer, PlanID: "pro", PlanTier: 2, ElapsedPeriods: 1, LastChargedPeriod: 1, StartAt: 1000, PeriodSec: 100},
		},
	}
	support := NewSupport(fake, AccessWindowSecs).WithStore(NewInMemoryStore())
	accepts := []types.PaymentRequirements{planAccept("pro", 2, true)}

	result, aerr := support.VerifyAccess(context.Background(), accessHeader(t, proof), accepts, now)
	require.Nil(t, aerr)
	require.NotNil(t, result)
	assert.Equal(t, "pro", result.PlanID)
}

func TestVerifyAccessNotActiveWhenUnpaid(t *testing.T) {
	subID := "0x" + strings.Repeat("bb", 32)
	now := int64(1_700_000_000)
	proof, payer := signedProof(t, subID, uint64(now))

	fake := &fakeFacilitator{
		statuses: map[string]*SubscriptionStatus{
			subID: {SubID: subID, State: StateActive, Payer: payer, PlanID: "pro", PlanTier: 2, ElapsedPeriods: 3, LastChargedPeriod: 1, StartAt: 1000, PeriodSec: 100},
		},
	}
	support := NewSupport(fake, AccessWindowSecs)
	accepts := []types.PaymentRequirements{planAccept("pro", 2, true)}

	_, aerr := support.VerifyAccess(context.Background(), accessHeader(t, proof), accepts, now)
	require.NotNil(t, aerr)
	assert.Equal(t, AccessNotActive, aerr.Kind)
}

func TestVerifyAccessDeniedByHook(t *testing.T) {
	subID := "0x" + strings.Repeat("cc", 32)
	now := int64(1_700_000_000)
	proof, _ := signedProof(t, subID, uint64(now))

	support := NewSupport(&fakeFacilitator{}, AccessWindowSecs).
		OnBeforeAccess(func(_ context.Context, _ AccessContext) BeforeAccessResult {
			return BeforeAccessResult{Abort: true, Reason: "blocked"}
		})

	_, aerr := support.VerifyAccess(context.Background(), accessHeader(t, proof), nil, now)
	require.NotNil(t, aerr)
	assert.Equal(t, AccessDenied, aerr.Kind)
}

func TestVerifyCancelValidation(t *testing.T) {
	support := NewSupport(&fakeFacilitator{}, AccessWindowSecs)
	assert.Error(t, support.VerifyCancel(&CancelAuth{}))
	assert.NoError(t, support.VerifyCancel(&CancelAuth{SubID: "0xabc"}))

	assert.Error(t, support.VerifyCancelPending(&PendingChangeCancelAuth{SubID: "0xabc"}))
	assert.NoError(t, support.VerifyCancelPending(&PendingChangeCancelAuth{SubID: "0xabc", NewSubID: "0xdef"}))
}

func TestEncodeResponseHeader(t *testing.T) {
	hdr := EncodeResponseHeader(&SubmitOutcome{SubID: "0xabc", TxHash: strptr("0xtx"), State: StateActive})
	raw, err := base64.StdEncoding.DecodeString(hdr)
	require.NoError(t, err)
	var body map[string]interface{}
	require.NoError(t, json.Unmarshal(raw, &body))
	assert.Equal(t, "0xabc", body["subId"])
	assert.Equal(t, "0xtx", body["txHash"])
}

func verifiedFromTerms(terms SubscriptionTerms) *VerifiedSubscription {
	return &VerifiedSubscription{Inner: &SubscriptionPayloadInner{Terms: terms}}
}

func TestVerifySubscriptionBindsToOffer(t *testing.T) {
	support := NewSupport(&fakeFacilitator{}, AccessWindowSecs)
	payload := payloadFromInner(t, &SubscriptionPayloadInner{Terms: honestTerms(), TermsSignature: "0xsig"})
	accepts := []types.PaymentRequirements{planAccept("pro", 2, true)}

	v, err := support.VerifySubscription(payload, accepts, "0xtok")
	require.NoError(t, err)
	require.NotNil(t, v)
	assert.Equal(t, "pro", v.Inner.Terms.PlanID)
}

func TestVerifySubscriptionRejectsUnofferedPlan(t *testing.T) {
	support := NewSupport(&fakeFacilitator{}, AccessWindowSecs)
	terms := honestTerms()
	terms.PlanID = "enterprise"
	payload := payloadFromInner(t, &SubscriptionPayloadInner{Terms: terms})
	accepts := []types.PaymentRequirements{planAccept("pro", 2, true)}

	_, err := support.VerifySubscription(payload, accepts, "0xtok")
	assert.Error(t, err)
}

func TestSettleSubscriptionCreateRecordsActiveOutcome(t *testing.T) {
	ctx := context.Background()
	fake := &fakeFacilitator{
		createResp: &CreateSubscriptionResponse{SubID: "sub-new", State: StateActive},
		statuses:   map[string]*SubscriptionStatus{"sub-new": {SubID: "sub-new", State: StateActive}},
	}
	store := NewInMemoryStore()
	support := NewSupport(fake, AccessWindowSecs).WithStore(store)
	support.pollInterval = 0

	out, err := support.SettleSubscription(ctx, verifiedFromTerms(honestTerms()), 196, true)
	require.NoError(t, err)
	assert.Equal(t, "sub-new", out.SubID)
	assert.Equal(t, StateActive, out.State)

	rec, _ := store.Get(ctx, "sub-new")
	require.NotNil(t, rec, "create must persist a record")
	assert.Equal(t, "pro", rec.PlanID)
}

func TestSettleSubscriptionPollsPendingCreate(t *testing.T) {
	ctx := context.Background()
	fake := &fakeFacilitator{
		createResp: &CreateSubscriptionResponse{SubID: "sub-p", State: StatePending},
		statuses: map[string]*SubscriptionStatus{
			"sub-p": {SubID: "sub-p", State: StateActive, ElapsedPeriods: 1, LastChargedPeriod: 1},
		},
	}
	support := NewSupport(fake, AccessWindowSecs).WithStore(NewInMemoryStore())
	support.pollInterval = 0

	out, err := support.SettleSubscription(ctx, verifiedFromTerms(honestTerms()), 196, true)
	require.NoError(t, err)
	assert.Equal(t, StateActive, out.State, "pending create is polled until the period is charged")
}

func TestSettleSubscriptionDowngradeSkipsPolling(t *testing.T) {
	ctx := context.Background()
	// The successor is left PENDING and uncharged: had the settlement polled, it
	// would refresh PollMaxAttempts times; a downgrade refreshes exactly once.
	fake := &fakeFacilitator{
		changeResp: &ChangeResponse{NewSubID: "sub-dg", State: StatePending},
		statuses: map[string]*SubscriptionStatus{
			"sub-dg": {SubID: "sub-dg", State: StatePending, ElapsedPeriods: 2, LastChargedPeriod: 0},
		},
	}
	support := NewSupport(fake, AccessWindowSecs).WithStore(NewInMemoryStore())
	support.pollInterval = 0

	terms := honestTerms()
	terms.ChangeFromSubID = "0xdead"
	terms.ChangeEffectiveAt = ChangeEffectivePeriodEnd

	out, err := support.SettleSubscription(ctx, verifiedFromTerms(terms), 196, true)
	require.NoError(t, err)
	assert.Equal(t, "sub-dg", out.SubID)
	assert.Equal(t, StatePending, out.State)
	assert.Len(t, fake.getCalls, 1, "a scheduled downgrade is not polled to completion")
}

func TestSettleCancelRelaysAndRefreshes(t *testing.T) {
	ctx := context.Background()
	fake := &fakeFacilitator{
		statuses: map[string]*SubscriptionStatus{"0xsub": {SubID: "0xsub", State: StateCanceled}},
	}
	support := NewSupport(fake, AccessWindowSecs).WithStore(NewInMemoryStore())

	resp, err := support.SettleCancel(ctx, &CancelAuth{SubID: "0xsub"})
	require.NoError(t, err)
	assert.Equal(t, "0xsub", resp.SubID)
	assert.Contains(t, fake.getCalls, "0xsub", "cancel refreshes the affected record")
}

func TestSettleCancelPendingRelaysAndRefreshes(t *testing.T) {
	ctx := context.Background()
	fake := &fakeFacilitator{
		statuses: map[string]*SubscriptionStatus{"0xsub": {SubID: "0xsub", State: StateActive}},
	}
	support := NewSupport(fake, AccessWindowSecs)

	resp, err := support.SettleCancelPending(ctx, &PendingChangeCancelAuth{SubID: "0xsub", NewSubID: "0xnew"})
	require.NoError(t, err)
	assert.Equal(t, "0xsub", resp.SubID)
	assert.Contains(t, fake.getCalls, "0xsub")
}

func TestSupportDueSubscriptions(t *testing.T) {
	ctx := context.Background()
	store := NewInMemoryStore()
	dueAt := uint64(1000)
	laterAt := uint64(5000)
	require.NoError(t, store.Put(ctx, &SubscriptionRecord{SubID: "due", State: StateActive, NextChargeableAt: &dueAt}))
	require.NoError(t, store.Put(ctx, &SubscriptionRecord{SubID: "later", State: StateActive, NextChargeableAt: &laterAt}))
	support := NewSupport(&fakeFacilitator{}, AccessWindowSecs).WithStore(store)

	got, err := support.DueSubscriptions(ctx, 1000)
	require.NoError(t, err)
	require.Len(t, got, 1)
	assert.Equal(t, "due", got[0].SubID)
}

func TestVerifyAccessCacheFastPathSkipsFacilitator(t *testing.T) {
	subID := "0x" + strings.Repeat("a1", 32)
	now := int64(1_700_000_000)
	proof, payer := signedProof(t, subID, uint64(now))

	startAt := uint64(now) - 250 // periodSec 100 → current billing period is 3
	lcp := uint32(3)
	store := NewInMemoryStore()
	require.NoError(t, store.Put(context.Background(), &SubscriptionRecord{
		SubID: subID, State: StateActive, Payer: payer, PlanID: "pro", PlanTier: 2,
		StartAt: startAt, PeriodSec: 100, PeriodMode: PeriodModeFixed,
		LastChargedPeriod: &lcp, UpdatedAt: uint64(now),
	}))
	fake := &fakeFacilitator{}
	support := NewSupport(fake, AccessWindowSecs).WithStore(store)

	result, aerr := support.VerifyAccess(context.Background(), accessHeader(t, proof), nil, now)
	require.Nil(t, aerr)
	require.NotNil(t, result)
	assert.Equal(t, "pro", result.PlanID)
	assert.Empty(t, fake.getCalls, "a fresh, owned, charged cache record must not hit the facilitator")
}

func TestVerifyAccessCacheMissWhenStale(t *testing.T) {
	subID := "0x" + strings.Repeat("b2", 32)
	now := int64(1_700_000_000)
	proof, payer := signedProof(t, subID, uint64(now))

	startAt := uint64(now) - 250
	lcp := uint32(3)
	store := NewInMemoryStore()
	require.NoError(t, store.Put(context.Background(), &SubscriptionRecord{
		SubID: subID, State: StateActive, Payer: payer, PlanID: "pro", PlanTier: 2,
		StartAt: startAt, PeriodSec: 100, PeriodMode: PeriodModeFixed,
		LastChargedPeriod: &lcp, UpdatedAt: uint64(now) - 1000, // older than the 30s TTL
	}))
	fake := &fakeFacilitator{statuses: map[string]*SubscriptionStatus{
		subID: {SubID: subID, State: StateActive, Payer: payer, PlanID: "pro", PlanTier: 2, ElapsedPeriods: 3, LastChargedPeriod: 3, StartAt: startAt, PeriodSec: 100},
	}}
	support := NewSupport(fake, AccessWindowSecs).WithStore(store)

	result, aerr := support.VerifyAccess(context.Background(), accessHeader(t, proof), nil, now)
	require.Nil(t, aerr)
	require.NotNil(t, result)
	assert.NotEmpty(t, fake.getCalls, "a stale cache record must fall through to the facilitator")
}

func TestVerifyAccessRejectsMalformedHeader(t *testing.T) {
	support := NewSupport(&fakeFacilitator{}, AccessWindowSecs)
	_, aerr := support.VerifyAccess(context.Background(), "!!!not base64!!!", nil, 1_700_000_000)
	require.NotNil(t, aerr)
	assert.Equal(t, AccessUnauthorized, aerr.Kind)
}

func TestVerifyAccessRejectsForeignPayer(t *testing.T) {
	subID := "0x" + strings.Repeat("c3", 32)
	now := int64(1_700_000_000)
	proof, _ := signedProof(t, subID, uint64(now))

	fake := &fakeFacilitator{statuses: map[string]*SubscriptionStatus{
		subID: {SubID: subID, State: StateActive, Payer: "0x9999999999999999999999999999999999999999", PlanID: "pro", ElapsedPeriods: 1, LastChargedPeriod: 1, StartAt: 1000, PeriodSec: 100},
	}}
	support := NewSupport(fake, AccessWindowSecs)

	_, aerr := support.VerifyAccess(context.Background(), accessHeader(t, proof), nil, now)
	require.NotNil(t, aerr)
	assert.Equal(t, AccessUnauthorized, aerr.Kind)
}

func TestVerifyTermsMatchOfferAcceptsJSONRoundTrippedOffer(t *testing.T) {
	// An offer echoed back through JSON has its numeric extra values decoded as
	// float64; the numeric getters must still match them against the terms.
	support := NewSupport(&fakeFacilitator{}, AccessWindowSecs)
	raw, err := json.Marshal(planAccept("pro", 2, true))
	require.NoError(t, err)
	var decoded types.PaymentRequirements
	require.NoError(t, json.Unmarshal(raw, &decoded))

	terms := honestTerms()
	assert.NoError(t, support.VerifyTermsMatchOffer(&terms, []types.PaymentRequirements{decoded}, "0xtok"))
}
