// Package upto_test contains end-to-end integration tests that exercise the
// upto scheme across all three roles — client, server, facilitator — wired
// together via the public x402 surface (x402Client / x402ResourceServer /
// x402Facilitator).
//
// The test deliberately avoids any real RPC: a mocked FacilitatorEvmSigner
// satisfies the facilitator's on-chain primitives (ReadContract, WriteContract,
// WaitForTransactionReceipt, GetBalance, GetChainID, GetCode, VerifyTypedData,
// SendTransaction), and an in-process FacilitatorClient wraps the x402Facilitator
// so the resource server can verify and settle without hitting a network endpoint.
//
// Goals:
//   - Confirm the three NewUptoEvmScheme factories register cleanly via the
//     generic Register() entry points on each x402 component.
//   - Confirm scheme="upto" payloads route through the upto package end-to-end
//     (the upto facilitator's WriteContract target is the upto Permit2 proxy).
//   - Confirm scheme="upto" coexists with scheme="exact" — a payload tagged
//     "exact" does NOT route through the upto facilitator (asserted via the
//     facilitator's rejection of non-upto payloads).
package upto_test

import (
	"bytes"
	"context"
	"crypto/ecdsa"
	"encoding/json"
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
	uptofac "github.com/okx/payments/go/x402/mechanisms/evm/upto/facilitator"
	uptoserver "github.com/okx/payments/go/x402/mechanisms/evm/upto/server"
	"github.com/okx/payments/go/x402/types"
)

// ---------------------------------------------------------------------------
// Compile-time confirmation that the upto factories satisfy the public x402
// scheme interfaces — the registry wiring depends on these.
// ---------------------------------------------------------------------------

var (
	_ x402.SchemeNetworkClient      = (*uptoclient.UptoEvmScheme)(nil)
	_ x402.SchemeNetworkServer      = (*uptoserver.UptoEvmScheme)(nil)
	_ x402.SchemeNetworkFacilitator = (*uptofac.UptoEvmScheme)(nil)
)

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

const (
	e2eNetwork      = "eip155:8453"
	e2ePayTo        = "0x000000000000000000000000000000000000bEEF"
	e2eTokenAddress = "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
	e2eAmount       = "1000000"
	e2eTxHash       = "0xe2ee2ee2ee2ee2ee2ee2ee2ee2ee2ee2ee2ee2ee2ee2ee2ee2ee2ee2ee2ee2ee"
)

// ---------------------------------------------------------------------------
// e2eClientSigner — ECDSA-backed ClientEvmSigner used by the upto client to
// sign the Permit2 witness. Identical shape to the in-package signers used by
// upto/client/scheme_test.go and upto/facilitator/scheme_test.go (kept local
// to avoid the import cycle that signers/evm carries).
// ---------------------------------------------------------------------------

type e2eClientSigner struct {
	privateKey *ecdsa.PrivateKey
	address    common.Address
}

func newE2EClientSigner(t *testing.T) *e2eClientSigner {
	t.Helper()
	pk, err := crypto.GenerateKey()
	if err != nil {
		t.Fatalf("generate key: %v", err)
	}
	return &e2eClientSigner{
		privateKey: pk,
		address:    crypto.PubkeyToAddress(pk.PublicKey),
	}
}

func (s *e2eClientSigner) Address() string { return s.address.Hex() }

