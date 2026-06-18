package facilitator

import (
	"bytes"
	"context"
	"crypto/ecdsa"
	"errors"
	"fmt"
	"math/big"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/math"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/signer/core/apitypes"

	"github.com/okx/payments/go/x402"
	"github.com/okx/payments/go/x402/mechanisms/evm"
	uptoclient "github.com/okx/payments/go/x402/mechanisms/evm/upto/client"
	"github.com/okx/payments/go/x402/types"
)

// ---------------------------------------------------------------------------
// Compile-time assertion: UptoEvmScheme satisfies x402.SchemeNetworkFacilitator
// ---------------------------------------------------------------------------

var _ x402.SchemeNetworkFacilitator = (*UptoEvmScheme)(nil)

// ---------------------------------------------------------------------------
// Shared test fixtures
// ---------------------------------------------------------------------------

const (
	testPayToAddress = "0x000000000000000000000000000000000000bEEF"
	testTokenAddress = "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
	testNetwork      = "eip155:8453"
	testAmount       = "1000000"
	testTxHash       = "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
)

// ---------------------------------------------------------------------------
// testClientSigner — minimal evm.ClientEvmSigner backed by an ECDSA key.
// Reused from upto/client and upto/server style — kept in-package so this
// test file can build payloads end-to-end via the client builder.
// ---------------------------------------------------------------------------

type testClientSigner struct {
	privateKey *ecdsa.PrivateKey
	address    common.Address
}

func newTestClientSigner(t *testing.T) *testClientSigner {
	t.Helper()
	pk, err := crypto.GenerateKey()
	if err != nil {
		t.Fatalf("generate key: %v", err)
	}
	return &testClientSigner{
		privateKey: pk,
		address:    crypto.PubkeyToAddress(pk.PublicKey),
	}
}

func (s *testClientSigner) Address() string { return s.address.Hex() }

func (s *testClientSigner) SignTypedData(
	ctx context.Context,
	domain evm.TypedDataDomain,
	dataTypes map[string][]evm.TypedDataField,
	primaryType string,
	message map[string]interface{},
) ([]byte, error) {
	td := apitypes.TypedData{
		Types:       make(apitypes.Types),
		PrimaryType: primaryType,
		Domain: apitypes.TypedDataDomain{
			Name:              domain.Name,
			Version:           domain.Version,
			ChainId:           (*math.HexOrDecimal256)(domain.ChainID),
			VerifyingContract: domain.VerifyingContract,
		},
		Message: message,
	}
	for typeName, fields := range dataTypes {
		out := make([]apitypes.Type, len(fields))
		for i, f := range fields {
			out[i] = apitypes.Type{Name: f.Name, Type: f.Type}
		}
		td.Types[typeName] = out
	}
	if _, ok := td.Types["EIP712Domain"]; !ok {
		td.Types["EIP712Domain"] = []apitypes.Type{
			{Name: "name", Type: "string"},
			{Name: "chainId", Type: "uint256"},
			{Name: "verifyingContract", Type: "address"},
		}
	}
	dataHash, err := td.HashStruct(td.PrimaryType, td.Message)
	if err != nil {
		return nil, err
	}
	domSep, err := td.HashStruct("EIP712Domain", td.Domain.Map())
	if err != nil {
		return nil, err
	}
	raw := []byte{0x19, 0x01}
	raw = append(raw, domSep...)
	raw = append(raw, dataHash...)
	digest := crypto.Keccak256(raw)
	sig, err := crypto.Sign(digest, s.privateKey)
	if err != nil {
		return nil, err
	}
	sig[64] += 27
	return sig, nil
}

// ---------------------------------------------------------------------------
// mockFacilitatorSigner — controllable evm.FacilitatorEvmSigner.
// Captures the last WriteContract call so tests can assert the selector and
// target address used for settle().
// ---------------------------------------------------------------------------

type capturedWrite struct {
	Address      string
	ABI          []byte
	FunctionName string
	Args         []interface{}
}

type mockFacilitatorSigner struct {
	addresses []string
	chainID   *big.Int

	// Multicall / ReadContract control
	readContractFn func(address string, abi []byte, functionName string, args ...interface{}) (interface{}, error)

	// WriteContract control
	writeTxHash string
	writeErr    error
	lastWrite   *capturedWrite

	// Receipt control
	receiptStatus uint64
	receiptErr    error

	// VerifyTypedData / GetCode control (used only when ERC-1271 fallback runs)
	verifyTypedDataOK  bool
	verifyTypedDataErr error
	code               []byte
	codeErr            error
}

func (m *mockFacilitatorSigner) GetAddresses() []string {
	if m.addresses == nil {
		return []string{}
	}
	return m.addresses
}

