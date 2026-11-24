package evm

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math/big"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/okx/payments/go/mpp/protocol"
	"github.com/okx/payments/go/mpp/saclient"
	"github.com/okx/payments/go/mpp/store"
)

// ─────────────────────────────────────────────────────────────────────────────
// Test constants & helpers
// ─────────────────────────────────────────────────────────────────────────────

const (
	testChainID      uint64 = 1
	testEscrowHex           = "0x1234567890123456789012345678901234567890"
	testPayerHex            = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
	testChannelIDHex        = "0x0000000000000000000000000000000000000000000000000000000000000001"
)

// newTestEVMSessionMethod returns an EVMSessionMethod wired with an in-memory store and mock SA client.
func newTestEVMSessionMethod(escrow string, chainID uint64) (*EVMSessionMethod, store.Store[store.ChannelState]) {
	cs := store.NewMemoryStore[store.ChannelState]()
	m, _ := NewEVMSessionMethod(EVMSessionMethodConfig{
		Recipient:      testPayerHex,
		SAClient:       &noopSAClient{},
		ChainID:        chainID,
		EscrowContract: escrow,
		Store:          cs,
	})
	return m, cs
}

// noopSAClient is a minimal SA client that accepts all calls and returns empty receipts.
type noopSAClient struct{}

func (n *noopSAClient) Settle(_ context.Context, _ *saclient.ChargeSettleRequest) (*saclient.ChargeReceipt, error) {
	return &saclient.ChargeReceipt{}, nil
}
func (n *noopSAClient) VerifyHash(_ context.Context, _ *saclient.ChargeVerifyHashRequest) (*saclient.ChargeReceipt, error) {
	return &saclient.ChargeReceipt{}, nil
}
func (n *noopSAClient) SessionOpen(_ context.Context, _ *saclient.SessionOpenRequest) (*saclient.SessionReceipt, error) {
	return &saclient.SessionReceipt{}, nil
}
func (n *noopSAClient) SessionTopUp(_ context.Context, _ *saclient.SessionTopUpRequest) (*saclient.SessionReceipt, error) {
	return &saclient.SessionReceipt{}, nil
}
func (n *noopSAClient) SessionSettle(_ context.Context, _ *saclient.SessionSettleRequest) (*saclient.SessionReceipt, error) {
	return &saclient.SessionReceipt{}, nil
}
func (n *noopSAClient) SessionClose(_ context.Context, _ *saclient.SessionCloseRequest) (*saclient.SessionReceipt, error) {
	return &saclient.SessionReceipt{}, nil
}
func (n *noopSAClient) SessionStatus(_ context.Context, _ string) (*saclient.SessionStatus, error) {
	return &saclient.SessionStatus{}, nil
}

// newTestEVMSessionMethodWithSigner returns an EVMSessionMethod with a payee signer and noop SA client.
func newTestEVMSessionMethodWithSigner(escrow string, chainID uint64) (*EVMSessionMethod, store.Store[store.ChannelState]) {
	key, _ := crypto.GenerateKey()
	signer := NewPrivateKeySigner(key)
	cs := store.NewMemoryStore[store.ChannelState]()
	m, _ := NewEVMSessionMethod(EVMSessionMethodConfig{
		Recipient:      strings.ToLower(crypto.PubkeyToAddress(key.PublicKey).Hex()),
		SAClient:       &noopSAClient{},
		ChainID:        chainID,
		EscrowContract: escrow,
		Signer:         signer,
		Store:          cs,
	})
	return m, cs
}

// seedChannelState inserts a ChannelState into the store.
func seedChannelState(t *testing.T, cs store.Store[store.ChannelState], channelID string, state *store.ChannelState) {
	t.Helper()
	if err := cs.Put(context.Background(), channelID, state); err != nil {
		t.Fatalf("seed channel %q: %v", channelID, err)
	}
}

// makeChallengeEcho returns a minimal ChallengeEcho.
func makeChallengeEcho(id string) *protocol.ChallengeEcho {
	return &protocol.ChallengeEcho{ID: id}
}

