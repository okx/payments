package subscription

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/okx/payments/go/x402/types"
)

// UnpackSubscriptionPayload extracts the buyer's inner subscription payload from
// a PAYMENT-SIGNATURE. It re-serializes the generic payload map and decodes it
// into the typed inner shape, erroring if required fields are missing.
func UnpackSubscriptionPayload(payload *types.PaymentPayload) (*SubscriptionPayloadInner, error) {
	if payload == nil {
		return nil, fmt.Errorf("subscription payload is nil")
	}
	raw, err := json.Marshal(payload.Payload)
	if err != nil {
		return nil, fmt.Errorf("failed to encode subscription payload: %w", err)
	}
	var inner SubscriptionPayloadInner
	if err := json.Unmarshal(raw, &inner); err != nil {
		return nil, fmt.Errorf("failed to decode subscription payload: %w", err)
	}
	return &inner, nil
}

// DecodeAccessProof decodes the base64(JSON) APP-Access header into an
// AccessProof. Signature recovery happens separately in the access gate.
func DecodeAccessProof(header string) (*AccessProof, error) {
	decoded, err := base64.StdEncoding.DecodeString(strings.TrimSpace(header))
	if err != nil {
		return nil, fmt.Errorf("APP-Access base64 decode failed: %w", err)
	}
	var proof AccessProof
	if err := json.Unmarshal(decoded, &proof); err != nil {
		return nil, fmt.Errorf("APP-Access JSON decode failed: %w", err)
	}
	return &proof, nil
}

// ToCreateRequest maps a buyer's inner payload to the flat create body, renaming
// permitSingle→permit, permitSingleSignature→permitSig, termsSignature→termsSig.
func ToCreateRequest(chainIndex uint64, inner *SubscriptionPayloadInner, syncSettle bool) *CreateSubscriptionRequest {
	return &CreateSubscriptionRequest{
		ChainIndex: chainIndex,
		Terms:      inner.Terms,
		Permit:     inner.PermitSingle,
		TermsSig:   inner.TermsSignature,
		PermitSig:  inner.PermitSingleSignature,
		SyncSettle: syncSettle,
	}
}

// ToChangeRequest maps a buyer's inner payload to the flat change body. OldSubID
// is informational; the facilitator keys off newTerms.changeFromSubId.
func ToChangeRequest(chainIndex uint64, oldSubID string, inner *SubscriptionPayloadInner, syncSettle bool) *ChangeSubscriptionRequest {
	return &ChangeSubscriptionRequest{
		ChainIndex: chainIndex,
		OldSubID:   oldSubID,
		NewTerms:   inner.Terms,
		Permit:     inner.PermitSingle,
		TermsSig:   inner.TermsSignature,
		PermitSig:  inner.PermitSingleSignature,
		SyncSettle: syncSettle,
	}
}

// ChainIndexFromNetwork derives the numeric chain index from an eip155 CAIP-2
// network id, e.g. "eip155:196" → 196.
func ChainIndexFromNetwork(network string) (uint64, bool) {
	rest, ok := strings.CutPrefix(network, "eip155:")
	if !ok {
		return 0, false
	}
	n, err := strconv.ParseUint(rest, 10, 64)
	if err != nil {
		return 0, false
	}
	return n, true
}