func (m *mockFacilitatorSigner) ReadContract(
	ctx context.Context,
	address string,
	abi []byte,
	functionName string,
	args ...interface{},
) (interface{}, error) {
	if m.readContractFn != nil {
		return m.readContractFn(address, abi, functionName, args...)
	}
	return nil, fmt.Errorf("mock: ReadContract %s on %s not configured", functionName, address)
}

func (m *mockFacilitatorSigner) VerifyTypedData(
	ctx context.Context,
	address string,
	domain evm.TypedDataDomain,
	dataTypes map[string][]evm.TypedDataField,
	primaryType string,
	message map[string]interface{},
	signature []byte,
) (bool, error) {
	if m.verifyTypedDataErr != nil {
		return false, m.verifyTypedDataErr
	}
	return m.verifyTypedDataOK, nil
}

func (m *mockFacilitatorSigner) WriteContract(
	ctx context.Context,
	address string,
	abi []byte,
	functionName string,
	args ...interface{},
) (string, error) {
	m.lastWrite = &capturedWrite{
		Address:      address,
		ABI:          abi,
		FunctionName: functionName,
		Args:         args,
	}
	if m.writeErr != nil {
		return "", m.writeErr
	}
	if m.writeTxHash == "" {
		return testTxHash, nil
	}
	return m.writeTxHash, nil
}

func (m *mockFacilitatorSigner) SendTransaction(ctx context.Context, to string, data []byte) (string, error) {
	return testTxHash, nil
}

func (m *mockFacilitatorSigner) WaitForTransactionReceipt(ctx context.Context, txHash string) (*evm.TransactionReceipt, error) {
	if m.receiptErr != nil {
		return nil, m.receiptErr
	}
	status := m.receiptStatus
	if status == 0 {
		status = evm.TxStatusSuccess
	}
	return &evm.TransactionReceipt{Status: status, BlockNumber: 1, TxHash: txHash}, nil
}

func (m *mockFacilitatorSigner) GetBalance(ctx context.Context, address, tokenAddress string) (*big.Int, error) {
	return big.NewInt(1_000_000_000_000), nil
}

func (m *mockFacilitatorSigner) GetChainID(ctx context.Context) (*big.Int, error) {
	if m.chainID == nil {
		return big.NewInt(8453), nil
	}
	return m.chainID, nil
}

func (m *mockFacilitatorSigner) GetCode(ctx context.Context, address string) ([]byte, error) {
	if m.codeErr != nil {
		return nil, m.codeErr
	}
	return m.code, nil
}

// ---------------------------------------------------------------------------
// Multicall result builder. Replicates the on-the-wire shape that
// evm.Multicall expects from FacilitatorEvmSigner.ReadContract when calling
// Multicall3.tryAggregate.
// ---------------------------------------------------------------------------

type tryAggregateEntry struct {
	Success    bool
	ReturnData []byte
}

func packBigInt(t *testing.T, v *big.Int) []byte {
	t.Helper()
	uint256Ty, err := abi.NewType("uint256", "", nil)
	if err != nil {
		t.Fatalf("abi.NewType: %v", err)
	}
	args := abi.Arguments{{Type: uint256Ty}}
	data, err := args.Pack(v)
	if err != nil {
		t.Fatalf("args.Pack: %v", err)
	}
	return data
}

func packAddress(t *testing.T, addr common.Address) []byte {
	t.Helper()
	addressTy, err := abi.NewType("address", "", nil)
	if err != nil {
		t.Fatalf("abi.NewType: %v", err)
	}
	args := abi.Arguments{{Type: addressTy}}
	data, err := args.Pack(addr)
	if err != nil {
		t.Fatalf("args.Pack: %v", err)
	}
	return data
}

// multicallReadFn returns a readContractFn that handles Multicall3.tryAggregate
// dispatches and any other ReadContract calls (simulate via settle).
func multicallReadFn(
	t *testing.T,
	balance, allowance *big.Int,
	proxyDeployed bool,
	settleOK bool,
	settleErr error,
) func(address string, abi []byte, functionName string, args ...interface{}) (interface{}, error) {
	t.Helper()
	return func(address string, _ []byte, functionName string, args ...interface{}) (interface{}, error) {
		// settle() simulation goes here when called directly (not via multicall)
		if functionName == "settle" || functionName == "settleWithPermit" {
			if !settleOK {
				if settleErr != nil {
					return nil, settleErr
				}
				return nil, fmt.Errorf("settle reverted (simulated)")
			}
			return nil, nil
		}
		if functionName == "tryAggregate" {
			// Multicall3.tryAggregate(bool, calls[])
			entries := []tryAggregateEntry{
				{Success: proxyDeployed, ReturnData: packAddress(t, common.HexToAddress(evm.PERMIT2Address))},
				{Success: true, ReturnData: packBigInt(t, balance)},
				{Success: true, ReturnData: packBigInt(t, allowance)},
			}
			if !proxyDeployed {
				entries[0].ReturnData = nil
			}
			return entries, nil
		}
		return nil, fmt.Errorf("unhandled ReadContract: %s on %s", functionName, address)
	}
}

