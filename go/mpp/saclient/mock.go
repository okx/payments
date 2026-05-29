package saclient

import (
	"context"
	"fmt"
	"time"
)

// MockSAClient is an in-memory SA client for testing. It accepts all
// credentials without on-chain verification and returns synthetic receipts.
// Useful for local development and integration tests where real SA API
// access is not available.
type MockSAClient struct {
	// ChainID included in all receipts.
	ChainID uint64

	// ChargeCount tracks the number of charge settle calls (for generating unique references).
	chargeCount int

	// Sessions tracks open channels by channelId.
	sessions map[string]*mockSession
}

type mockSession struct {
	deposit          string
	cumulativeAmount string
	status           string // OPEN, CLOSING, CLOSED
	payer            string
	payee            string
	token            string
}

// NewMockSAClient creates a mock SA client for the given chain.
func NewMockSAClient(chainID uint64) *MockSAClient {
	return &MockSAClient{
		ChainID:  chainID,
		sessions: make(map[string]*mockSession),
	}
}

func (m *MockSAClient) now() string {
	return time.Now().UTC().Format(time.RFC3339)
}

func (m *MockSAClient) txRef() string {
	m.chargeCount++
	return fmt.Sprintf("0x%064x", m.chargeCount)
}

// Settle accepts a charge credential and returns a synthetic receipt.
func (m *MockSAClient) Settle(_ context.Context, req *ChargeSettleRequest) (*ChargeReceipt, error) {
	challengeID := ""
	if req.Challenge != nil {
		challengeID = req.Challenge.ID
	}
	return &ChargeReceipt{
		Method:      "evm",
		Reference:   m.txRef(),
		Status:      "success",
		Timestamp:   m.now(),
		ChainID:     m.ChainID,
		ChallengeID: challengeID,
	}, nil
}

// VerifyHash accepts a hash credential and returns a synthetic receipt.
func (m *MockSAClient) VerifyHash(_ context.Context, req *ChargeVerifyHashRequest) (*ChargeReceipt, error) {
	challengeID := ""
	if req.Challenge != nil {
		challengeID = req.Challenge.ID
	}
	return &ChargeReceipt{
		Method:      "evm",
		Reference:   req.Payload.Hash,
		Status:      "success",
		Timestamp:   m.now(),
		ChainID:     m.ChainID,
		ChallengeID: challengeID,
	}, nil
}

// SessionOpen records a new channel and returns a receipt.
func (m *MockSAClient) SessionOpen(_ context.Context, req *SessionOpenRequest) (*SessionReceipt, error) {
	deposit := "0"
	if req.Payload.Authorization != nil {
		deposit = req.Payload.Authorization.Value
	}

	payer := ""
	if req.Payload.Authorization != nil {
		payer = req.Payload.Authorization.From
	}

	payee := ""
	if req.Payload.Authorization != nil {
		payee = req.Payload.Authorization.To
	}

	m.sessions[req.Payload.ChannelID] = &mockSession{
		deposit: deposit,
		status:  "OPEN",
		payer:   payer,
		payee:   payee,
	}

	return &SessionReceipt{
		Method:    "evm",
		Intent:    "session",
		Status:    "success",
		Timestamp: m.now(),
		ChannelID: req.Payload.ChannelID,
		ChainID:   m.ChainID,
		Reference: m.txRef(),
		Deposit:   deposit,
	}, nil
}

// SessionTopUp adds deposit to an existing channel.
func (m *MockSAClient) SessionTopUp(_ context.Context, req *SessionTopUpRequest) (*SessionReceipt, error) {
	sess, ok := m.sessions[req.Payload.ChannelID]
	if !ok {
		return nil, fmt.Errorf("channel %s not found", req.Payload.ChannelID)
	}
	sess.deposit = req.Payload.AdditionalDeposit // simplified — should add
	return &SessionReceipt{
		Method:    "evm",
		Intent:    "session",
		Status:    "success",
		Timestamp: m.now(),
		ChannelID: req.Payload.ChannelID,
		ChainID:   m.ChainID,
		Reference: m.txRef(),
		Deposit:   sess.deposit,
	}, nil
}

// SessionSettle returns a synthetic settle receipt.
func (m *MockSAClient) SessionSettle(_ context.Context, req *SessionSettleRequest) (*SessionReceipt, error) {
	sess, ok := m.sessions[req.Payload.ChannelID]
	if !ok {
		return nil, fmt.Errorf("channel %s not found", req.Payload.ChannelID)
	}
	sess.cumulativeAmount = req.Payload.CumulativeAmount
	return &SessionReceipt{
		Method:    "evm",
		Intent:    "session",
		Status:    "success",
		Timestamp: m.now(),
		ChannelID: req.Payload.ChannelID,
		ChainID:   m.ChainID,
		Reference: m.txRef(),
		Deposit:   sess.deposit,
	}, nil
}

// SessionClose marks the channel as closed and returns a receipt.
func (m *MockSAClient) SessionClose(_ context.Context, req *SessionCloseRequest) (*SessionReceipt, error) {
	sess, ok := m.sessions[req.Payload.ChannelID]
	if !ok {
		return nil, fmt.Errorf("channel %s not found", req.Payload.ChannelID)
	}
	sess.status = "CLOSED"
	sess.cumulativeAmount = req.Payload.CumulativeAmount
	return &SessionReceipt{
		Method:    "evm",
		Intent:    "session",
		Status:    "success",
		Timestamp: m.now(),
		ChannelID: req.Payload.ChannelID,
		ChainID:   m.ChainID,
		Reference: m.txRef(),
		Deposit:   sess.deposit,
	}, nil
}

// SessionStatus returns the current channel state.
func (m *MockSAClient) SessionStatus(_ context.Context, channelID string) (*SessionStatus, error) {
	sess, ok := m.sessions[channelID]
	if !ok {
		return nil, fmt.Errorf("channel %s not found", channelID)
	}
	return &SessionStatus{
		ChannelID:        channelID,
		Payer:            sess.payer,
		Payee:            sess.payee,
		Token:            sess.token,
		Deposit:          sess.deposit,
		CumulativeAmount: sess.cumulativeAmount,
		SettledOnChain:   "0",
		RemainingBalance: sess.deposit,
		SessionStatus:    sess.status,
	}, nil
}