// mustTestSigner creates a deterministic signer for tests.
// Uses testPayerHex's known hardhat private key.
func mustTestSigner(t *testing.T) Signer {
	t.Helper()
	// Hardhat account #0 private key
	key, err := crypto.HexToECDSA("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
	if err != nil {
		t.Fatalf("mustTestSigner: %v", err)
	}
	return NewPrivateKeySigner(key)
}

// signTestVoucher signs a voucher for testing and returns hex string with 0x prefix.
func signTestVoucher(t *testing.T, channelID [32]byte, amount *big.Int) string {
	t.Helper()
	signer := mustTestSigner(t)
	sig, err := SignVoucher(signer, channelID, amount, common.HexToAddress(testEscrowHex), testChainID, "", "")
	if err != nil {
		t.Fatalf("signTestVoucher: %v", err)
	}
	return "0x" + hex.EncodeToString(sig)
}

// makeProofSource returns an EVM proof source string for the hardhat address.
func makeProofSource() string {
	return fmt.Sprintf("did:pkh:eip155:%d:%s", testChainID, testPayerHex)
}

// makeOpenPayloadJSON returns a valid hash-mode open payload JSON string.
func makeOpenPayloadJSON(deposit string) string {
	// Generate a proper 65-byte voucher signature for cum=0.
	key, _ := crypto.GenerateKey()
	signer := NewPrivateKeySigner(key)
	channelID, _ := hexDecode32("0x01")
	sig, _ := SignVoucher(signer, channelID, big.NewInt(0), common.HexToAddress(testEscrowHex), testChainID, "", "")
	sigHex := "0x" + hex.EncodeToString(sig)
	return fmt.Sprintf(`{"action":"open","type":"hash","channelId":"0x01","salt":"0x02","cumulativeAmount":"0","signature":%q,"hash":"0xabc","deposit":%q,"authorizedSigner":%q}`, sigHex, deposit, crypto.PubkeyToAddress(key.PublicKey).Hex())
}

// makeTopUpPayloadJSON returns a valid hash-mode topUp payload JSON string.
func makeTopUpPayloadJSON(channelID, additionalDeposit string) string {
	return fmt.Sprintf(`{"action":"topUp","type":"hash","channelId":%q,"additionalDeposit":%q,"hash":"0xabc"}`, channelID, additionalDeposit)
}

// makeSessionRequest builds a SessionRequest embedding EVMSessionMethodDetails as MethodDetails.
func makeSessionRequest(escrow string, chainID uint64) *protocol.SessionRequest {
	chainIDVal := chainID
	details := EVMSessionMethodDetails{
		EscrowContract: escrow,
		ChainID:        &chainIDVal,
	}
	raw, _ := json.Marshal(details)
	return &protocol.SessionRequest{
		MethodDetails: json.RawMessage(raw),
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Builder tests
// ─────────────────────────────────────────────────────────────────────────────

func TestEVMSessionMethodConfig(t *testing.T) {
	cs := store.NewMemoryStore[store.ChannelState]()

	m, err := NewEVMSessionMethod(EVMSessionMethodConfig{
		Recipient:      testPayerHex,
		SAClient:       &noopSAClient{},
		ChainID:        testChainID,
		EscrowContract: testEscrowHex,
		FeePayer:       true,
		Store:          cs,
	})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if m.chainID != testChainID {
		t.Errorf("chainID: got %d want %d", m.chainID, testChainID)
	}
	if m.escrowContract != testEscrowHex {
		t.Errorf("escrowContract: got %s want %s", m.escrowContract, testEscrowHex)
	}
	if m.recipient != testPayerHex {
		t.Errorf("recipient mismatch")
	}
	if !m.feePayer {
		t.Errorf("feePayer: expected true")
	}
	if m.ChannelStore() == nil {
		t.Errorf("ChannelStore: expected non-nil")
	}
}

func TestEVMSessionMethodConfigValidation(t *testing.T) {
	_, err := NewEVMSessionMethod(EVMSessionMethodConfig{})
	if err == nil {
		t.Error("expected error for empty config")
	}
	_, err = NewEVMSessionMethod(EVMSessionMethodConfig{Recipient: testPayerHex})
	if err == nil {
		t.Error("expected error for missing SAClient")
	}
	_, err = NewEVMSessionMethod(EVMSessionMethodConfig{SAClient: &noopSAClient{}})
	if err == nil {
		t.Error("expected error for missing Recipient")
	}
}

func TestEVMSessionMethodConfigDefaults(t *testing.T) {
	m, err := NewEVMSessionMethod(EVMSessionMethodConfig{
		Recipient: testPayerHex,
		SAClient:  &noopSAClient{},
	})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if m.chainID != XLayerChainID {
		t.Errorf("default chainID: got %d want %d", m.chainID, XLayerChainID)
	}
	if m.escrowContract != DefaultEscrowContract {
		t.Errorf("default escrow: got %s want %s", m.escrowContract, DefaultEscrowContract)
	}
	if m.domainName != DefaultDomainName {
		t.Errorf("default domainName: got %s want %s", m.domainName, DefaultDomainName)
	}
	if m.channels == nil {
		t.Error("default store should be non-nil")
	}
	if m.nonceProvider == nil {
		t.Error("default nonceProvider should be non-nil")
	}
}

func TestEVMSessionMethodMethod(t *testing.T) {
	m, _ := NewEVMSessionMethod(EVMSessionMethodConfig{
		Recipient: testPayerHex,
		SAClient:  &noopSAClient{},
	})
	if got := m.Method(); got != MethodNameEVM {
		t.Errorf("Method() = %q, want %q", got, MethodNameEVM)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ChallengeMethodDetails tests
// ─────────────────────────────────────────────────────────────────────────────

func TestChallengeMethodDetailsNilWhenNoEscrow(t *testing.T) {
	m := &EVMSessionMethod{escrowContract: ""}
	if got := m.ChallengeMethodDetails(); got != nil {
		t.Errorf("expected nil when no escrow, got %s", string(got))
	}
}

func TestChallengeMethodDetailsWithEscrow(t *testing.T) {
	m := &EVMSessionMethod{
		escrowContract: testEscrowHex,
		chainID:        testChainID,
		feePayer:       true,
	}

	raw := m.ChallengeMethodDetails()
	if raw == nil {
		t.Fatal("expected non-nil ChallengeMethodDetails")
	}

	var details EVMSessionMethodDetails
	if err := json.Unmarshal(raw, &details); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if details.EscrowContract != testEscrowHex {
		t.Errorf("EscrowContract: got %s want %s", details.EscrowContract, testEscrowHex)
	}
	if details.ChainID == nil || *details.ChainID != testChainID {
		t.Errorf("ChainID: unexpected value %v", details.ChainID)
	}
	if details.FeePayer == nil || !*details.FeePayer {
		t.Errorf("FeePayer: expected true")
	}
}

func TestChallengeMethodDetailsNoChainIDNoFeePayer(t *testing.T) {
	m := &EVMSessionMethod{escrowContract: testEscrowHex}
	raw := m.ChallengeMethodDetails()
	if raw == nil {
		t.Fatal("expected non-nil")
	}
	var details EVMSessionMethodDetails
	if err := json.Unmarshal(raw, &details); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if details.ChainID != nil {
		t.Errorf("ChainID: expected nil, got %v", *details.ChainID)
	}
	if details.FeePayer == nil || *details.FeePayer != false {
		t.Errorf("FeePayer: expected false, got %v", details.FeePayer)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Payload unmarshal + dispatch tests
// ─────────────────────────────────────────────────────────────────────────────

func TestVerifySession_EmptyPayload(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Payload: &protocol.PaymentPayload{Payload: ""},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for empty payload")
	}
}

func TestVerifySession_InvalidJSON(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Payload: &protocol.PaymentPayload{Payload: "not-json"},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for invalid JSON")
	}
}

func TestVerifySession_UnknownAction(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Payload: &protocol.PaymentPayload{Payload: `{"action":"unknown"}`},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for unknown action")
	}
}

func TestPayloadActionEmbedding(t *testing.T) {
	raw := `{"action":"open","type":"hash","channelId":"0x01","deposit":"1000"}`
	var p OpenPayload
	if err := json.Unmarshal([]byte(raw), &p); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if p.Action != ActionOpen {
		t.Errorf("Action: got %q want %q", p.Action, ActionOpen)
	}
	if p.ChannelID != "0x01" {
		t.Errorf("ChannelID: got %q want %q", p.ChannelID, "0x01")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// VerifySession error path tests
// ─────────────────────────────────────────────────────────────────────────────

func TestVerifySessionMissingCredential(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	_, err := m.VerifySession(context.Background(), nil, nil)
	if err == nil {
		t.Error("expected error for nil credential")
	}
}

func TestVerifySessionMissingEcho(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Payload: &protocol.PaymentPayload{Type: protocol.PayloadTypeProof, Payload: "{}"},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing echo")
	}
}

func TestVerifySessionMissingPayload(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Echo: makeChallengeEcho("chal1"),
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing payload")
	}
}

func TestVerifySessionInvalidPayloadJSON(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Payload: &protocol.PaymentPayload{Payload: "not-json"},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for invalid payload JSON")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// handleOpen tests
// ─────────────────────────────────────────────────────────────────────────────

func TestHandleOpen_Success(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)

	request := makeSessionRequest(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal-open"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: makeOpenPayloadJSON("1000000")},
	}

	receipt, err := m.VerifySession(context.Background(), cred, request)
	if err != nil {
		t.Fatalf("handleOpen: %v", err)
	}
	if receipt == nil {
		t.Fatal("expected non-nil receipt")
	}
}

func TestHandleOpen_MissingDeposit(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	request := makeSessionRequest(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Payload: &protocol.PaymentPayload{Payload: makeOpenPayloadJSON("")},
	}
	_, err := m.VerifySession(context.Background(), cred, request)
	if err == nil {
		t.Error("expected error for missing deposit")
	}
}

func TestHandleOpen_InvalidSource(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	request := makeSessionRequest(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  "invalid-source",
		Payload: &protocol.PaymentPayload{Payload: makeOpenPayloadJSON("1000")},
	}
	_, err := m.VerifySession(context.Background(), cred, request)
	if err == nil {
		t.Error("expected error for invalid source")
	}
}

func TestHandleOpen_NoEscrow(t *testing.T) {
	// Method has no escrow, request has no escrow -> should fail
	m := &EVMSessionMethod{
		chainID:  testChainID,
		channels: store.NewMemoryStore[store.ChannelState](),
		saClient: &noopSAClient{},
	}
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Payload: &protocol.PaymentPayload{Payload: makeOpenPayloadJSON("1000")},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing escrow")
	}
}

func TestHandleOpen_NoStore(t *testing.T) {
	m, _ := NewEVMSessionMethod(EVMSessionMethodConfig{
		Recipient:      testPayerHex,
		SAClient:       &noopSAClient{},
		ChainID:        testChainID,
		EscrowContract: testEscrowHex,
	})
	// NewEVMSessionMethod creates a default store, so this test
	// verifies that the default store still works (no nil panic).
	request := makeSessionRequest(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: makeOpenPayloadJSON("1000")},
	}
	receipt, err := m.VerifySession(context.Background(), cred, request)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if receipt == nil {
		t.Fatal("expected non-nil receipt")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// handleTopUp tests
// ─────────────────────────────────────────────────────────────────────────────

func TestHandleTopUp_Success(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex

	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:            channelID,
		Payer:                strings.ToLower(testPayerHex),
		Deposit:              big.NewInt(1000000),
		HighestVoucherAmount: new(big.Int),
		Finalized:            false,
		EscrowContract:       testEscrowHex,
		ChainID:              testChainID,
		Spent:                new(big.Int),
	})

	raw := makeTopUpPayloadJSON(channelID, "500000")
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal-topup"),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	receipt, err := m.VerifySession(context.Background(), cred, nil)
	if err != nil {
		t.Fatalf("handleTopUp: %v", err)
	}
	if receipt == nil {
		t.Fatal("expected non-nil receipt")
	}

	// Verify updated deposit in store.
	state, err := m.ChannelStore().Get(context.Background(), channelID)
	if err != nil {
		t.Fatalf("GetChannel: %v", err)
	}
	if state.Deposit.String() != "1500000" {
		t.Errorf("Deposit: got %s want 1500000", state.Deposit.String())
	}
}

func TestHandleTopUp_MissingChannelID(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Payload: &protocol.PaymentPayload{Payload: makeTopUpPayloadJSON("", "500")},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing channelId")
	}
}

func TestHandleTopUp_MissingAmount(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:      channelID,
		Deposit:        big.NewInt(1000),
		Finalized:      false,
		Spent:          new(big.Int),
	})
	raw := makeTopUpPayloadJSON(channelID, "")
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing additionalDeposit")
	}
}