// ---------------------------------------------------------------------------
// Helpers to build PaymentRequirements + PaymentPayload via the upto client.
// ---------------------------------------------------------------------------

func makeRequirementsFor(facilitatorAddr string) types.PaymentRequirements {
	return types.PaymentRequirements{
		Scheme:            evm.SchemeUpto,
		Network:           testNetwork,
		Asset:             testTokenAddress,
		Amount:            testAmount,
		PayTo:             testPayToAddress,
		MaxTimeoutSeconds: 600,
		Extra: map[string]interface{}{
			"facilitatorAddress": facilitatorAddr,
		},
	}
}

func buildClientPayload(
	t *testing.T,
	signer *testClientSigner,
	requirements types.PaymentRequirements,
) types.PaymentPayload {
	t.Helper()
	clientScheme := uptoclient.NewUptoEvmScheme(signer)
	payload, err := clientScheme.CreatePaymentPayload(context.Background(), requirements)
	if err != nil {
		t.Fatalf("client.CreatePaymentPayload: %v", err)
	}
	payload.Accepted = types.PaymentRequirements{
		Scheme:  requirements.Scheme,
		Network: requirements.Network,
		Asset:   requirements.Asset,
		Amount:  requirements.Amount,
		PayTo:   requirements.PayTo,
	}
	return payload
}

// ---------------------------------------------------------------------------
// Scheme() / CaipFamily() / GetExtra() / GetSigners() metadata tests
// ---------------------------------------------------------------------------

func TestUptoEvmScheme_SchemeMetadata(t *testing.T) {
	signer := &mockFacilitatorSigner{addresses: []string{"0x000000000000000000000000000000000000FaC1"}}
	scheme := NewUptoEvmScheme(signer, nil)

	if got := scheme.Scheme(); got != "upto" {
		t.Errorf("Scheme() = %q, want %q", got, "upto")
	}
	if got := scheme.Scheme(); got != evm.SchemeUpto {
		t.Errorf("Scheme() = %q, want evm.SchemeUpto = %q", got, evm.SchemeUpto)
	}
	if got := scheme.CaipFamily(); got != "eip155:*" {
		t.Errorf("CaipFamily() = %q, want %q", got, "eip155:*")
	}

	extra := scheme.GetExtra(x402.Network(testNetwork))
	if extra == nil {
		t.Fatal("GetExtra returned nil; want map with facilitatorAddress")
	}
	if got, _ := extra[ExtraFacilitatorAddressKey].(string); got != "0x000000000000000000000000000000000000FaC1" {
		t.Errorf("GetExtra[%q] = %q, want first signer addr", ExtraFacilitatorAddressKey, got)
	}

	signers := scheme.GetSigners(x402.Network(testNetwork))
	if len(signers) != 1 || signers[0] != "0x000000000000000000000000000000000000FaC1" {
		t.Errorf("GetSigners = %v, want one facilitator address", signers)
	}
}

func TestUptoEvmScheme_GetExtra_EmptySignerAddresses(t *testing.T) {
	signer := &mockFacilitatorSigner{addresses: []string{}}
	scheme := NewUptoEvmScheme(signer, nil)
	if extra := scheme.GetExtra(x402.Network(testNetwork)); extra != nil {
		t.Errorf("GetExtra() = %v, want nil for empty signer addresses", extra)
	}
}

// ---------------------------------------------------------------------------
// Verify happy path — off-chain green, on-chain balance + allowance green.
// ---------------------------------------------------------------------------