func (s *e2eClientSigner) SignTypedData(
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
		// Permit2 domain has no version field.
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
// e2eFacilitatorSigner — controllable FacilitatorEvmSigner.
//
// Captures the last WriteContract call so the test can assert that scheme=upto
// payments are routed through the upto Permit2 proxy (not the exact one).
// ---------------------------------------------------------------------------

type capturedWrite struct {
	Address      string
	ABI          []byte
	FunctionName string
	Args         []interface{}
}

type e2eFacilitatorSigner struct {
	addresses []string
	chainID   *big.Int

	readContractFn func(address string, abi []byte, functionName string, args ...interface{}) (interface{}, error)

	lastWrite *capturedWrite
	writeErr  error
}

func (m *e2eFacilitatorSigner) GetAddresses() []string {
	if m.addresses == nil {
		return []string{}
	}
	return m.addresses
}

func (m *e2eFacilitatorSigner) ReadContract(
	ctx context.Context,
	address string,
	abi []byte,
	functionName string,
	args ...interface{},
) (interface{}, error) {
	if m.readContractFn != nil {
		return m.readContractFn(address, abi, functionName, args...)
	}
	return nil, fmt.Errorf("e2eFacilitatorSigner: ReadContract %s on %s not configured", functionName, address)
}

func (m *e2eFacilitatorSigner) VerifyTypedData(
	ctx context.Context,
	address string,
	domain evm.TypedDataDomain,
	dataTypes map[string][]evm.TypedDataField,
	primaryType string,
	message map[string]interface{},
	signature []byte,
) (bool, error) {
	// The off-chain validator does EOA recovery itself; this is only reached
	// for ERC-1271 fallback paths, which the e2e test does not exercise.
	return false, nil
}

func (m *e2eFacilitatorSigner) WriteContract(
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
	return e2eTxHash, nil
}

func (m *e2eFacilitatorSigner) SendTransaction(ctx context.Context, to string, data []byte) (string, error) {
	return e2eTxHash, nil
}

func (m *e2eFacilitatorSigner) WaitForTransactionReceipt(ctx context.Context, txHash string) (*evm.TransactionReceipt, error) {
	return &evm.TransactionReceipt{Status: evm.TxStatusSuccess, BlockNumber: 1, TxHash: txHash}, nil
}

func (m *e2eFacilitatorSigner) GetBalance(ctx context.Context, address, tokenAddress string) (*big.Int, error) {
	return big.NewInt(1_000_000_000_000), nil
}

func (m *e2eFacilitatorSigner) GetChainID(ctx context.Context) (*big.Int, error) {
	if m.chainID == nil {
		return big.NewInt(8453), nil
	}
	return m.chainID, nil
}

func (m *e2eFacilitatorSigner) GetCode(ctx context.Context, address string) ([]byte, error) {
	// EOA — empty code.
	return nil, nil
}

// ---------------------------------------------------------------------------
// Multicall fixtures — replicates evm.Multicall.tryAggregate return shape.
// ---------------------------------------------------------------------------

type tryAggregateEntry struct {
	Success    bool
	ReturnData []byte
}

func packUint256(t *testing.T, v *big.Int) []byte {
	t.Helper()
	ty, err := abi.NewType("uint256", "", nil)
	if err != nil {
		t.Fatalf("abi.NewType uint256: %v", err)
	}
	args := abi.Arguments{{Type: ty}}
	data, err := args.Pack(v)
	if err != nil {
		t.Fatalf("pack uint256: %v", err)
	}
	return data
}

func packAddress(t *testing.T, a common.Address) []byte {
	t.Helper()
	ty, err := abi.NewType("address", "", nil)
	if err != nil {
		t.Fatalf("abi.NewType address: %v", err)
	}
	args := abi.Arguments{{Type: ty}}
	data, err := args.Pack(a)
	if err != nil {
		t.Fatalf("pack address: %v", err)
	}
	return data
}

// happyPathReadContractFn services every ReadContract / settle simulation the
// facilitator issues during a green verify+settle flow.
func happyPathReadContractFn(t *testing.T) func(string, []byte, string, ...interface{}) (interface{}, error) {
	t.Helper()
	return func(address string, _ []byte, functionName string, args ...interface{}) (interface{}, error) {
		switch functionName {
		case "settle", "settleWithPermit":
			// Simulation OK — no return data needed for a successful simulate.
			return nil, nil
		case "tryAggregate":
			// Multicall3.tryAggregate(allowFailure, calls[])
			// Order matches DiagnoseUptoPermit2SimulationFailure's call list:
			// 1) PERMIT2() probe on the upto proxy
			// 2) balanceOf(payer)
			// 3) allowance(payer, PERMIT2)
			return []tryAggregateEntry{
				{Success: true, ReturnData: packAddress(t, common.HexToAddress(evm.PERMIT2Address))},
				{Success: true, ReturnData: packUint256(t, big.NewInt(1_000_000_000_000))},
				{Success: true, ReturnData: packUint256(t, big.NewInt(1_000_000_000_000))},
			}, nil
		}
		return nil, fmt.Errorf("happyPathReadContractFn: unhandled ReadContract %s on %s", functionName, address)
	}
}

// ---------------------------------------------------------------------------
// inProcessFacilitatorClient — adapts x402.X402Facilitator to the
// FacilitatorClient interface the resource server expects. Mirrors the same
// helper pattern used in go/x402/test/integration/evm_test.go for the exact
// scheme.
// ---------------------------------------------------------------------------

type inProcessFacilitatorClient struct {
	facilitator *x402.X402Facilitator
}

func (l *inProcessFacilitatorClient) Verify(
	ctx context.Context,
	payloadBytes []byte,
	requirementsBytes []byte,
) (*x402.VerifyResponse, error) {
	return l.facilitator.Verify(ctx, payloadBytes, requirementsBytes)
}

func (l *inProcessFacilitatorClient) Settle(
	ctx context.Context,
	payloadBytes []byte,
	requirementsBytes []byte,
) (*x402.SettleResponse, error) {
	return l.facilitator.Settle(ctx, payloadBytes, requirementsBytes)
}

func (l *inProcessFacilitatorClient) GetSupported(ctx context.Context) (x402.SupportedResponse, error) {
	return l.facilitator.GetSupported(), nil
}

// ---------------------------------------------------------------------------
// e2eSetup builds a fully wired (client, server, facilitator) triple where the
// upto scheme is registered on every component via the public Register() APIs.
//
// Returns the three components plus the facilitator signer (for assertions on
// captured WriteContract calls).
// ---------------------------------------------------------------------------

type e2eSetup struct {
	client            *x402.X402Client
	server            *x402.X402ResourceServer
	facilitator       *x402.X402Facilitator
	facilitatorSigner *e2eFacilitatorSigner
	clientSigner      *e2eClientSigner
}

func buildE2ESetup(t *testing.T) *e2eSetup {
	t.Helper()
	ctx := context.Background()

	clientSigner := newE2EClientSigner(t)
	facilitatorSigner := &e2eFacilitatorSigner{
		addresses:      []string{clientSigner.Address()},
		chainID:        big.NewInt(8453),
		readContractFn: happyPathReadContractFn(t),
	}

	// --- Facilitator wiring (registers via the public Register() API).
	facilitator := x402.Newx402Facilitator()
	facilitator.Register(
		[]x402.Network{e2eNetwork},
		uptofac.NewUptoEvmScheme(facilitatorSigner, nil),
	)

	// --- Client wiring (registers via the public Register() API).
	client := x402.Newx402Client()
	client.Register(e2eNetwork, uptoclient.NewUptoEvmScheme(clientSigner))

	// --- Server wiring (registers via the public Register() API), bridged to
	// the facilitator through the in-process FacilitatorClient.
	server := x402.Newx402ResourceServer(
		x402.WithFacilitatorClient(&inProcessFacilitatorClient{facilitator: facilitator}),
	)
	server.Register(e2eNetwork, uptoserver.NewUptoEvmScheme())

	if err := server.Initialize(ctx); err != nil {
		t.Fatalf("server.Initialize: %v", err)
	}

	return &e2eSetup{
		client:            client,
		server:            server,
		facilitator:       facilitator,
		facilitatorSigner: facilitatorSigner,
		clientSigner:      clientSigner,
	}
}

// buildE2ERequirements builds payment requirements for the upto scheme via the
// resource server's BuildPaymentRequirements (driven by ResourceConfig). The
// server reads supported kinds from the facilitator (populated via Initialize)
// and propagates extra.facilitatorAddress into the requirements.
func buildE2ERequirements(t *testing.T, s *e2eSetup) types.PaymentRequirements {
	t.Helper()
	ctx := context.Background()

	reqs, err := s.server.BuildPaymentRequirementsFromConfig(ctx, x402.ResourceConfig{
		Scheme:  evm.SchemeUpto,
		Network: e2eNetwork,
		PayTo:   e2ePayTo,
		Price: map[string]interface{}{
			"amount": e2eAmount,
			"asset":  e2eTokenAddress,
		},
		MaxTimeoutSeconds: 600,
	})
	if err != nil {
		t.Fatalf("BuildPaymentRequirementsFromConfig: %v", err)
	}
	if len(reqs) != 1 {
		t.Fatalf("BuildPaymentRequirementsFromConfig returned %d requirements, want 1", len(reqs))
	}

	req := reqs[0]

	// Server must have stamped the upto-specific extras.
	if got, _ := req.Extra["assetTransferMethod"].(string); got != "permit2" {
		t.Fatalf("requirements.Extra.assetTransferMethod = %q, want %q", got, "permit2")
	}
	if got, _ := req.Extra["facilitatorAddress"].(string); got == "" {
		t.Fatal("requirements.Extra.facilitatorAddress is empty — server failed to propagate from facilitator.GetExtra")
	}

	return req
}

// ---------------------------------------------------------------------------
// TestUptoE2E_FullFlow_ClientServerFacilitator — the canonical end-to-end test.
//
// 1. Wire all three NewUptoEvmScheme factories into x402Client /
//    x402ResourceServer / X402Facilitator via the generic Register() APIs.
// 2. Buyer flow: build payment payload via the client.
// 3. Seller flow: ResourceServer.VerifyPayment routes scheme=upto correctly
//    and reports IsValid=true.
// 4. Facilitator flow: ResourceServer.SettlePayment drives the registered upto
//    facilitator scheme; assert the captured WriteContract target equals the
//    upto Permit2 proxy address (the load-bearing routing assertion).
// ---------------------------------------------------------------------------

func TestUptoE2E_FullFlow_ClientServerFacilitator(t *testing.T) {
	setup := buildE2ESetup(t)
	ctx := context.Background()

	// --- 1. Requirements via the server (also confirms the upto server scheme
	//        is registered and that GetSupported flows through the facilitator).
	requirements := buildE2ERequirements(t, setup)
	if requirements.Scheme != evm.SchemeUpto {
		t.Fatalf("requirements.Scheme = %q, want %q", requirements.Scheme, evm.SchemeUpto)
	}

	// --- 2. Client builds the payment payload (signs the upto Permit2 witness).
	payload, err := setup.client.CreatePaymentPayload(ctx, requirements, nil, nil)
	if err != nil {
		t.Fatalf("client.CreatePaymentPayload: %v", err)
	}
	if payload.X402Version != 2 {
		t.Fatalf("payload.X402Version = %d, want 2", payload.X402Version)
	}
	if payload.Accepted.Scheme != evm.SchemeUpto {
		t.Fatalf("payload.Accepted.Scheme = %q, want %q", payload.Accepted.Scheme, evm.SchemeUpto)
	}

	// Payload must be an upto Permit2 payload (witness.facilitator present).
	if !evm.IsUptoPermit2Payload(payload.Payload) {
		t.Fatal("payload.Payload is not an upto Permit2 payload — client wiring is broken")
	}

	uptoPayload, err := evm.UptoPermit2PayloadFromMap(payload.Payload)
	if err != nil {
		t.Fatalf("UptoPermit2PayloadFromMap: %v", err)
	}
	if uptoPayload.Permit2Authorization.Witness.Facilitator == "" {
		t.Fatal("witness.facilitator is empty — client did not bind the facilitator address")
	}
	if !strings.EqualFold(uptoPayload.Permit2Authorization.Witness.To, e2ePayTo) {
		t.Fatalf("witness.to = %q, want %q", uptoPayload.Permit2Authorization.Witness.To, e2ePayTo)
	}
	if uptoPayload.Permit2Authorization.Spender != evm.X402UptoPermit2ProxyAddress {
		t.Fatalf("spender = %q, want X402UptoPermit2ProxyAddress %q",
			uptoPayload.Permit2Authorization.Spender, evm.X402UptoPermit2ProxyAddress)
	}

	// --- 3. Seller verifies via the resource server (routes via in-process
	//        facilitator client → x402Facilitator → registered upto facilitator
	//        scheme — which Verify-routes upto payloads to VerifyUptoPermit2).
	verifyResp, err := setup.server.VerifyPayment(ctx, payload, requirements)
	if err != nil {
		t.Fatalf("server.VerifyPayment: %v", err)
	}
	if !verifyResp.IsValid {
		t.Fatalf("VerifyPayment IsValid=false, reason=%q message=%q",
			verifyResp.InvalidReason, verifyResp.InvalidMessage)
	}
	if !strings.EqualFold(verifyResp.Payer, setup.clientSigner.Address()) {
		t.Fatalf("VerifyPayment Payer = %q, want %q", verifyResp.Payer, setup.clientSigner.Address())
	}

	// --- 4. Settle via the resource server. Confirms the facilitator's
	//        WriteContract is routed against the upto Permit2 proxy address.
	settleResp, err := setup.server.SettlePayment(ctx, payload, requirements, nil)
	if err != nil {
		t.Fatalf("server.SettlePayment: %v", err)
	}
	if !settleResp.Success {
		t.Fatalf("SettlePayment Success=false, reason=%q message=%q",
			settleResp.ErrorReason, settleResp.ErrorMessage)
	}
	if settleResp.Transaction != e2eTxHash {
		t.Fatalf("SettlePayment Transaction = %q, want %q", settleResp.Transaction, e2eTxHash)
	}

	// Routing assertion: the captured WriteContract MUST hit the upto Permit2
	// proxy address — proves scheme=upto landed on the upto facilitator, not
	// the exact one.
	lw := setup.facilitatorSigner.lastWrite
	if lw == nil {
		t.Fatal("facilitator signer captured no WriteContract call")
	}
	if !strings.EqualFold(lw.Address, evm.X402UptoPermit2ProxyAddress) {
		t.Fatalf("WriteContract target = %q, want X402UptoPermit2ProxyAddress %q",
			lw.Address, evm.X402UptoPermit2ProxyAddress)
	}
	if lw.FunctionName != evm.FunctionSettle {
		t.Fatalf("WriteContract functionName = %q, want %q", lw.FunctionName, evm.FunctionSettle)
	}

	// Selector parity — the captured ABI must expose the same `settle` 4-byte
	// selector as evm.X402UptoPermit2ProxySettleABI. If a future change drifts
	// the upto settle signature, this comparison breaks.
	expectedSel := abiFunctionSelector(t, evm.X402UptoPermit2ProxySettleABI, evm.FunctionSettle)
	actualSel := abiFunctionSelector(t, lw.ABI, evm.FunctionSettle)
	if !bytes.Equal(expectedSel, actualSel) {
		t.Fatalf("settle selector mismatch: got %x, want %x", actualSel, expectedSel)
	}
}

// ---------------------------------------------------------------------------
// TestUptoE2E_ExactPayloadDoesNotRouteThroughUpto — guards against scheme
// crosstalk. An exact-scheme payload submitted to the facilitator with only
// the upto scheme registered must be rejected via NoFacilitatorForNetwork (no
// exact facilitator is registered) — proving that scheme="upto" routing is
// scheme-name discriminated, not just network-level.
// ---------------------------------------------------------------------------

func TestUptoE2E_ExactPayloadDoesNotRouteThroughUpto(t *testing.T) {
	setup := buildE2ESetup(t)
	ctx := context.Background()

	// Synthesize an exact-scheme payload by mutating the wire schema of a real
	// upto payload (we never put it on the wire — we just need a
	// types.PaymentPayload with Accepted.Scheme = "exact"). The facilitator
	// router selects by payload.Accepted.Scheme (via requirements.Scheme), so
	// this is sufficient to exercise the cross-scheme rejection path.
	requirements := buildE2ERequirements(t, setup)
	uptoPayload, err := setup.client.CreatePaymentPayload(ctx, requirements, nil, nil)
	if err != nil {
		t.Fatalf("client.CreatePaymentPayload: %v", err)
	}

	// Pretend the payload is from the exact scheme.
	exactRequirements := requirements
	exactRequirements.Scheme = evm.SchemeExact

	exactPayload := uptoPayload
	exactPayload.Accepted = types.PaymentRequirements{
		Scheme:  evm.SchemeExact,
		Network: exactRequirements.Network,
		Asset:   exactRequirements.Asset,
		Amount:  exactRequirements.Amount,
		PayTo:   exactRequirements.PayTo,
	}

	// VerifyPayment routes to the facilitator client — but the server only has
	// the upto facilitator registered, so the resource server itself fails
	// fast at the "no facilitator client for scheme/network" check (which only
	// has upto in its map after Initialize).
	resp, err := setup.server.VerifyPayment(ctx, exactPayload, exactRequirements)
	if err == nil {
		t.Fatalf("expected error rejecting exact payload (only upto registered), got resp=%+v", resp)
	}

	// The rejection message must reference the scheme — confirms the router
	// considered scheme=exact and could not route it.
	if !strings.Contains(err.Error(), evm.SchemeExact) && !strings.Contains(err.Error(), "facilitator") {
		t.Fatalf("error message does not mention scheme/facilitator routing: %v", err)
	}
}

// ---------------------------------------------------------------------------
// TestUptoE2E_FacilitatorBytesRoundtrip — exercise the facilitator's Verify
// path via its byte-oriented Network Boundary (the FacilitatorClient interface
// the resource server uses). Confirms the upto facilitator scheme is
// reachable via raw JSON in addition to the typed Go API.
// ---------------------------------------------------------------------------

func TestUptoE2E_FacilitatorBytesRoundtrip(t *testing.T) {
	setup := buildE2ESetup(t)
	ctx := context.Background()

	requirements := buildE2ERequirements(t, setup)
	payload, err := setup.client.CreatePaymentPayload(ctx, requirements, nil, nil)
	if err != nil {
		t.Fatalf("client.CreatePaymentPayload: %v", err)
	}

	payloadBytes, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	reqBytes, err := json.Marshal(requirements)
	if err != nil {
		t.Fatalf("marshal requirements: %v", err)
	}

	verifyResp, err := setup.facilitator.Verify(ctx, payloadBytes, reqBytes)
	if err != nil {
		t.Fatalf("facilitator.Verify (bytes): %v", err)
	}
	if !verifyResp.IsValid {
		t.Fatalf("facilitator.Verify IsValid=false, reason=%q", verifyResp.InvalidReason)
	}

	settleResp, err := setup.facilitator.Settle(ctx, payloadBytes, reqBytes)
	if err != nil {
		t.Fatalf("facilitator.Settle (bytes): %v", err)
	}
	if !settleResp.Success {
		t.Fatalf("facilitator.Settle Success=false, reason=%q", settleResp.ErrorReason)
	}
	if settleResp.Transaction != e2eTxHash {
		t.Fatalf("facilitator.Settle Transaction = %q, want %q", settleResp.Transaction, e2eTxHash)
	}
}

// ---------------------------------------------------------------------------
// TestUptoE2E_GetSupportedReportsUptoScheme — confirm the facilitator advertises
// the upto scheme + the facilitatorAddress extra under GetSupported. This is
// the upstream mechanism that lets the server propagate extra.facilitatorAddress
// into the requirements (so the client can sign the witness with it).
// ---------------------------------------------------------------------------

func TestUptoE2E_GetSupportedReportsUptoScheme(t *testing.T) {
	setup := buildE2ESetup(t)

	supported := setup.facilitator.GetSupported()
	var foundUpto bool
	for _, kind := range supported.Kinds {
		if kind.Scheme == evm.SchemeUpto && string(kind.Network) == e2eNetwork {
			foundUpto = true
			facAddr, _ := kind.Extra["facilitatorAddress"].(string)
			if facAddr == "" {
				t.Errorf("GetSupported upto kind has no facilitatorAddress in extra: %+v", kind.Extra)
			}
		}
	}
	if !foundUpto {
		t.Fatalf("GetSupported did not advertise upto on %s: kinds=%+v", e2eNetwork, supported.Kinds)
	}
}

// ---------------------------------------------------------------------------
// abiFunctionSelector parses an ABI JSON blob and returns the 4-byte selector
// for the named function. Used to confirm structural ABI parity between the
// upto facilitator's WriteContract call and the canonical upto settle ABI.
// ---------------------------------------------------------------------------

func abiFunctionSelector(t *testing.T, abiJSON []byte, functionName string) []byte {
	t.Helper()
	parsed, err := abi.JSON(strings.NewReader(string(abiJSON)))
	if err != nil {
		t.Fatalf("abi.JSON: %v", err)
	}
	method, ok := parsed.Methods[functionName]
	if !ok {
		t.Fatalf("function %q not found in ABI", functionName)
	}
	return method.ID
}
