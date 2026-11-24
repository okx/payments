package subscription

// Wire types for the period scheme. Atomic token amounts are decimal strings
// (uint160/uint256); addresses, bytes32 and signatures are 0x-hex strings;
// smaller counters are native integers. Field order in the signed structs is
// fixed by the EIP-712 typestring — reordering breaks signature recovery.

// SubscriptionTerms is the buyer-signed subscription authorization. The first
// 17 fields are covered by the EIP-712 signature in the exact order below;
// planId rides on the wire but is not part of the typehash. It is relayed
// verbatim — the SDK never recomputes the digest.
type SubscriptionTerms struct {
	Payer                string `json:"payer"`
	Merchant             string `json:"merchant"`
	Facilitator          string `json:"facilitator"`
	Token                string `json:"token"`
	AmountPerPeriod      string `json:"amountPerPeriod"`
	PeriodSec            uint64 `json:"periodSec"`
	MaxPeriods           uint32 `json:"maxPeriods"`
	StartAt              uint64 `json:"startAt"`
	InitialChargePeriods uint32 `json:"initialChargePeriods"`
	InitialChargeAmount  string `json:"initialChargeAmount"`
	TermsDeadline        uint64 `json:"termsDeadline"`
	PermitHash           string `json:"permitHash"`
	Salt                 string `json:"salt"`
	// PlanID is unsigned and optional on decode (older payloads omit it) but is
	// always emitted so a create body carries the full 18-key object.
	PlanID            string `json:"planId"`
	PlanTier          uint8  `json:"planTier"`
	ChangeFromSubID   string `json:"changeFromSubId"`
	ChangeEffectiveAt uint8  `json:"changeEffectiveAt"`
	// PeriodMode defaults to fixed on decode; always emitted.
	PeriodMode uint8 `json:"periodMode"`
}

// PermitDetails is the Permit2 AllowanceTransfer detail block.
type PermitDetails struct {
	Token      string `json:"token"`
	Amount     string `json:"amount"`
	Expiration uint64 `json:"expiration"`
	Nonce      uint64 `json:"nonce"`
}

// PermitSingle is the Permit2 single-token allowance authorization. SigDeadline
// is a decimal string (uint256).
type PermitSingle struct {
	Details     PermitDetails `json:"details"`
	Spender     string        `json:"spender"`
	SigDeadline string        `json:"sigDeadline"`
}

// CancelAuth is the buyer- or merchant-signed cancel authorization.
type CancelAuth struct {
	Action    uint8  `json:"action"`
	SubID     string `json:"subId"`
	Initiator uint8  `json:"initiator"`
	Nonce     string `json:"nonce"`
	Deadline  uint64 `json:"deadline"`
	Signature string `json:"signature"`
}

// PendingChangeCancelAuth cancels a not-yet-effective downgrade. NewSubID must
// equal the current pending successor and is part of the signed digest.
type PendingChangeCancelAuth struct {
	SubID     string `json:"subId"`
	NewSubID  string `json:"newSubId"`
	Nonce     string `json:"nonce"`
	Deadline  uint64 `json:"deadline"`
	Signature string `json:"signature"`
}

// AccessProof is the buyer-signed (subId, payer, timestamp) proof carried in the
// APP-Access header.
type AccessProof struct {
	Kind      string `json:"kind"`
	SubID     string `json:"subId"`
	Payer     string `json:"payer"`
	Timestamp uint64 `json:"timestamp"`
	Signature string `json:"signature"`
}

// SubscriptionPayloadInner is the inner shape of a buyer PAYMENT-SIGNATURE
// before the codec renames keys for the facilitator.
type SubscriptionPayloadInner struct {
	PermitSingle          PermitSingle      `json:"permitSingle"`
	PermitSingleSignature string            `json:"permitSingleSignature"`
	Terms                 SubscriptionTerms `json:"terms"`
	TermsSignature        string            `json:"termsSignature"`
}

// ---------------------------------------------------------------------------
// Facilitator request bodies (seller → facilitator). Writes carry subId in the
// body; the codec renames the buyer's inner keys.
// ---------------------------------------------------------------------------

// CreateSubscriptionRequest is the flat body for POST /subscriptions.
type CreateSubscriptionRequest struct {
	ChainIndex uint64            `json:"chainIndex"`
	Terms      SubscriptionTerms `json:"terms"`
	Permit     PermitSingle      `json:"permit"`
	TermsSig   string            `json:"termsSig"`
	PermitSig  string            `json:"permitSig"`
	SyncSettle bool              `json:"syncSettle"`
}

// ChangeSubscriptionRequest is the flat body for POST /subscriptions/change.
// OldSubID is informational; the facilitator uses newTerms.changeFromSubId.
type ChangeSubscriptionRequest struct {
	ChainIndex uint64            `json:"chainIndex"`
	OldSubID   string            `json:"oldSubId"`
	NewTerms   SubscriptionTerms `json:"newTerms"`
	Permit     PermitSingle      `json:"permit"`
	TermsSig   string            `json:"termsSig"`
	PermitSig  string            `json:"permitSig"`
	SyncSettle bool              `json:"syncSettle"`
}

