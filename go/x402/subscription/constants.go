// Package subscription implements the seller-side x402 "period" scheme: a
// recurring-payment capability layered on the OKX subscription facilitator.
//
// The seller SDK never runs the facilitator. It builds 402 requirements for the
// period scheme, relays buyer-signed subscribe/change/cancel payloads to the
// facilitator's dedicated endpoints, and gates protected routes by billing
// period rather than by a single verify→settle exchange.
package subscription

import "time"

// SchemePeriod is the wire scheme string advertised in payment requirements and
// carried on the buyer's PAYMENT-SIGNATURE. The internal package is named
// "subscription"; the protocol name is "period".
const SchemePeriod = "period"

// AccessHeader carries the buyer's base64(JSON) AccessProof on every protected
// request.
const AccessHeader = "APP-Access"

// AccessProofKind is the only accepted proof kind.
const AccessProofKind = "subscription-id"

// Subscription lifecycle states. PENDING and FAILED are SDK-local; the middle
// four mirror the on-chain lifecycle.
const (
	StatePending   uint8 = 0
	StateActive    uint8 = 1
	StateCompleted uint8 = 2
	StateCanceled  uint8 = 3
	StateChanged   uint8 = 4
	StateFailed    uint8 = 99
)

// Change effective-at markers used on a change's signed terms.
const (
	ChangeEffectiveNone      uint8 = 0
	ChangeEffectiveImmediate uint8 = 1 // upgrade
	ChangeEffectivePeriodEnd uint8 = 2 // downgrade
)

// Period modes. Fixed uses periodSec; calendar-month uses whole months from an
// anchor and requires periodSec == 0.
const (
	PeriodModeFixed         uint8 = 0
	PeriodModeCalendarMonth uint8 = 1
)

// Cancel initiators.
const (
	CancelInitiatorPayer    uint8 = 0
	CancelInitiatorMerchant uint8 = 1
)

// Charge types recorded by the facilitator ledger.
const (
	ChargeTypeInitial        uint8 = 1
	ChargeTypePeriodic       uint8 = 2
	ChargeTypeDowngradeFirst uint8 = 3
	ChargeTypeFinalizeMarker uint8 = 4
)

// Pending-settlement polling bound: at most 5 attempts, one second apart, with
// no sleep after the final attempt.
const (
	PollInterval    = time.Second
	PollMaxAttempts = 5
)

// AccessWindowSecs is the default ±replay window for an AccessProof timestamp.
const AccessWindowSecs int64 = 300

// AccessCacheTTLSecs is the default freshness window for the store-backed
// access fast path. A record older than this falls through to a facilitator
// /detail lookup.
const AccessCacheTTLSecs int64 = 30

// accessProofTimestampBytes is the width the proof timestamp is packed at when
// hashing (a uint256, big-endian). It must match the buyer's encoder.
const accessProofTimestampBytes = 32