func TestVerifyUptoPermit2_HappyPath(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	facilitatorAddr := clientSigner.Address() // same address for client+facilitator (test-only)
	requirements := makeRequirementsFor(facilitatorAddr)
	payload := buildClientPayload(t, clientSigner, requirements)

	mockSigner := &mockFacilitatorSigner{
		addresses: []string{facilitatorAddr},
		// Balance & allowance sufficient; simulation succeeds (no multicall — settle returns success).
		readContractFn: multicallReadFn(t,
			big.NewInt(1_000_000_000), // balance >> requirements.Amount
			big.NewInt(1_000_000_000), // allowance >> requirements.Amount
			true,                      // proxy deployed
			true,                      // settle simulation OK
			nil,
		),
	}

	uptoPayload, err := evm.UptoPermit2PayloadFromMap(payload.Payload)
	if err != nil {
		t.Fatalf("decode payload: %v", err)
	}

	resp, err := VerifyUptoPermit2(context.Background(), mockSigner, payload, requirements, uptoPayload, nil, nil)
	if err != nil {
		t.Fatalf("VerifyUptoPermit2: %v", err)
	}
	if !resp.IsValid {
		t.Fatalf("IsValid = false, want true (reason=%s)", resp.InvalidReason)
	}
	if !strings.EqualFold(resp.Payer, clientSigner.Address()) {
		t.Errorf("Payer = %q, want %q", resp.Payer, clientSigner.Address())
	}
}

// ---------------------------------------------------------------------------
// Verify rejection paths (off-chain gates)
// ---------------------------------------------------------------------------

func TestVerifyUptoPermit2_RejectsWrongFacilitator(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	// Client signed witness.facilitator = signer.address — but the facilitator
	// signer's GetAddresses returns a DIFFERENT address.
	requirements := makeRequirementsFor(clientSigner.Address())
	payload := buildClientPayload(t, clientSigner, requirements)

	mockSigner := &mockFacilitatorSigner{
		addresses: []string{"0x000000000000000000000000000000000000DEAD"},
	}
	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)

	_, err := VerifyUptoPermit2(context.Background(), mockSigner, payload, requirements, uptoPayload, nil, nil)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !strings.Contains(err.Error(), ErrUptoFacilitatorMismatch) {
		t.Errorf("error %q does not contain %q", err.Error(), ErrUptoFacilitatorMismatch)
	}
}

func TestVerifyUptoPermit2_RejectsTamperedAmountExceedsPermitted(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	requirements := makeRequirementsFor(clientSigner.Address())
	payload := buildClientPayload(t, clientSigner, requirements)
	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)

	// Server now asks for more than the signed cap.
	tampered := requirements
	tampered.Amount = "2000000" // > signed 1_000_000
	payload.Accepted.Amount = "2000000"

	mockSigner := &mockFacilitatorSigner{addresses: []string{clientSigner.Address()}}
	_, err := VerifyUptoPermit2(context.Background(), mockSigner, payload, tampered, uptoPayload, nil, nil)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	// Verify uses strict equality (permitted.amount == requirements.amount),
	// matching TS verifyUptoPermit2 — any mismatch (including req > cap) returns
	// permit2_amount_mismatch. The settle path enforces the ≤-cap rule via
	// ErrUptoSettlementExceedsAmount separately.
	if !strings.Contains(err.Error(), ErrPermit2AmountMismatch) {
		t.Errorf("error %q does not contain %q", err.Error(), ErrPermit2AmountMismatch)
	}
}

func TestVerifyUptoPermit2_RejectsExpiredDeadline(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	requirements := makeRequirementsFor(clientSigner.Address())
	payload := buildClientPayload(t, clientSigner, requirements)

	auth := payload.Payload["permit2Authorization"].(map[string]interface{})
	auth["deadline"] = "1"

	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)
	mockSigner := &mockFacilitatorSigner{addresses: []string{clientSigner.Address()}}

	_, err := VerifyUptoPermit2(context.Background(), mockSigner, payload, requirements, uptoPayload, nil, nil)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !strings.Contains(err.Error(), ErrPermit2DeadlineExpired) {
		t.Errorf("error %q does not contain %q", err.Error(), ErrPermit2DeadlineExpired)
	}
}

func TestVerifyUptoPermit2_RejectsSignatureMismatch(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	requirements := makeRequirementsFor(clientSigner.Address())
	payload := buildClientPayload(t, clientSigner, requirements)

	// Flip a byte of the signature.
	sigHex := payload.Payload["signature"].(string)
	sigBytes, _ := evm.HexToBytes(sigHex)
	sigBytes[0] ^= 0xFF
	payload.Payload["signature"] = evm.BytesToHex(sigBytes)

	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)
	// payer is NOT a deployed contract (code is empty) → must surface ErrPermit2InvalidSignature.
	mockSigner := &mockFacilitatorSigner{
		addresses: []string{clientSigner.Address()},
		code:      nil,
	}

	_, err := VerifyUptoPermit2(context.Background(), mockSigner, payload, requirements, uptoPayload, nil, nil)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !strings.Contains(err.Error(), ErrPermit2InvalidSignature) {
		t.Errorf("error %q does not contain %q", err.Error(), ErrPermit2InvalidSignature)
	}
}

