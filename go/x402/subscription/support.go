package subscription

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/okx/payments/go/x402/types"
)

// AccessErrorKind classifies why access was refused, which the middleware maps
// to a bare 402 (Unauthorized/Denied) or a 402-with-offer (NotActive).
type AccessErrorKind int

const (
	// AccessUnauthorized: proof invalid or subscription unknown — bare 402.
	AccessUnauthorized AccessErrorKind = iota
	// AccessNotActive: current period unpaid — 402 with a subscribe offer.
	AccessNotActive
	// AccessDenied: merchant policy vetoed — hard 402, no offer.
	AccessDenied
)

// AccessError is a classified access failure.
type AccessError struct {
	Kind    AccessErrorKind
	Message string
}

func (e *AccessError) Error() string { return e.Message }

// AccessResult is the outcome of a granted access check.
type AccessResult struct {
	SubID    string
	PlanID   string
	PlanTier uint8
}

// VerifiedSubscription is a locally-verified buyer subscribe/change payload
// ready to relay to the facilitator.
type VerifiedSubscription struct {
	Inner *SubscriptionPayloadInner
}

// SubmitOutcome is the result of a create/change settlement.
type SubmitOutcome struct {
	SubID  string
	TxHash *string
	State  uint8
}

// ChargeRecordOutcome is the result of ChargeAndRecord.
type ChargeRecordOutcome struct {
	State               uint8
	Period              uint32
	TxHash              *string
	PlanChangeTriggered bool
	NewSubID            *string
}

// SubscriptionSupport carries the store, facilitator client, access replay
// window, access cache TTL and optional veto hook that back the period flows.
type SubscriptionSupport struct {
	facilitator      SubscriptionFacilitatorClient
	store            SubscriptionStore
	accessWindowSecs int64
	accessCacheTTL   int64
	onBeforeAccess   OnBeforeAccessHook
	pollInterval     time.Duration
}

// NewSupport creates a SubscriptionSupport with the given facilitator client and
// AccessProof replay window (seconds).
func NewSupport(facilitator SubscriptionFacilitatorClient, accessWindowSecs int64) *SubscriptionSupport {
	return &SubscriptionSupport{
		facilitator:      facilitator,
		accessWindowSecs: accessWindowSecs,
		pollInterval:     PollInterval,
	}
}

// WithStore attaches a store and enables the access cache fast path (defaulting
// the TTL to 30s if not already set).
func (s *SubscriptionSupport) WithStore(store SubscriptionStore) *SubscriptionSupport {
	s.store = store
	if s.accessCacheTTL == 0 {
		s.accessCacheTTL = AccessCacheTTLSecs
	}
	return s
}

// WithAccessCacheTTL overrides the access cache freshness window in seconds.
// Zero disables the cache (always query the facilitator).
func (s *SubscriptionSupport) WithAccessCacheTTL(secs int64) *SubscriptionSupport {
	s.accessCacheTTL = secs
	return s
}

// OnBeforeAccess installs the veto hook run after proof verification.
func (s *SubscriptionSupport) OnBeforeAccess(hook OnBeforeAccessHook) *SubscriptionSupport {
	s.onBeforeAccess = hook
	return s
}

// Store returns the attached store (may be nil).
func (s *SubscriptionSupport) Store() SubscriptionStore { return s.store }