// ChargeRequest is the body for POST /subscriptions/charge.
type ChargeRequest struct {
	SubID      string `json:"subId"`
	SyncSettle bool   `json:"syncSettle"`
}

// CancelSubscriptionRequest is the body for POST /subscriptions/cancel.
type CancelSubscriptionRequest struct {
	SubID      string     `json:"subId"`
	CancelAuth CancelAuth `json:"cancelAuth"`
	SyncSettle bool       `json:"syncSettle"`
}

// CancelPendingChangeRequest is the body for POST /subscriptions/cancel-pending-change.
type CancelPendingChangeRequest struct {
	SubID      string                  `json:"subId"`
	CancelAuth PendingChangeCancelAuth `json:"cancelAuth"`
	SyncSettle bool                    `json:"syncSettle"`
}

// FinalizeExpiredRequest is the body for POST /subscriptions/finalize-expired.
type FinalizeExpiredRequest struct {
	SubID string `json:"subId"`
}

// ---------------------------------------------------------------------------
// Facilitator response bodies.
// ---------------------------------------------------------------------------

// CreateSubscriptionResponse is returned by POST /subscriptions.
type CreateSubscriptionResponse struct {
	SubID  string  `json:"subId"`
	TxHash *string `json:"txHash,omitempty"`
	State  uint8   `json:"state"`
}

// ChargeResponse is returned by POST /subscriptions/charge. PlanChangeTriggered
// with NewSubID signals a downgrade activated and future charges must target
// the successor.
type ChargeResponse struct {
	SubID               string  `json:"subId"`
	Period              uint32  `json:"period"`
	TxHash              *string `json:"txHash,omitempty"`
	State               uint8   `json:"state"`
	PlanChangeTriggered bool    `json:"planChangeTriggered"`
	NewSubID            *string `json:"newSubId,omitempty"`
}

// ChangeResponse is returned by POST /subscriptions/change.
type ChangeResponse struct {
	NewSubID string  `json:"newSubId"`
	TxHash   *string `json:"txHash,omitempty"`
	State    uint8   `json:"state"`
}

// TxResultResponse is returned by cancel / cancel-pending-change /
// finalize-expired. State is null when there is no on-chain transition.
type TxResultResponse struct {
	SubID  string  `json:"subId"`
	TxHash *string `json:"txHash,omitempty"`
	State  *uint8  `json:"state"`
}

// PendingPlanChange describes a scheduled (not-yet-effective) downgrade.
type PendingPlanChange struct {
	SubID               string `json:"subId"`
	NewSubID            string `json:"newSubId"`
	EffectiveFromPeriod uint32 `json:"effectiveFromPeriod"`
	State               uint8  `json:"state"`
}

// SubscriptionCharge is one facilitator ledger entry.
type SubscriptionCharge struct {
	SubID               string  `json:"subId"`
	Period              uint32  `json:"period"`
	ChargeType          uint8   `json:"chargeType"`
	Amount              string  `json:"amount"`
	State               uint8   `json:"state"`
	TxHash              *string `json:"txHash,omitempty"`
	PlanChangeTriggered bool    `json:"planChangeTriggered"`
	NewSubID            *string `json:"newSubId,omitempty"`
}

// ChargesResponse is returned by GET /subscriptions/charges.
type ChargesResponse struct {
	Charges []SubscriptionCharge `json:"charges"`
}

// SubscriptionStatus is the detail response. Every field past state is optional
// on decode so partial backends parse; extra fields default to zero.
type SubscriptionStatus struct {
	SubID             string             `json:"subId"`
	State             uint8              `json:"state"`
	Payer             string             `json:"payer,omitempty"`
	Merchant          string             `json:"merchant,omitempty"`
	Token             string             `json:"token,omitempty"`
	AmountPerPeriod   string             `json:"amountPerPeriod,omitempty"`
	PeriodSec         uint64             `json:"periodSec,omitempty"`
	MaxPeriods        uint32             `json:"maxPeriods,omitempty"`
	StartAt           uint64             `json:"startAt,omitempty"`
	PeriodMode        uint8              `json:"periodMode,omitempty"`
	BillingAnchorAt   uint64             `json:"billingAnchorAt,omitempty"`
	LastChargedPeriod uint32             `json:"lastChargedPeriod,omitempty"`
	TotalPulled       string             `json:"totalPulled,omitempty"`
	PlanID            string             `json:"planId,omitempty"`
	PlanTier          uint8              `json:"planTier,omitempty"`
	CurrentPeriod     uint32             `json:"currentPeriod,omitempty"`
	ElapsedPeriods    uint64             `json:"elapsedPeriods,omitempty"`
	NextChargeableAt  *uint64            `json:"nextChargeableAt,omitempty"`
	ChangedToSubID    *string            `json:"changedToSubId,omitempty"`
	IsActive          bool               `json:"isActive,omitempty"`
	ServiceEnded      bool               `json:"serviceEnded,omitempty"`
	PendingPlanChange *PendingPlanChange `json:"pendingPlanChange,omitempty"`
}