func TestHandleTopUp_ChannelClosed(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:      channelID,
		Deposit:        big.NewInt(1000),
		Finalized:      true,
		Spent:          new(big.Int),
	})
	raw := makeTopUpPayloadJSON(channelID, "500")
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for closed channel")
	}
}

func TestHandleTopUp_ChannelNotFound(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	raw := makeTopUpPayloadJSON(channelID, "500")
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing channel")
	}
}

func TestHandleTopUp_InvalidTopUpAmount(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:      channelID,
		Deposit:        big.NewInt(1000),
		Finalized:      false,
		Spent:          new(big.Int),
	})
	raw := makeTopUpPayloadJSON(channelID, "bad")
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for invalid top-up amount")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// handleVoucher tests
// ─────────────────────────────────────────────────────────────────────────────

func TestHandleVoucher_NewVoucherAccepted(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	amount := big.NewInt(500)

	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:            channelID,
		Payer:                strings.ToLower(testPayerHex),
		Deposit:              big.NewInt(1000000),
		HighestVoucherAmount: new(big.Int),
		Finalized:            false,
		EscrowContract:       testEscrowHex,
		ChainID:              testChainID,
		Spent:                new(big.Int),
	})

	channelIDBytes, _ := hexDecode32(channelID)
	sigHex := signTestVoucher(t, channelIDBytes, amount)

	raw := fmt.Sprintf(`{"action":"voucher","channelId":%q,"cumulativeAmount":"%s","signature":%q}`, channelID, amount.String(), sigHex)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal-voucher"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}

	receipt, err := m.VerifySession(context.Background(), cred, nil)
	if err != nil {
		t.Fatalf("handleVoucher: %v", err)
	}
	if receipt == nil {
		t.Fatal("expected receipt")
	}

	// Verify cumulative amount updated.
	state, err := m.ChannelStore().Get(context.Background(), channelID)
	if err != nil {
		t.Fatalf("GetChannel: %v", err)
	}
	if state.HighestVoucherAmount.String() != "500" {
		t.Errorf("HighestVoucherAmount: got %s want 500", state.HighestVoucherAmount.String())
	}
}