func TestVerifyUptoPermit2_RejectsWrongSpender(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	requirements := makeRequirementsFor(clientSigner.Address())
	payload := buildClientPayload(t, clientSigner, requirements)

	auth := payload.Payload["permit2Authorization"].(map[string]interface{})
	auth["spender"] = "0x0000000000000000000000000000000000000123"

	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)
	mockSigner := &mockFacilitatorSigner{addresses: []string{clientSigner.Address()}}

	_, err := VerifyUptoPermit2(context.Background(), mockSigner, payload, requirements, uptoPayload, nil, nil)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !strings.Contains(err.Error(), ErrPermit2InvalidSpender) {
		t.Errorf("error %q does not contain %q", err.Error(), ErrPermit2InvalidSpender)
	}
}

// Insufficient balance is detected during simulation diagnostic.
func TestVerifyUptoPermit2_DiagnoseInsufficientBalance(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	facilitatorAddr := clientSigner.Address()
	requirements := makeRequirementsFor(facilitatorAddr)
	payload := buildClientPayload(t, clientSigner, requirements)
	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)

	mockSigner := &mockFacilitatorSigner{
		addresses: []string{facilitatorAddr},
		readContractFn: multicallReadFn(t,
			big.NewInt(1),             // balance < requirements.Amount → diagnostic should pick this up
			big.NewInt(1_000_000_000), // allowance is fine
			true,                      // proxy deployed
			false,                     // settle simulation FAILS
			nil,
		),
	}

	_, err := VerifyUptoPermit2(context.Background(), mockSigner, payload, requirements, uptoPayload, nil, nil)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !strings.Contains(err.Error(), ErrPermit2InsufficientBalance) {
		t.Errorf("error %q does not contain %q", err.Error(), ErrPermit2InsufficientBalance)
	}
}

// Insufficient allowance is detected during simulation diagnostic.
func TestVerifyUptoPermit2_DiagnoseInsufficientAllowance(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	facilitatorAddr := clientSigner.Address()
	requirements := makeRequirementsFor(facilitatorAddr)
	payload := buildClientPayload(t, clientSigner, requirements)
	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)

	mockSigner := &mockFacilitatorSigner{
		addresses: []string{facilitatorAddr},
		readContractFn: multicallReadFn(t,
			big.NewInt(1_000_000_000), // balance fine
			big.NewInt(1),             // allowance < requirements.Amount
			true,                      // proxy deployed
			false,                     // settle simulation FAILS
			nil,
		),
	}

	_, err := VerifyUptoPermit2(context.Background(), mockSigner, payload, requirements, uptoPayload, nil, nil)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !strings.Contains(err.Error(), ErrPermit2AllowanceRequired) {
		t.Errorf("error %q does not contain %q", err.Error(), ErrPermit2AllowanceRequired)
	}
}

// ---------------------------------------------------------------------------
// Settle happy path. Asserts:
//   - Success=true, Transaction hash captured
//   - WriteContract called with X402UptoPermit2ProxyAddress as `to`
//   - WriteContract used the upto settle ABI (selector match)
//   - WriteContract's `amount` arg matched the settlement amount
//
// ---------------------------------------------------------------------------

// noSettleSim opts settle out of the eth_call pre-flight. The default is now
// true (matching TS settleUptoPermit2's `?? true`); these settle unit tests
// exercise broadcast / revert-mapping / zero-amount / cap-guard logic, not the
// simulation path (that is covered by the Verify tests above).
var noSettleSim = func() *UptoPermit2FacilitatorConfig {
	skip := false
	return &UptoPermit2FacilitatorConfig{SimulateInSettle: &skip}
}()