// VerifyTermsMatchOffer binds buyer-signed terms to an advertised plan. The
// facilitator verifies the signature; this ensures the signed economics match
// what the seller offered for the claimed planId.
func (s *SubscriptionSupport) VerifyTermsMatchOffer(terms *SubscriptionTerms, offerAccepts []types.PaymentRequirements, offerAsset string) error {
	var offer *types.PaymentRequirements
	for i := range offerAccepts {
		if offerAccepts[i].Scheme != SchemePeriod || offerAccepts[i].Extra == nil {
			continue
		}
		if id, _ := planFields(offerAccepts[i].Extra); id == terms.PlanID {
			offer = &offerAccepts[i]
			break
		}
	}
	if offer == nil {
		return fmt.Errorf("planId `%s` is not offered on this route", terms.PlanID)
	}
	extra := offer.Extra

	if !strings.EqualFold(offer.PayTo, terms.Merchant) {
		return fmt.Errorf("terms.merchant %s does not match offer payTo %s", terms.Merchant, offer.PayTo)
	}
	if v, _ := extraStr(extra, "amountPerPeriod"); v != terms.AmountPerPeriod {
		return fmt.Errorf("terms.amountPerPeriod %s does not match offer %s", terms.AmountPerPeriod, v)
	}
	if v, ok := extraUint64(extra, "periodSec"); !ok || v != terms.PeriodSec {
		return fmt.Errorf("terms.periodSec %d does not match offer", terms.PeriodSec)
	}
	if v, ok := extraUint64(extra, "maxPeriods"); !ok || v != uint64(terms.MaxPeriods) {
		return fmt.Errorf("terms.maxPeriods %d does not match offer", terms.MaxPeriods)
	}
	offerMode, _ := extraUint64(extra, "periodMode")
	if offerMode != uint64(terms.PeriodMode) {
		return fmt.Errorf("terms.periodMode %d does not match offer", terms.PeriodMode)
	}
	if _, tier := planFields(extra); tier != terms.PlanTier {
		return fmt.Errorf("terms.planTier %d does not match offer", terms.PlanTier)
	}
	if offerAsset != "" && !strings.EqualFold(offerAsset, terms.Token) {
		return fmt.Errorf("terms.token %s does not match offer asset %s", terms.Token, offerAsset)
	}

	// initialCharge is bound on create only; on a change the amount varies
	// (upgrade) or must be zero (downgrade), both contract-enforced.
	if isZeroBytes32(terms.ChangeFromSubID) {
		offerPeriods, offerAmount := offerInitialCharge(extra)
		if uint64(terms.InitialChargePeriods) != offerPeriods {
			return fmt.Errorf("terms.initialChargePeriods %d does not match offer %d", terms.InitialChargePeriods, offerPeriods)
		}
		if terms.InitialChargeAmount != offerAmount {
			return fmt.Errorf("terms.initialChargeAmount %s does not match offer %s", terms.InitialChargeAmount, offerAmount)
		}
	}

	// startAt pin-check: when the offer pins startAt, the signed terms must
	// match it. Offers pin startAt=0, which enforces immediate start.
	if pinned, ok := extraUint64(extra, "startAt"); ok {
		if terms.StartAt != pinned {
			return fmt.Errorf("terms.startAt %d does not match pinned offer startAt %d", terms.StartAt, pinned)
		}
	}

	return nil
}

// offerInitialCharge reads the offer's initialCharge periodCount and totalAmount,
// defaulting to (0, "0") when the block is absent.
func offerInitialCharge(extra map[string]interface{}) (uint64, string) {
	ic, ok := extra["initialCharge"].(map[string]interface{})
	if !ok {
		return 0, "0"
	}
	periods, _ := asUint64(ic["periodCount"])
	amount := "0"
	if s, ok := ic["totalAmount"].(string); ok {
		amount = s
	}
	return periods, amount
}

// VerifySubscription unpacks a buyer PAYMENT-SIGNATURE and binds its terms to the
// route's offers. It performs no network I/O.
func (s *SubscriptionSupport) VerifySubscription(payload *types.PaymentPayload, offerAccepts []types.PaymentRequirements, offerAsset string) (*VerifiedSubscription, error) {
	inner, err := UnpackSubscriptionPayload(payload)
	if err != nil {
		return nil, err
	}
	if err := s.VerifyTermsMatchOffer(&inner.Terms, offerAccepts, offerAsset); err != nil {
		return nil, err
	}
	return &VerifiedSubscription{Inner: inner}, nil
}