func TestHandleVoucher_MissingChannelID(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: `{"action":"voucher","channelId":"","cumulativeAmount":"500","signature":"0xabc"}`},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing channelId")
	}
}

func TestHandleVoucher_MissingAmount(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"","signature":"0xabc"}`, testChannelIDHex)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing amount")
	}
}

func TestHandleVoucher_MissingSignature(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"500","signature":""}`, testChannelIDHex)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing signature")
	}
}

func TestHandleVoucher_MissingSource(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:      channelID,
		Deposit:        big.NewInt(1000),
		Finalized:      false,
		ChainID:        testChainID,
		Spent:          new(big.Int),
	})
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"500","signature":"0xabc"}`, channelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing source")
	}
}

func TestHandleVoucher_InvalidSource(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:      channelID,
		Deposit:        big.NewInt(1000),
		Finalized:      false,
		ChainID:        testChainID,
		Spent:          new(big.Int),
	})
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"500","signature":"0xabc"}`, channelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  "bad-source",
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for invalid source")
	}
}

func TestHandleVoucher_ChainIDMismatch(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, 999)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:      channelID,
		Deposit:        big.NewInt(1000),
		Finalized:      false,
		ChainID:        999,
		Spent:          new(big.Int),
	})
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"500","signature":"0xabc"}`, channelID)
	// Source says chain 1, but method expects chain 999
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(), // chain 1
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected chain ID mismatch error")
	}
}

func TestHandleVoucher_InvalidChannelIDHex(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	badChannelID := "0xZZZZ"
	seedChannelState(t, cs, badChannelID, &store.ChannelState{
		ChannelID:      badChannelID,
		Deposit:        big.NewInt(1000),
		Finalized:      false,
		ChainID:        testChainID,
		Spent:          new(big.Int),
	})
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"500","signature":"0xabc"}`, badChannelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for invalid channelId hex")
	}
}