func TestSettleUptoPermit2_HappyPath(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	facilitatorAddr := clientSigner.Address()
	requirements := makeRequirementsFor(facilitatorAddr)
	payload := buildClientPayload(t, clientSigner, requirements)
	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)

	mockSigner := &mockFacilitatorSigner{
		addresses:   []string{facilitatorAddr},
		writeTxHash: testTxHash,
	}

	resp, err := SettleUptoPermit2(context.Background(), mockSigner, payload, requirements, uptoPayload, nil, noSettleSim)
	if err != nil {
		t.Fatalf("SettleUptoPermit2: %v", err)
	}
	if !resp.Success {
		t.Fatalf("Success = false, want true")
	}
	if resp.Transaction != testTxHash {
		t.Errorf("Transaction = %q, want %q", resp.Transaction, testTxHash)
	}

	// Assert WriteContract target.
	if mockSigner.lastWrite == nil {
		t.Fatal("WriteContract was not called")
	}
	if !strings.EqualFold(mockSigner.lastWrite.Address, evm.X402UptoPermit2ProxyAddress) {
		t.Errorf("WriteContract target = %q, want X402UptoPermit2ProxyAddress %q",
			mockSigner.lastWrite.Address, evm.X402UptoPermit2ProxyAddress)
	}
	if mockSigner.lastWrite.FunctionName != evm.FunctionSettle {
		t.Errorf("WriteContract function = %q, want %q",
			mockSigner.lastWrite.FunctionName, evm.FunctionSettle)
	}

	// Selector check: the ABI used must be the upto settle ABI. Compare the
	// computed 4-byte selector of "settle(permit, amount, owner, witness,
	// signature)" against the selector derived from
	// X402UptoPermit2ProxySettleABI.
	expectedSelector := uptoSettleSelector(t)
	gotSelector := abiFunctionSelector(t, mockSigner.lastWrite.ABI, evm.FunctionSettle)
	if !bytes.Equal(expectedSelector, gotSelector) {
		t.Errorf("settle selector mismatch: got %x, want %x", gotSelector, expectedSelector)
	}

	// Settlement amount arg = 2nd positional arg (after permit struct).
	if len(mockSigner.lastWrite.Args) < 5 {
		t.Fatalf("expected >=5 args, got %d", len(mockSigner.lastWrite.Args))
	}
	gotAmount, ok := mockSigner.lastWrite.Args[1].(*big.Int)
	if !ok {
		t.Fatalf("settle args[1] is %T, want *big.Int", mockSigner.lastWrite.Args[1])
	}
	if gotAmount.String() != testAmount {
		t.Errorf("settle amount = %s, want %s", gotAmount, testAmount)
	}
}

// SettlementOverrides.Amount path — emulated by passing a requirements struct
// whose Amount has been overridden to a smaller value than the signed permitted
// amount. (The resource server's SettlePayment applies the override before
// forwarding to the facilitator; this test exercises that the facilitator
// uses requirements.Amount as settlementAmount.)
func TestSettleUptoPermit2_HonorsSettlementOverrides(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	facilitatorAddr := clientSigner.Address()
	// Client signed permitted.amount = "1000000" (testAmount).
	clientReq := makeRequirementsFor(facilitatorAddr)
	payload := buildClientPayload(t, clientSigner, clientReq)
	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)

	// Server overrides settlement amount to 400_000 (less than signed cap).
	overrideReq := clientReq
	overrideReq.Amount = "400000"
	payload.Accepted.Amount = "400000"

	mockSigner := &mockFacilitatorSigner{
		addresses:   []string{facilitatorAddr},
		writeTxHash: testTxHash,
	}

	resp, err := SettleUptoPermit2(context.Background(), mockSigner, payload, overrideReq, uptoPayload, nil, noSettleSim)
	if err != nil {
		t.Fatalf("SettleUptoPermit2: %v", err)
	}
	if !resp.Success {
		t.Fatalf("Success = false, want true")
	}

	// Asset that WriteContract received the overridden amount, not the signed cap.
	if mockSigner.lastWrite == nil {
		t.Fatal("WriteContract was not called")
	}
	gotAmount, ok := mockSigner.lastWrite.Args[1].(*big.Int)
	if !ok {
		t.Fatalf("settle args[1] is %T, want *big.Int", mockSigner.lastWrite.Args[1])
	}
	if gotAmount.String() != "400000" {
		t.Errorf("settle amount = %s, want override 400000", gotAmount)
	}

	// Permit struct (args[0]) is a struct; the permitted.amount inside must
	// still be the signed cap (1000000).
	permitVal, ok := mockSigner.lastWrite.Args[0].(struct {
		Permitted struct {
			Token  common.Address
			Amount *big.Int
		}
		Nonce    *big.Int
		Deadline *big.Int
	})
	if !ok {
		t.Fatalf("settle args[0] type %T not the expected permit struct shape", mockSigner.lastWrite.Args[0])
	}
	if permitVal.Permitted.Amount.String() != testAmount {
		t.Errorf("permit.permitted.amount = %s, want %s", permitVal.Permitted.Amount, testAmount)
	}
}

// Settle with settlementAmount > permitted.amount → ErrUptoSettlementExceedsAmount.
func TestSettleUptoPermit2_RejectsSettlementAboveCap(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	facilitatorAddr := clientSigner.Address()
	clientReq := makeRequirementsFor(facilitatorAddr)
	payload := buildClientPayload(t, clientSigner, clientReq)
	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)

	tampered := clientReq
	tampered.Amount = "2000000" // > signed 1_000_000
	payload.Accepted.Amount = "2000000"

	mockSigner := &mockFacilitatorSigner{
		addresses: []string{facilitatorAddr},
	}

	_, err := SettleUptoPermit2(context.Background(), mockSigner, payload, tampered, uptoPayload, nil, noSettleSim)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !strings.Contains(err.Error(), ErrUptoSettlementExceedsAmount) {
		t.Errorf("error %q does not contain %q", err.Error(), ErrUptoSettlementExceedsAmount)
	}

	if mockSigner.lastWrite != nil {
		t.Error("WriteContract was called even though settle should reject pre-broadcast")
	}
}