// SettleSubscription relays a verified subscribe/change to the facilitator,
// creating when changeFromSubId is zero and changing otherwise, writing the
// resulting record and polling a pending (non-downgrade) settlement.
func (s *SubscriptionSupport) SettleSubscription(ctx context.Context, verified *VerifiedSubscription, chainIndex uint64, syncSettle bool) (*SubmitOutcome, error) {
	inner := verified.Inner
	terms := inner.Terms
	isChange := !isZeroBytes32(terms.ChangeFromSubID)
	isDowngrade := isChange && terms.ChangeEffectiveAt == ChangeEffectivePeriodEnd

	var outcome *SubmitOutcome
	if !isChange {
		resp, err := s.facilitator.CreateSubscription(ctx, ToCreateRequest(chainIndex, inner, syncSettle))
		if err != nil {
			return nil, err
		}
		outcome = &SubmitOutcome{SubID: resp.SubID, TxHash: resp.TxHash, State: resp.State}
	} else {
		resp, err := s.facilitator.ChangeSubscription(ctx, ToChangeRequest(chainIndex, terms.ChangeFromSubID, inner, syncSettle))
		if err != nil {
			return nil, err
		}
		outcome = &SubmitOutcome{SubID: resp.NewSubID, TxHash: resp.TxHash, State: resp.State}
	}

	if s.store != nil {
		rec := &SubscriptionRecord{
			SubID:      outcome.SubID,
			State:      outcome.State,
			Payer:      terms.Payer,
			PlanID:     terms.PlanID,
			PlanTier:   terms.PlanTier,
			StartAt:    terms.StartAt,
			PeriodSec:  terms.PeriodSec,
			PeriodMode: terms.PeriodMode,
			MaxPeriods: terms.MaxPeriods,
			UpdatedAt:  uint64(nowUnix()),
		}
		_ = s.store.Put(ctx, rec)
	}

	if outcome.State == StatePending && !isDowngrade {
		if status := s.pollUntilSettled(ctx, outcome.SubID); status != nil {
			outcome.State = status.State
		}
	} else {
		s.refreshSubscription(ctx, outcome.SubID)
	}

	return outcome, nil
}

// ChargeAndRecord is the reconciling charge primitive the scheduler calls. When
// a charge triggers a plan change it refreshes the successor subId so future
// charges target it; a normal charge advances the record's state and next-charge
// time; a pending charge is polled.
func (s *SubscriptionSupport) ChargeAndRecord(ctx context.Context, subID string, syncSettle bool) (*ChargeRecordOutcome, error) {
	resp, err := s.facilitator.Charge(ctx, &ChargeRequest{SubID: subID, SyncSettle: syncSettle})
	if err != nil {
		return nil, err
	}

	if resp.PlanChangeTriggered && resp.NewSubID != nil {
		s.refreshSubscription(ctx, *resp.NewSubID)
	}

	state := resp.State
	if resp.State == StatePending {
		if status := s.pollUntilSettled(ctx, subID); status != nil {
			state = status.State
		}
	} else {
		s.refreshSubscription(ctx, subID)
	}

	return &ChargeRecordOutcome{
		State:               state,
		Period:              resp.Period,
		TxHash:              resp.TxHash,
		PlanChangeTriggered: resp.PlanChangeTriggered,
		NewSubID:            resp.NewSubID,
	}, nil
}

// pollUntilSettled refreshes a subscription up to PollMaxAttempts times, one
// PollInterval apart, stopping as soon as the current period is charged or the
// subscription has FAILED. It never sleeps after the final attempt.
func (s *SubscriptionSupport) pollUntilSettled(ctx context.Context, subID string) *SubscriptionStatus {
	var last *SubscriptionStatus
	for attempt := 0; attempt < PollMaxAttempts; attempt++ {
		if status := s.refreshSubscription(ctx, subID); status != nil {
			last = status
			if CurrentPeriodCharged(status, nowUnix()) || status.State == StateFailed {
				return status
			}
		}
		if attempt+1 < PollMaxAttempts {
			time.Sleep(s.pollInterval)
		}
	}
	return last
}

// refreshSubscription pulls the facilitator detail and writes it through to the
// store. Best-effort: returns nil on lookup failure.
func (s *SubscriptionSupport) refreshSubscription(ctx context.Context, subID string) *SubscriptionStatus {
	status, err := s.facilitator.GetSubscription(ctx, subID)
	if err != nil || status == nil {
		return nil
	}
	s.recordStatus(ctx, subID, status, "", nowUnix())
	return status
}