func TestHandleVoucher_InvalidAmount(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:            channelID,
		Deposit:              big.NewInt(1000000),
		HighestVoucherAmount: new(big.Int),
		Finalized:            false,
		ChainID:              testChainID,
		EscrowContract:       testEscrowHex,
		Spent:                new(big.Int),
	})
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"not-a-number","signature":"0xabc"}`, channelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for invalid amount")
	}
}

func TestHandleVoucher_ReplayAccepted(t *testing.T) {
	// Replay vouchers (amount == HighestVoucherAmount, same sig) should be accepted
	// without signature verification against the chain.
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	storedSig, _ := hex.DecodeString("abcdef")
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:               channelID,
		Deposit:                 big.NewInt(1000000),
		HighestVoucherAmount:    big.NewInt(600),
		HighestVoucherSignature: storedSig,
		Finalized:               false,
		ChainID:                 testChainID,
		EscrowContract:          testEscrowHex,
		Spent:                   new(big.Int),
	})
	// amount == HighestVoucherAmount, same signature → replay
	raw := fmt.Sprintf(`{"action":"voucher","channelId":%q,"cumulativeAmount":"600","signature":"0xabcdef"}`, channelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	receipt, err := m.VerifySession(context.Background(), cred, nil)
	if err != nil {
		t.Fatalf("replay voucher should succeed, got error: %v", err)
	}
	if receipt == nil {
		t.Fatal("expected non-nil receipt for replay voucher")
	}
}

func TestHandleVoucher_ReplayWithDeduction(t *testing.T) {
	// Replay with perRequestCost set should deduct from balance.
	cs := store.NewMemoryStore[store.ChannelState]()
	m, _ := NewEVMSessionMethod(EVMSessionMethodConfig{
		Recipient:      testPayerHex,
		SAClient:       &noopSAClient{},
		ChainID:        testChainID,
		EscrowContract: testEscrowHex,
		Store:          cs,
		PerRequestCost: big.NewInt(100),
	})
	channelID := testChannelIDHex
	storedSig, _ := hex.DecodeString("abcdef")
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:               channelID,
		Deposit:                 big.NewInt(1000000),
		HighestVoucherAmount:    big.NewInt(600),
		HighestVoucherSignature: storedSig,
		Finalized:               false,
		ChainID:                 testChainID,
		EscrowContract:          testEscrowHex,
		Spent:                   new(big.Int),
	})
	// amount == HighestVoucherAmount, same sig → replay, deducts 100
	raw := fmt.Sprintf(`{"action":"voucher","channelId":%q,"cumulativeAmount":"600","signature":"0xabcdef"}`, channelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err != nil {
		t.Fatalf("first replay should succeed: %v", err)
	}
	state, _ := m.ChannelStore().Get(context.Background(), channelID)
	if state.Spent.Cmp(big.NewInt(100)) != 0 {
		t.Errorf("Spent after first request: got %s, want 100", state.Spent)
	}
	if state.Units != 1 {
		t.Errorf("Units after first request: got %d, want 1", state.Units)
	}
}

func TestHandleVoucher_ReplayInsufficientBalance(t *testing.T) {
	// Replay when balance is exhausted should fail.
	cs := store.NewMemoryStore[store.ChannelState]()
	m, _ := NewEVMSessionMethod(EVMSessionMethodConfig{
		Recipient:      testPayerHex,
		SAClient:       &noopSAClient{},
		ChainID:        testChainID,
		EscrowContract: testEscrowHex,
		Store:          cs,
		PerRequestCost: big.NewInt(100),
	})
	channelID := testChannelIDHex
	storedSig, _ := hex.DecodeString("abcdef")
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:               channelID,
		Deposit:                 big.NewInt(1000000),
		HighestVoucherAmount:    big.NewInt(600),
		HighestVoucherSignature: storedSig,
		Finalized:               false,
		ChainID:                 testChainID,
		EscrowContract:          testEscrowHex,
		Spent:                   big.NewInt(550), // only 50 available
	})
	// amount == HighestVoucherAmount, same sig → replay, but only 50 available < 100 cost
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"600","signature":"0xabcdef"}`, channelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error when balance insufficient for deduction")
	}
}