// Settle revert with "AmountExceedsPermitted" → ErrUptoAmountExceedsPermitted.
func TestSettleUptoPermit2_RevertClassifiedAsAmountExceedsPermitted(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	facilitatorAddr := clientSigner.Address()
	requirements := makeRequirementsFor(facilitatorAddr)
	payload := buildClientPayload(t, clientSigner, requirements)
	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)

	mockSigner := &mockFacilitatorSigner{
		addresses: []string{facilitatorAddr},
		writeErr:  errors.New("execution reverted: AmountExceedsPermitted()"),
	}

	_, err := SettleUptoPermit2(context.Background(), mockSigner, payload, requirements, uptoPayload, nil, noSettleSim)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !strings.Contains(err.Error(), ErrUptoAmountExceedsPermitted) {
		t.Errorf("error %q does not contain %q", err.Error(), ErrUptoAmountExceedsPermitted)
	}
}

// Settle revert "transfer amount exceeds balance" → ErrPermit2InsufficientBalance.
func TestSettleUptoPermit2_RevertClassifiedAsInsufficientBalance(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	facilitatorAddr := clientSigner.Address()
	requirements := makeRequirementsFor(facilitatorAddr)
	payload := buildClientPayload(t, clientSigner, requirements)
	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)

	mockSigner := &mockFacilitatorSigner{
		addresses: []string{facilitatorAddr},
		writeErr:  errors.New("execution reverted: TRANSFER_FROM_FAILED"),
	}

	_, err := SettleUptoPermit2(context.Background(), mockSigner, payload, requirements, uptoPayload, nil, noSettleSim)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !strings.Contains(err.Error(), ErrPermit2InsufficientBalance) {
		t.Errorf("error %q does not contain %q", err.Error(), ErrPermit2InsufficientBalance)
	}
}

// Settle revert "UnauthorizedFacilitator" → ErrUptoUnauthorizedFacilitator.
func TestSettleUptoPermit2_RevertClassifiedAsUnauthorizedFacilitator(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	facilitatorAddr := clientSigner.Address()
	requirements := makeRequirementsFor(facilitatorAddr)
	payload := buildClientPayload(t, clientSigner, requirements)
	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)

	mockSigner := &mockFacilitatorSigner{
		addresses: []string{facilitatorAddr},
		writeErr:  errors.New("execution reverted: UnauthorizedFacilitator()"),
	}

	_, err := SettleUptoPermit2(context.Background(), mockSigner, payload, requirements, uptoPayload, nil, noSettleSim)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !strings.Contains(err.Error(), ErrUptoUnauthorizedFacilitator) {
		t.Errorf("error %q does not contain %q", err.Error(), ErrUptoUnauthorizedFacilitator)
	}
}

// Settle zero-amount → success with empty Transaction.
func TestSettleUptoPermit2_ZeroAmountNoBroadcast(t *testing.T) {
	clientSigner := newTestClientSigner(t)
	facilitatorAddr := clientSigner.Address()
	clientReq := makeRequirementsFor(facilitatorAddr)
	payload := buildClientPayload(t, clientSigner, clientReq)
	uptoPayload, _ := evm.UptoPermit2PayloadFromMap(payload.Payload)

	overrideReq := clientReq
	overrideReq.Amount = "0"
	payload.Accepted.Amount = "0"

	mockSigner := &mockFacilitatorSigner{addresses: []string{facilitatorAddr}}

	resp, err := SettleUptoPermit2(context.Background(), mockSigner, payload, overrideReq, uptoPayload, nil, noSettleSim)
	if err != nil {
		t.Fatalf("SettleUptoPermit2: %v", err)
	}
	if !resp.Success {
		t.Fatalf("Success = false, want true for zero-amount settle")
	}
	if resp.Transaction != "" {
		t.Errorf("Transaction = %q, want empty for zero-amount settle", resp.Transaction)
	}
	if mockSigner.lastWrite != nil {
		t.Error("WriteContract was called for zero-amount settle; expected no broadcast")
	}
}

