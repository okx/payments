package evm

import (
	"encoding/json"
	"fmt"

	"github.com/okx/payments/go/mpp/protocol"
)

// SessionReceipt holds the EVM-specific settlement data attached to a session
// payment receipt.
type SessionReceipt struct {
	ChannelID        string `json:"channelId"`
	CumulativeAmount string `json:"cumulativeAmount"`
	EscrowContract   string `json:"escrowContract"`
	Status           string `json:"status"`
	Reference        string `json:"reference,omitempty"`
}

// NewSessionReceipt creates a SessionReceipt with the given fields.
func NewSessionReceipt(channelID, cumulativeAmount, escrowContract, status, reference string) *SessionReceipt {
	return &SessionReceipt{
		ChannelID:        channelID,
		CumulativeAmount: cumulativeAmount,
		EscrowContract:   escrowContract,
		Status:           status,
		Reference:        reference,
	}
}

// ToBaseReceipt encodes the SessionReceipt as base64url JSON and attaches it
// as the settlement field of a success receipt for the given challenge ID.
func (r *SessionReceipt) ToBaseReceipt(challengeID string) (*protocol.Receipt, error) {
	data, err := json.Marshal(r)
	if err != nil {
		return nil, fmt.Errorf("session receipt: marshal: %w", err)
	}
	encoded := protocol.Base64URLEncode(data)
	return protocol.NewSuccessReceipt(challengeID, protocol.MethodName(MethodNameEVM), protocol.IntentSession, encoded), nil
}