func TestHandleVoucher_AmountExceedsDeposit(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:            channelID,
		Deposit:              big.NewInt(100),
		HighestVoucherAmount: new(big.Int),
		Finalized:            false,
		ChainID:              testChainID,
		EscrowContract:       testEscrowHex,
		Spent:                new(big.Int),
	})
	sigHex := "0x" + strings.Repeat("00", 65)
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"500","signature":%q}`, channelID, sigHex)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for amount exceeding deposit")
	}
}

func TestHandleVoucher_InvalidSignatureHex(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:            channelID,
		Deposit:              big.NewInt(1000000),
		HighestVoucherAmount: new(big.Int),
		Finalized:            false,
		ChainID:              testChainID,
		EscrowContract:       testEscrowHex,
		Spent:                new(big.Int),
	})
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"500","signature":"0xZZZZZZ"}`, channelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for invalid signature hex")
	}
}

func TestHandleVoucher_ClosedChannel(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:      channelID,
		Deposit:        big.NewInt(1000),
		Finalized:      true,
		Spent:          new(big.Int),
	})
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"500","signature":"0xabc"}`, channelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for closed channel")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// handleClose tests
// ─────────────────────────────────────────────────────────────────────────────

func TestHandleClose_Success(t *testing.T) {
	m, cs := newTestEVMSessionMethodWithSigner(testEscrowHex, testChainID)
	channelID := testChannelIDHex

	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:            channelID,
		Payer:                strings.ToLower(testPayerHex),
		Deposit:              big.NewInt(1000000),
		HighestVoucherAmount: big.NewInt(500),
		Finalized:            false,
		EscrowContract:       testEscrowHex,
		ChainID:              testChainID,
		Spent:                new(big.Int),
	})

	// Sign a real voucher for close verification.
	channelIDBytes, _ := hexDecode32(channelID)
	closeAmount := big.NewInt(800)
	sigHex := signTestVoucher(t, channelIDBytes, closeAmount)

	raw := fmt.Sprintf(`{"action":"close","channelId":%q,"cumulativeAmount":"800","signature":%q}`, channelID, sigHex)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal-close"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}

	receipt, err := m.VerifySession(context.Background(), cred, nil)
	if err != nil {
		t.Fatalf("handleClose: %v", err)
	}
	if receipt == nil {
		t.Fatal("expected receipt")
	}

	// Channel should be removed after close.
	removed, _ := m.ChannelStore().Get(context.Background(), channelID)
	if removed != nil {
		t.Error("expected channel to be removed after close")
	}
}

func TestHandleClose_MissingChannelID(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: `{"channelId":"","cumulativeAmount":"800","signature":"0xabc"}`},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing channelId")
	}
}

func TestHandleClose_MissingFinalAmount(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"","signature":"0xabc"}`, testChannelIDHex)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing cumulativeAmount")
	}
}

func TestHandleClose_MissingSignature(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"800","signature":""}`, testChannelIDHex)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing signature")
	}
}

func TestHandleClose_ChannelNotFound(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	sigHex := "0x" + strings.Repeat("00", 65)
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"800","signature":%q}`, testChannelIDHex, sigHex)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for channel not found")
	}
}

func TestHandleClose_AlreadyClosed(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:      channelID,
		Deposit:        big.NewInt(1000),
		Finalized:      true,
		Spent:          new(big.Int),
	})
	sigHex := "0x" + strings.Repeat("00", 65)
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"800","signature":%q}`, channelID, sigHex)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for already-closed channel")
	}
}

func TestHandleClose_MissingSource(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:      channelID,
		Deposit:        big.NewInt(1000),
		Finalized:      false,
		ChainID:        testChainID,
		Spent:          new(big.Int),
	})
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"800","signature":"0xabc"}`, channelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for missing source")
	}
}

func TestHandleClose_ChainIDMismatch(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, 999)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:      channelID,
		Deposit:        big.NewInt(1000),
		Finalized:      false,
		ChainID:        999,
		Spent:          new(big.Int),
	})
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"800","signature":"0xabc"}`, channelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(), // chain 1
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected chain ID mismatch error")
	}
}

func TestHandleClose_InvalidChannelIDHex(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	badChannelID := "0xZZZZ"
	seedChannelState(t, cs, badChannelID, &store.ChannelState{
		ChannelID:      badChannelID,
		Deposit:        big.NewInt(1000),
		Finalized:      false,
		ChainID:        testChainID,
		Spent:          new(big.Int),
	})
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"800","signature":"0xabc"}`, badChannelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for invalid channelId hex")
	}
}