// recordStatus writes a facilitator detail into the store, preserving prior
// non-zero startAt/billingAnchorAt/planId/planTier when the fresh detail omits
// them (clobbering a known startAt back to 0 would defeat the access cache).
func (s *SubscriptionSupport) recordStatus(ctx context.Context, subID string, status *SubscriptionStatus, fallbackPayer string, now int64) (string, uint8) {
	planID := status.PlanID
	planTier := status.PlanTier
	startAt := status.StartAt
	anchor := status.BillingAnchorAt

	if s.store == nil {
		return planID, planTier
	}

	if prior, _ := s.store.Get(ctx, subID); prior != nil {
		if planID == "" {
			planID = prior.PlanID
		}
		if planTier == 0 {
			planTier = prior.PlanTier
		}
		if startAt == 0 {
			startAt = prior.StartAt
		}
		if anchor == 0 {
			anchor = prior.BillingAnchorAt
		}
	}

	payer := status.Payer
	if payer == "" {
		payer = fallbackPayer
	}

	lcp := status.LastChargedPeriod
	_ = s.store.Put(ctx, &SubscriptionRecord{
		SubID:             subID,
		State:             status.State,
		Payer:             payer,
		PlanID:            planID,
		PlanTier:          planTier,
		NextChargeableAt:  status.NextChargeableAt,
		ChangedToSubID:    status.ChangedToSubID,
		StartAt:           startAt,
		PeriodSec:         status.PeriodSec,
		PeriodMode:        status.PeriodMode,
		BillingAnchorAt:   anchor,
		MaxPeriods:        status.MaxPeriods,
		LastChargedPeriod: &lcp,
		UpdatedAt:         uint64(now),
	})

	return planID, planTier
}

// VerifyAccess verifies an APP-Access proof, runs the veto hook, tries the store
// cache fast path, and otherwise pulls facilitator detail before applying the
// period gate. On success it returns the subscriber's subId and plan.
func (s *SubscriptionSupport) VerifyAccess(ctx context.Context, header string, accepts []types.PaymentRequirements, now int64) (*AccessResult, *AccessError) {
	proof, err := DecodeAccessProof(header)
	if err != nil {
		return nil, &AccessError{Kind: AccessUnauthorized, Message: err.Error()}
	}
	verified, err := VerifyAccessProof(proof, now, s.accessWindowSecs)
	if err != nil {
		return nil, &AccessError{Kind: AccessUnauthorized, Message: err.Error()}
	}

	if s.onBeforeAccess != nil {
		res := s.onBeforeAccess(ctx, AccessContext{
			SubID:   verified.SubID,
			Payer:   verified.Payer,
			Accepts: AcceptedPlans(accepts),
		})
		if res.Abort {
			reason := res.Reason
			if reason == "" {
				reason = fmt.Sprintf("access denied by merchant policy for subscription %s", verified.SubID)
			}
			return nil, &AccessError{Kind: AccessDenied, Message: reason}
		}
	}

	if r := s.accessCacheHit(ctx, verified, now); r != nil {
		return r, nil
	}

	status, ferr := s.facilitator.GetSubscription(ctx, verified.SubID)
	if ferr != nil || status == nil {
		return nil, &AccessError{Kind: AccessUnauthorized, Message: fmt.Sprintf("subscription lookup failed for %s", verified.SubID)}
	}
	if status.Payer != "" && !strings.EqualFold(status.Payer, verified.Payer) {
		return nil, &AccessError{Kind: AccessUnauthorized, Message: fmt.Sprintf("AccessProof payer %s does not own subscription %s", verified.Payer, verified.SubID)}
	}

	planID, planTier := s.recordStatus(ctx, verified.SubID, status, verified.Payer, now)

	if !CurrentPeriodCharged(status, now) {
		return nil, &AccessError{Kind: AccessNotActive, Message: fmt.Sprintf("subscription %s current period not charged (lastCharged=%d elapsed=%d)", verified.SubID, status.LastChargedPeriod, status.ElapsedPeriods)}
	}

	return &AccessResult{SubID: verified.SubID, PlanID: planID, PlanTier: planTier}, nil
}