// Settle / Verify route rejection: scheme.Verify and scheme.Settle reject
// payloads without `witness.facilitator`.
func TestUptoEvmScheme_RejectsNonUptoPayload(t *testing.T) {
	signer := &mockFacilitatorSigner{addresses: []string{"0x000000000000000000000000000000000000FaC1"}}
	scheme := NewUptoEvmScheme(signer, nil)

	exactPayload := types.PaymentPayload{
		X402Version: 2,
		Accepted: types.PaymentRequirements{
			Scheme:  evm.SchemeUpto,
			Network: testNetwork,
		},
		Payload: map[string]interface{}{
			"signature": "0x" + strings.Repeat("00", 65),
			"permit2Authorization": map[string]interface{}{
				"from":     "0x1234567890123456789012345678901234567890",
				"spender":  evm.X402ExactPermit2ProxyAddress,
				"nonce":    "1",
				"deadline": "9999999999",
				"permitted": map[string]interface{}{
					"token":  testTokenAddress,
					"amount": "1000000",
				},
				"witness": map[string]interface{}{
					"to":         testPayToAddress,
					"validAfter": "0",
					// missing "facilitator" → not an upto payload
				},
			},
		},
	}
	req := types.PaymentRequirements{Scheme: evm.SchemeUpto, Network: testNetwork}

	if _, err := scheme.Verify(context.Background(), exactPayload, req, nil); err == nil {
		t.Fatal("Verify: expected error for non-upto payload, got nil")
	} else if !strings.Contains(err.Error(), ErrUnsupportedPayloadType) {
		t.Errorf("Verify error %q does not contain %q", err.Error(), ErrUnsupportedPayloadType)
	}

	if _, err := scheme.Settle(context.Background(), exactPayload, req, nil); err == nil {
		t.Fatal("Settle: expected error for non-upto payload, got nil")
	} else if !strings.Contains(err.Error(), ErrUnsupportedPayloadType) {
		t.Errorf("Settle error %q does not contain %q", err.Error(), ErrUnsupportedPayloadType)
	}
}

// ---------------------------------------------------------------------------
// Verbatim TS sentinel parity (locks the wire contract).
// ---------------------------------------------------------------------------

func TestUptoFacilitatorErrorStrings_MatchTSVerbatim(t *testing.T) {
	cases := []struct {
		name string
		got  string
		want string
	}{
		{"ErrUptoInvalidScheme", ErrUptoInvalidScheme, "invalid_upto_evm_scheme"},
		{"ErrUptoNetworkMismatch", ErrUptoNetworkMismatch, "invalid_upto_evm_network_mismatch"},
		{"ErrUptoSettlementExceedsAmount", ErrUptoSettlementExceedsAmount, "invalid_upto_evm_payload_settlement_exceeds_amount"},
		{"ErrUptoAmountExceedsPermitted", ErrUptoAmountExceedsPermitted, "upto_amount_exceeds_permitted"},
		{"ErrUptoUnauthorizedFacilitator", ErrUptoUnauthorizedFacilitator, "upto_unauthorized_facilitator"},
		{"ErrUptoFacilitatorMismatch", ErrUptoFacilitatorMismatch, "upto_facilitator_mismatch"},
		{"ErrUnsupportedPayloadType", ErrUnsupportedPayloadType, "unsupported_payload_type"},
		{"ErrPermit2InvalidSpender", ErrPermit2InvalidSpender, "invalid_permit2_spender"},
		{"ErrPermit2DeadlineExpired", ErrPermit2DeadlineExpired, "permit2_deadline_expired"},
		{"ErrPermit2InvalidSignature", ErrPermit2InvalidSignature, "invalid_permit2_signature"},
	}
	for _, tc := range cases {
		if tc.got != tc.want {
			t.Errorf("%s = %q, want %q (TS canonical)", tc.name, tc.got, tc.want)
		}
	}
}

// ---------------------------------------------------------------------------
// Helpers (selector computation)
// ---------------------------------------------------------------------------

// abiFunctionSelector parses the supplied ABI JSON and returns the 4-byte
// selector of the named function.
func abiFunctionSelector(t *testing.T, abiJSON []byte, functionName string) []byte {
	t.Helper()
	parsed, err := abi.JSON(strings.NewReader(string(abiJSON)))
	if err != nil {
		t.Fatalf("abi.JSON: %v", err)
	}
	method, ok := parsed.Methods[functionName]
	if !ok {
		t.Fatalf("ABI has no %q method", functionName)
	}
	return method.ID
}

// uptoSettleSelector returns the canonical 4-byte selector for the upto
// settle(...) function by parsing X402UptoPermit2ProxySettleABI.
func uptoSettleSelector(t *testing.T) []byte {
	t.Helper()
	return abiFunctionSelector(t, evm.X402UptoPermit2ProxySettleABI, evm.FunctionSettle)
}