func TestHandleClose_InvalidFinalAmount(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:            channelID,
		Deposit:              big.NewInt(1000),
		HighestVoucherAmount: new(big.Int),
		Finalized:            false,
		ChainID:              testChainID,
		EscrowContract:       testEscrowHex,
		Spent:                new(big.Int),
	})
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"bad","signature":"0xabc"}`, channelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for invalid finalAmount")
	}
}

func TestHandleClose_InvalidSignatureHex(t *testing.T) {
	m, cs := newTestEVMSessionMethod(testEscrowHex, testChainID)
	channelID := testChannelIDHex
	seedChannelState(t, cs, channelID, &store.ChannelState{
		ChannelID:            channelID,
		Deposit:              big.NewInt(1000000),
		HighestVoucherAmount: new(big.Int),
		Finalized:            false,
		ChainID:              testChainID,
		EscrowContract:       testEscrowHex,
		Spent:                new(big.Int),
	})
	raw := fmt.Sprintf(`{"channelId":%q,"cumulativeAmount":"800","signature":"0xZZZZZZ"}`, channelID)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal1"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: raw},
	}
	_, err := m.VerifySession(context.Background(), cred, nil)
	if err == nil {
		t.Error("expected error for invalid signature hex")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// resolveChainAndEscrow tests
// ─────────────────────────────────────────────────────────────────────────────

func TestResolveChainAndEscrow_FromInstance(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	chainID, addr, err := m.resolveChainAndEscrow(nil)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if chainID != testChainID {
		t.Errorf("chainID: got %d want %d", chainID, testChainID)
	}
	if addr != testEscrowHex {
		t.Errorf("addr mismatch: got %s want %s", addr, testEscrowHex)
	}
}

func TestResolveChainAndEscrow_FromRequest(t *testing.T) {
	// Instance has no escrow; request provides it
	m := &EVMSessionMethod{}
	request := makeSessionRequest(testEscrowHex, testChainID)
	chainID, addr, err := m.resolveChainAndEscrow(request)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if chainID != testChainID {
		t.Errorf("chainID: got %d want %d", chainID, testChainID)
	}
	if addr != testEscrowHex {
		t.Errorf("addr mismatch: got %s want %s", addr, testEscrowHex)
	}
}

func TestResolveChainAndEscrow_NoEscrow(t *testing.T) {
	m := &EVMSessionMethod{}
	_, _, err := m.resolveChainAndEscrow(nil)
	if err == nil {
		t.Error("expected error for no escrow")
	}
}

func TestResolveChainAndEscrow_RequestOverridesInstance(t *testing.T) {
	instanceEscrow := "0x1111111111111111111111111111111111111111"
	requestEscrow := "0x2222222222222222222222222222222222222222"
	m, _ := newTestEVMSessionMethod(instanceEscrow, testChainID)
	requestChainID := uint64(42)
	request := makeSessionRequest(requestEscrow, requestChainID)
	chainID, addr, err := m.resolveChainAndEscrow(request)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if chainID != requestChainID {
		t.Errorf("chainID: got %d want %d", chainID, requestChainID)
	}
	if addr != requestEscrow {
		t.Errorf("addr: got %s want %s", addr, requestEscrow)
	}
}

func TestResolveChainAndEscrow_InvalidRequestDetails(t *testing.T) {
	// Request has invalid JSON in MethodDetails — should fall back to instance config
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	request := &protocol.SessionRequest{
		MethodDetails: json.RawMessage(`{invalid-json}`),
	}
	// parseSessionDetails returns nil on bad JSON; falls back to instance config
	chainID, addr, err := m.resolveChainAndEscrow(request)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if chainID != testChainID {
		t.Errorf("chainID: got %d want %d", chainID, testChainID)
	}
	if addr != testEscrowHex {
		t.Errorf("addr mismatch")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// hexDecode32 tests
// ─────────────────────────────────────────────────────────────────────────────

func TestHexDecode32_Success(t *testing.T) {
	cases := []struct {
		input string
	}{
		{"0x01"},
		{"0x0000000000000000000000000000000000000000000000000000000000000001"},
		{"01"},
	}
	for _, tc := range cases {
		_, err := hexDecode32(tc.input)
		if err != nil {
			t.Errorf("hexDecode32(%q) error: %v", tc.input, err)
		}
	}
}

func TestHexDecode32_RightAligned(t *testing.T) {
	result, err := hexDecode32("0x01")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result[31] != 1 {
		t.Errorf("byte[31]: got %d want 1", result[31])
	}
	for i := 0; i < 31; i++ {
		if result[i] != 0 {
			t.Errorf("byte[%d]: got %d want 0", i, result[i])
		}
	}
}

func TestHexDecode32_TooLong(t *testing.T) {
	longHex := "0x" + fmt.Sprintf("%066x", 1)
	_, err := hexDecode32(longHex)
	if err == nil {
		t.Error("expected error for value too long")
	}
}

func TestHexDecode32_InvalidHex(t *testing.T) {
	_, err := hexDecode32("0xZZZZ")
	if err == nil {
		t.Error("expected error for invalid hex")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Respond tests
// ─────────────────────────────────────────────────────────────────────────────

func TestRespond_VoucherReturnsNil(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Payload: &protocol.PaymentPayload{
			Payload: `{"action":"voucher","channelId":"0x01","cumulativeAmount":"30","signature":"0xdeadbeef"}`,
		},
	}
	receipt := protocol.NewSuccessReceipt("id", "evm", protocol.IntentSession, "")
	result := m.Respond(cred, receipt)
	if result != nil {
		t.Errorf("expected nil for voucher action, got %v", result)
	}
}

func TestRespond_OpenReturnsManagement(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Payload: &protocol.PaymentPayload{
			Payload: `{"action":"open","type":"hash","channelId":"0x01","salt":"0x02","cumulativeAmount":"0","signature":"0xaa","hash":"0xbb","deposit":"60"}`,
		},
	}
	receipt := protocol.NewSuccessReceipt("id", "evm", protocol.IntentSession, "")
	result := m.Respond(cred, receipt)
	if result == nil {
		t.Error("expected non-nil management response for open action")
	}
}

func TestRespond_TopUpReturnsManagement(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Payload: &protocol.PaymentPayload{
			Payload: `{"action":"topUp","type":"hash","channelId":"0x01","additionalDeposit":"60","hash":"0xbb"}`,
		},
	}
	receipt := protocol.NewSuccessReceipt("id", "evm", protocol.IntentSession, "")
	result := m.Respond(cred, receipt)
	if result == nil {
		t.Error("expected non-nil management response for topUp action")
	}
}

func TestRespond_CloseReturnsManagement(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	cred := &protocol.PaymentCredential{
		Payload: &protocol.PaymentPayload{
			Payload: `{"action":"close","channelId":"0x01","cumulativeAmount":"30","signature":"0xaa"}`,
		},
	}
	receipt := protocol.NewSuccessReceipt("id", "evm", protocol.IntentSession, "")
	result := m.Respond(cred, receipt)
	if result == nil {
		t.Error("expected non-nil management response for close action")
	}
}

func TestRespond_NilCredential(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	result := m.Respond(nil, nil)
	if result != nil {
		t.Errorf("expected nil for nil credential, got %v", result)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// handleOpen voucher signature verification tests
// ─────────────────────────────────────────────────────────────────────────────

func TestHandleOpen_RejectsInvalidVoucherSig_WhenCumPositive(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	request := makeSessionRequest(testEscrowHex, testChainID)

	// Build open payload with cum > 0 but garbage sig
	payload := `{"action":"open","type":"hash","channelId":"0x01","salt":"0x02","cumulativeAmount":"100","signature":"0xdeadbeef","hash":"0xabc","deposit":"1000"}`
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal-bad-sig"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: payload},
	}

	_, err := m.VerifySession(context.Background(), cred, request)
	if err == nil {
		t.Fatal("expected error for invalid voucher sig with cum > 0")
	}
	if !strings.Contains(err.Error(), "signature") {
		t.Errorf("expected signature error, got: %v", err)
	}
}

func TestHandleOpen_AcceptsValidVoucherSig_WhenCumPositive(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	request := makeSessionRequest(testEscrowHex, testChainID)

	// Generate a real voucher sig for cum=100
	key, _ := crypto.GenerateKey()
	signer := NewPrivateKeySigner(key)
	channelID, _ := hexDecode32("0x01")
	sig, err := SignVoucher(signer, channelID, big.NewInt(100), common.HexToAddress(testEscrowHex), testChainID, "", "")
	if err != nil {
		t.Fatalf("sign voucher: %v", err)
	}
	sigHex := "0x" + hex.EncodeToString(sig)
	signerAddr := crypto.PubkeyToAddress(key.PublicKey).Hex()

	payload := fmt.Sprintf(`{"action":"open","type":"hash","channelId":"0x01","salt":"0x02","cumulativeAmount":"100","signature":%q,"hash":"0xabc","deposit":"1000","authorizedSigner":%q}`, sigHex, signerAddr)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal-good-sig"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: payload},
	}

	receipt, err := m.VerifySession(context.Background(), cred, request)
	if err != nil {
		t.Fatalf("expected success, got: %v", err)
	}
	if receipt == nil {
		t.Fatal("expected non-nil receipt")
	}
}

func TestHandleOpen_SkipsVerification_WhenCumZero(t *testing.T) {
	m, _ := newTestEVMSessionMethod(testEscrowHex, testChainID)
	request := makeSessionRequest(testEscrowHex, testChainID)

	// cum=0 with dummy sig — should pass (no verification for zero amount)
	cred := &protocol.PaymentCredential{
		Echo:    makeChallengeEcho("chal-zero"),
		Source:  makeProofSource(),
		Payload: &protocol.PaymentPayload{Payload: makeOpenPayloadJSON("1000")},
	}

	receipt, err := m.VerifySession(context.Background(), cred, request)
	if err != nil {
		t.Fatalf("expected success for cum=0, got: %v", err)
	}
	if receipt == nil {
		t.Fatal("expected non-nil receipt")
	}
}
