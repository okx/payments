package client

import (
	"context"
	"crypto/ecdsa"
	"math/big"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/math"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/signer/core/apitypes"

	"github.com/okx/payments/go/x402"
	"github.com/okx/payments/go/x402/mechanisms/evm"
	"github.com/okx/payments/go/x402/types"
)

// ---------------------------------------------------------------------------
// Test signer — minimal ClientEvmSigner backed by an ECDSA private key.
// Mirrors signers/evm.ClientSigner but lives in-package to avoid the import
// cycle (signers/evm depends on mechanisms/evm).
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
	types map[string][]evm.TypedDataField,
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
	for typeName, fields := range types {
		out := make([]apitypes.Type, len(fields))
		for i, f := range fields {
			out[i] = apitypes.Type{Name: f.Name, Type: f.Type}
		}
		td.Types[typeName] = out
	}
	if _, ok := td.Types["EIP712Domain"]; !ok {
		// Permit2 domain has NO `version` — match the on-chain shape.
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
// Shared test helpers
// ---------------------------------------------------------------------------

const (
	testFacilitatorAddress = "0x000000000000000000000000000000000000FaC1"
	testPayToAddress       = "0x000000000000000000000000000000000000bEEF"
	testTokenAddress       = "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
	testNetwork            = "eip155:8453"
	testChainID            = int64(8453)
)

func newTestRequirements() types.PaymentRequirements {
	return types.PaymentRequirements{
		Scheme:            evm.SchemeUpto,
		Network:           testNetwork,
		Asset:             testTokenAddress,
		Amount:            "1000000",
		PayTo:             testPayToAddress,
		MaxTimeoutSeconds: 600,
		Extra: map[string]interface{}{
			ExtraFacilitatorAddressKey: testFacilitatorAddress,
		},
	}
}

// ---------------------------------------------------------------------------
// Scheme() returns evm.SchemeUpto
// ---------------------------------------------------------------------------

func TestUptoEvmScheme_Scheme(t *testing.T) {
	signer := newTestClientSigner(t)
	scheme := NewUptoEvmScheme(signer)

	if got := scheme.Scheme(); got != "upto" {
		t.Fatalf("scheme.Scheme() = %q, want %q", got, "upto")
	}
	if got := scheme.Scheme(); got != evm.SchemeUpto {
		t.Fatalf("scheme.Scheme() = %q, want evm.SchemeUpto = %q", got, evm.SchemeUpto)
	}
}

// Compile-time assertion that UptoEvmScheme satisfies x402.SchemeNetworkClient.
var _ x402.SchemeNetworkClient = (*UptoEvmScheme)(nil)

// ---------------------------------------------------------------------------
// Happy path — build payload, decode it, assert witness fields propagate
// ---------------------------------------------------------------------------

func TestUptoEvmScheme_CreatePaymentPayload_HappyPath(t *testing.T) {
	signer := newTestClientSigner(t)
	scheme := NewUptoEvmScheme(signer)
	req := newTestRequirements()

	payload, err := scheme.CreatePaymentPayload(context.Background(), req)
	if err != nil {
		t.Fatalf("CreatePaymentPayload: %v", err)
	}

	if payload.X402Version != 2 {
		t.Fatalf("payload.X402Version = %d, want 2", payload.X402Version)
	}
	if payload.Payload == nil {
		t.Fatal("payload.Payload is nil")
	}

	// The Payload map must round-trip via UptoPermit2PayloadFromMap.
	upto, err := evm.UptoPermit2PayloadFromMap(payload.Payload)
	if err != nil {
		t.Fatalf("UptoPermit2PayloadFromMap: %v", err)
	}

	// Discriminator check — IsUptoPermit2Payload must accept the wire bytes.
	if !evm.IsUptoPermit2Payload(payload.Payload) {
		t.Fatal("IsUptoPermit2Payload returned false for fresh upto payload")
	}

	// Signature must be a 0x-prefixed 65-byte hex string.
	if !strings.HasPrefix(upto.Signature, "0x") {
		t.Fatalf("signature missing 0x prefix: %q", upto.Signature)
	}
	if len(upto.Signature) != 2+65*2 {
		t.Fatalf("signature hex length = %d, want %d (65 bytes)", len(upto.Signature), 2+65*2)
	}

	auth := upto.Permit2Authorization
	if !strings.EqualFold(auth.From, signer.Address()) {
		t.Fatalf("auth.From = %q, want signer %q", auth.From, signer.Address())
	}
	if !strings.EqualFold(auth.Spender, evm.X402UptoPermit2ProxyAddress) {
		t.Fatalf("auth.Spender = %q, want X402UptoPermit2ProxyAddress %q",
			auth.Spender, evm.X402UptoPermit2ProxyAddress)
	}
	if auth.Permitted.Amount != req.Amount {
		t.Fatalf("auth.Permitted.Amount = %q, want %q", auth.Permitted.Amount, req.Amount)
	}
	if !strings.EqualFold(auth.Permitted.Token, req.Asset) {
		t.Fatalf("auth.Permitted.Token = %q, want %q", auth.Permitted.Token, req.Asset)
	}

	// Witness propagation — the three fields that the upto witness signs over.
	if !strings.EqualFold(auth.Witness.To, req.PayTo) {
		t.Fatalf("witness.To = %q, want %q", auth.Witness.To, req.PayTo)
	}
	if !strings.EqualFold(auth.Witness.Facilitator, testFacilitatorAddress) {
		t.Fatalf("witness.Facilitator = %q, want %q",
			auth.Witness.Facilitator, testFacilitatorAddress)
	}
	if auth.Witness.ValidAfter == "" {
		t.Fatal("witness.ValidAfter is empty")
	}

	// Nonce + deadline must be non-empty decimal strings.
	if _, ok := new(big.Int).SetString(auth.Nonce, 10); !ok || auth.Nonce == "" {
		t.Fatalf("auth.Nonce = %q is not a non-empty decimal string", auth.Nonce)
	}
	if _, ok := new(big.Int).SetString(auth.Deadline, 10); !ok || auth.Deadline == "" {
		t.Fatalf("auth.Deadline = %q is not a non-empty decimal string", auth.Deadline)
	}
}

// ---------------------------------------------------------------------------
// Signature roundtrip via evm.VerifyPermit2Witness — proves the signature
// produced by CreatePaymentPayload validates against the same authorization
// + Permit2 domain + chain ID.
// ---------------------------------------------------------------------------

func TestUptoEvmScheme_SignatureRoundtripsThroughVerifyPermit2Witness(t *testing.T) {
	signer := newTestClientSigner(t)
	scheme := NewUptoEvmScheme(signer)
	req := newTestRequirements()

	payload, err := scheme.CreatePaymentPayload(context.Background(), req)
	if err != nil {
		t.Fatalf("CreatePaymentPayload: %v", err)
	}

	upto, err := evm.UptoPermit2PayloadFromMap(payload.Payload)
	if err != nil {
		t.Fatalf("UptoPermit2PayloadFromMap: %v", err)
	}

	sigBytes, err := evm.HexToBytes(upto.Signature)
	if err != nil {
		t.Fatalf("HexToBytes(signature): %v", err)
	}
	if len(sigBytes) != 65 {
		t.Fatalf("sig length = %d, want 65", len(sigBytes))
	}

	ok, err := evm.VerifyPermit2Witness(
		signer.address,
		sigBytes,
		upto.Permit2Authorization,
		evm.DefaultPermit2Domain(),
		big.NewInt(testChainID),
	)
	if err != nil {
		t.Fatalf("VerifyPermit2Witness: %v", err)
	}
	if !ok {
		t.Fatal("VerifyPermit2Witness returned false for self-signed upto payload")
	}
}

// ---------------------------------------------------------------------------
// Missing facilitatorAddress in requirements.Extra → ErrMissingFacilitatorAddress
// ---------------------------------------------------------------------------

func TestUptoEvmScheme_MissingFacilitatorAddress(t *testing.T) {
	signer := newTestClientSigner(t)
	scheme := NewUptoEvmScheme(signer)

	cases := []struct {
		name  string
		extra map[string]interface{}
	}{
		{name: "nil extra", extra: nil},
		{name: "empty extra map", extra: map[string]interface{}{}},
		{name: "facilitatorAddress empty string", extra: map[string]interface{}{
			ExtraFacilitatorAddressKey: "",
		}},
		{name: "facilitatorAddress wrong type", extra: map[string]interface{}{
			ExtraFacilitatorAddressKey: 12345,
		}},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			req := newTestRequirements()
			req.Extra = tc.extra

			_, err := scheme.CreatePaymentPayload(context.Background(), req)
			if err == nil {
				t.Fatal("expected error, got nil")
			}
			if !strings.Contains(err.Error(), ErrMissingFacilitatorAddress) {
				t.Fatalf("error %q does not contain ErrMissingFacilitatorAddress %q",
					err.Error(), ErrMissingFacilitatorAddress)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Invalid time window (MaxTimeoutSeconds <= -600) — deadline must be > validAfter
// ---------------------------------------------------------------------------

func TestUptoEvmScheme_InvalidTimeWindow(t *testing.T) {
	signer := newTestClientSigner(t)
	scheme := NewUptoEvmScheme(signer)
	req := newTestRequirements()
	// validAfter = now - 600, deadline = now + MaxTimeoutSeconds.
	// Pick MaxTimeoutSeconds so that deadline <= validAfter (i.e. <= -600).
	req.MaxTimeoutSeconds = -700

	_, err := scheme.CreatePaymentPayload(context.Background(), req)
	if err == nil {
		t.Fatal("expected error for invalid time window, got nil")
	}
	if !strings.Contains(err.Error(), ErrInvalidTimeWindow) {
		t.Fatalf("error %q does not contain ErrInvalidTimeWindow %q",
			err.Error(), ErrInvalidTimeWindow)
	}
}

// ---------------------------------------------------------------------------
// Invalid network (bad eip155 prefix) — error surfaces from GetEvmChainId
// ---------------------------------------------------------------------------

func TestUptoEvmScheme_InvalidNetwork(t *testing.T) {
	signer := newTestClientSigner(t)
	scheme := NewUptoEvmScheme(signer)
	req := newTestRequirements()
	req.Network = "not-an-eip155-network"

	_, err := scheme.CreatePaymentPayload(context.Background(), req)
	if err == nil {
		t.Fatal("expected error for invalid network, got nil")
	}
}

// ---------------------------------------------------------------------------
// Spender field is hardcoded to evm.X402UptoPermit2ProxyAddress — independent
// of anything in requirements.Extra. (The TS upto client also hardcodes it.)
// ---------------------------------------------------------------------------

func TestUptoEvmScheme_SpenderIsUptoProxy(t *testing.T) {
	signer := newTestClientSigner(t)
	scheme := NewUptoEvmScheme(signer)
	req := newTestRequirements()

	payload, err := scheme.CreatePaymentPayload(context.Background(), req)
	if err != nil {
		t.Fatalf("CreatePaymentPayload: %v", err)
	}
	upto, err := evm.UptoPermit2PayloadFromMap(payload.Payload)
	if err != nil {
		t.Fatalf("UptoPermit2PayloadFromMap: %v", err)
	}

	if !strings.EqualFold(upto.Permit2Authorization.Spender, evm.X402UptoPermit2ProxyAddress) {
		t.Fatalf("auth.Spender = %q, want X402UptoPermit2ProxyAddress %q",
			upto.Permit2Authorization.Spender, evm.X402UptoPermit2ProxyAddress)
	}
}

// ---------------------------------------------------------------------------
// Address validation: CreatePaymentPayload must reject malformed asset / payTo
// / facilitator addresses BEFORE signing. Without this, garbage would be
// silently committed into the Permit2 witness as the wrong / zero address.
// ---------------------------------------------------------------------------

func TestUptoEvmScheme_CreatePaymentPayload_RejectsInvalidAddresses(t *testing.T) {
	signer := newTestClientSigner(t)
	scheme := NewUptoEvmScheme(signer)
	base := newTestRequirements()

	tests := []struct {
		name   string
		mutate func(r *types.PaymentRequirements)
	}{
		{
			name: "invalid asset",
			mutate: func(r *types.PaymentRequirements) {
				r.Asset = "0xZZZZ_not_hex"
			},
		},
		{
			name: "invalid payTo",
			mutate: func(r *types.PaymentRequirements) {
				r.PayTo = "not_an_address"
			},
		},
		{
			name: "invalid facilitator",
			mutate: func(r *types.PaymentRequirements) {
				r.Extra[ExtraFacilitatorAddressKey] = "0xnope"
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req := base
			req.Extra = map[string]interface{}{}
			for k, v := range base.Extra {
				req.Extra[k] = v
			}
			tt.mutate(&req)

			_, err := scheme.CreatePaymentPayload(context.Background(), req)
			if err == nil {
				t.Fatal("expected error for invalid address, got nil")
			}
			if !strings.Contains(err.Error(), ErrInvalidAddress) {
				t.Fatalf("error %q does not contain %q", err.Error(), ErrInvalidAddress)
			}
		})
	}
}