// accessCacheHit returns a granted AccessResult from a fresh, owned, fully-known
// store record whose last charged period covers the current period, else nil.
func (s *SubscriptionSupport) accessCacheHit(ctx context.Context, verified *VerifiedAccess, now int64) *AccessResult {
	if s.store == nil || s.accessCacheTTL <= 0 {
		return nil
	}
	rec, _ := s.store.Get(ctx, verified.SubID)
	if rec == nil || rec.LastChargedPeriod == nil {
		return nil
	}
	fresh := now-int64(rec.UpdatedAt) <= s.accessCacheTTL
	ownerOK := rec.Payer != "" && strings.EqualFold(rec.Payer, verified.Payer)
	anchorKnown := rec.PeriodMode != PeriodModeCalendarMonth || rec.BillingAnchorAt > 0
	if !fresh || !ownerOK || rec.PlanID == "" || rec.StartAt == 0 || !anchorKnown {
		return nil
	}
	current := ElapsedPeriods(rec.PeriodMode, int64(rec.StartAt), int64(rec.BillingAnchorAt), int64(rec.PeriodSec), now)
	if current > 0 && int64(*rec.LastChargedPeriod) >= current {
		return &AccessResult{SubID: verified.SubID, PlanID: rec.PlanID, PlanTier: rec.PlanTier}
	}
	return nil
}

// VerifyCancel validates a cancel authorization's well-formedness before relay.
func (s *SubscriptionSupport) VerifyCancel(auth *CancelAuth) error {
	if strings.TrimSpace(auth.SubID) == "" {
		return fmt.Errorf("cancel auth missing subId")
	}
	return nil
}

// SettleCancel relays a signed cancel and refreshes the affected record.
func (s *SubscriptionSupport) SettleCancel(ctx context.Context, auth *CancelAuth) (*TxResultResponse, error) {
	resp, err := s.facilitator.CancelSubscription(ctx, &CancelSubscriptionRequest{
		SubID:      auth.SubID,
		CancelAuth: *auth,
		SyncSettle: true,
	})
	if err != nil {
		return nil, err
	}
	s.refreshSubscription(ctx, resp.SubID)
	return resp, nil
}

// VerifyCancelPending validates a pending-change cancel authorization.
func (s *SubscriptionSupport) VerifyCancelPending(auth *PendingChangeCancelAuth) error {
	if strings.TrimSpace(auth.SubID) == "" {
		return fmt.Errorf("cancel-pending auth missing subId")
	}
	if strings.TrimSpace(auth.NewSubID) == "" {
		return fmt.Errorf("cancel-pending auth missing newSubId")
	}
	return nil
}

// SettleCancelPending relays a signed pending-change cancel and refreshes.
func (s *SubscriptionSupport) SettleCancelPending(ctx context.Context, auth *PendingChangeCancelAuth) (*TxResultResponse, error) {
	resp, err := s.facilitator.CancelPendingChange(ctx, &CancelPendingChangeRequest{
		SubID:      auth.SubID,
		CancelAuth: *auth,
		SyncSettle: true,
	})
	if err != nil {
		return nil, err
	}
	s.refreshSubscription(ctx, resp.SubID)
	return resp, nil
}

// DueSubscriptions returns the store records due for a charge at now.
func (s *SubscriptionSupport) DueSubscriptions(ctx context.Context, now uint64) ([]*SubscriptionRecord, error) {
	return DueSubscriptions(ctx, s.store, now)
}

// EncodeResponseHeader builds the base64(JSON{subId,txHash,state}) value for the
// PAYMENT-RESPONSE header after a successful create/change.
func EncodeResponseHeader(outcome *SubmitOutcome) string {
	body := struct {
		SubID  string  `json:"subId"`
		TxHash *string `json:"txHash"`
		State  uint8   `json:"state"`
	}{SubID: outcome.SubID, TxHash: outcome.TxHash, State: outcome.State}
	data, err := json.Marshal(body)
	if err != nil {
		return ""
	}
	return base64.StdEncoding.EncodeToString(data)
}

// isZeroBytes32 reports whether a bytes32 hex string is empty or all zero, the
// marker for "create" (no change source).
func isZeroBytes32(s string) bool {
	t := strings.TrimPrefix(strings.TrimSpace(s), "0x")
	if t == "" {
		return true
	}
	for _, c := range t {
		if c != '0' {
			return false
		}
	}
	return true
}

func nowUnix() int64 {
	return time.Now().Unix()
}
