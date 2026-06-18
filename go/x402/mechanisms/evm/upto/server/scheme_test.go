package server

import (
	"context"
	"crypto/ecdsa"
	"strings"
	"testing"
	"time"

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
// Test signer — minimal evm.ClientEvmSigner backed by an ECDSA private key.
// Mirrors the in-package signer used by mechanisms/evm/permit2_test.go and
// upto/client/scheme_test.go. Lives in-package to avoid pulling signers/evm,
// which depends on mechanisms/evm and would create an import cycle.
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
// Shared test fixtures
// ---------------------------------------------------------------------------

const (
	testFacilitatorAddress = "0x000000000000000000000000000000000000FaC1"
	testPayToAddress       = "0x000000000000000000000000000000000000bEEF"
	testTokenAddress       = "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
	testNetwork            = "eip155:8453"
)

// buildClientPayload uses the upto client (stage 4) to produce a wire-format
// PaymentPayload that ValidateUptoPayload should accept. Returns the payload
// with payload.Accepted populated so ValidateUptoPayload's network/scheme
// checks have data to compare against.
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
	// Populate accepted so ValidateUptoPayload's scheme/network checks can run.
	payload.Accepted = types.PaymentRequirements{
		Scheme:  requirements.Scheme,
		Network: requirements.Network,
		Asset:   requirements.Asset,
		Amount:  requirements.Amount,
		PayTo:   requirements.PayTo,
	}
	return payload
}

func makeRequirements(facilitatorAddr string) types.PaymentRequirements {
	return types.PaymentRequirements{
		Scheme:            evm.SchemeUpto,
		Network:           testNetwork,
		Asset:             testTokenAddress,
		Amount:            "1000000",
		PayTo:             testPayToAddress,
		MaxTimeoutSeconds: 600,
		Extra: map[string]interface{}{
			ExtraFacilitatorAddressKey: facilitatorAddr,
		},
	}
}

// ---------------------------------------------------------------------------
// Compile-time assertion: UptoEvmScheme satisfies x402.SchemeNetworkServer
// ---------------------------------------------------------------------------

var _ x402.SchemeNetworkServer = (*UptoEvmScheme)(nil)

// ---------------------------------------------------------------------------
// Scheme() returns evm.SchemeUpto
// ---------------------------------------------------------------------------

func TestUptoEvmScheme_Scheme(t *testing.T) {
	s := NewUptoEvmScheme()
	if got := s.Scheme(); got != "upto" {
		t.Fatalf("Scheme() = %q, want %q", got, "upto")
	}
	if got := s.Scheme(); got != evm.SchemeUpto {
		t.Fatalf("Scheme() = %q, want evm.SchemeUpto = %q", got, evm.SchemeUpto)
	}
}

// ---------------------------------------------------------------------------
// ParsePrice + EnhancePaymentRequirements — happy path
// ---------------------------------------------------------------------------

func TestUptoEvmScheme_ParsePrice_DefaultStampsPermit2(t *testing.T) {
	s := NewUptoEvmScheme()
	result, err := s.ParsePrice(1.0, x402.Network(testNetwork))
	if err != nil {
		t.Fatalf("ParsePrice: %v", err)
	}
	if result.Asset == "" {
		t.Fatal("ParsePrice returned empty Asset")
	}
	if got, _ := result.Extra[AssetTransferMethodKey].(string); got != "permit2" {
		t.Fatalf("extra.%s = %q, want %q", AssetTransferMethodKey, got, "permit2")
	}
}

func TestUptoEvmScheme_EnhancePaymentRequirements_PropagatesFacilitator(t *testing.T) {
	s := NewUptoEvmScheme()

	in := types.PaymentRequirements{
		Scheme:  evm.SchemeUpto,
		Network: testNetwork,
		Asset:   testTokenAddress,
		Amount:  "1000000",
		PayTo:   testPayToAddress,
	}
	supported := types.SupportedKind{
		Scheme:  evm.SchemeUpto,
		Network: testNetwork,
		Extra: map[string]interface{}{
			ExtraFacilitatorAddressKey: testFacilitatorAddress,
		},
	}
	out, err := s.EnhancePaymentRequirements(context.Background(), in, supported, nil)
	if err != nil {
		t.Fatalf("EnhancePaymentRequirements: %v", err)
	}
	if got, _ := out.Extra[AssetTransferMethodKey].(string); got != "permit2" {
		t.Fatalf("extra.%s = %q, want %q", AssetTransferMethodKey, got, "permit2")
	}
	gotFac, _ := out.Extra[ExtraFacilitatorAddressKey].(string)
	if !strings.EqualFold(gotFac, testFacilitatorAddress) {
		t.Fatalf("extra.%s = %q, want %q (case-insensitive)",
			ExtraFacilitatorAddressKey, gotFac, testFacilitatorAddress)
	}
}

func TestUptoEvmScheme_EnhancePaymentRequirements_NoFacilitatorWhenEmpty(t *testing.T) {
	s := NewUptoEvmScheme()
	in := types.PaymentRequirements{
		Scheme:  evm.SchemeUpto,
		Network: testNetwork,
		Asset:   testTokenAddress,
		Amount:  "1000000",
		PayTo:   testPayToAddress,
	}
	// supportedKind without facilitatorAddress — out.Extra must NOT include it.
	supported := types.SupportedKind{Scheme: evm.SchemeUpto, Network: testNetwork}
	out, err := s.EnhancePaymentRequirements(context.Background(), in, supported, nil)
	if err != nil {
		t.Fatalf("EnhancePaymentRequirements: %v", err)
	}
	if _, present := out.Extra[ExtraFacilitatorAddressKey]; present {
		t.Fatalf("extra.%s should be absent when supportedKind has no facilitator",
			ExtraFacilitatorAddressKey)
	}
	if got, _ := out.Extra[AssetTransferMethodKey].(string); got != "permit2" {
		t.Fatalf("extra.%s = %q, want %q", AssetTransferMethodKey, got, "permit2")
	}
}

// ---------------------------------------------------------------------------
// ValidateUptoPayload — happy path
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_HappyPath(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	// Use signer as the facilitator so the witness.facilitator matches.
	// (In production the facilitator is a different address; here we just
	// need a known address that the client wrote into the witness via
	// requirements.Extra.facilitatorAddress.)
	payload := buildClientPayload(t, signer, req)

	if err := ValidateUptoPayload(payload, req); err != nil {
		t.Fatalf("ValidateUptoPayload (happy path): %v", err)
	}
}

// ---------------------------------------------------------------------------
// Tampered amount > maxAmount → ErrUptoSettlementExceedsAmount
// The client signs `permitted.amount = req.Amount`. If the server then asks
// for a settlement amount larger than that, ValidateUptoPayload must reject.
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_TamperedAmountExceedsPermitted(t *testing.T) {
	signer := newTestClientSigner(t)
	clientReq := makeRequirements(signer.address.Hex())
	clientReq.Amount = "1000000"
	payload := buildClientPayload(t, signer, clientReq)

	// Server now bumps requirements.Amount above the signed permitted amount.
	tamperedReq := clientReq
	tamperedReq.Amount = "2000000"
	// Keep payload.Accepted in sync so the network/scheme check passes; the
	// amount we tamper is requirements.Amount itself.
	payload.Accepted.Amount = "2000000"

	err := ValidateUptoPayload(payload, tamperedReq)
	if err == nil {
		t.Fatal("expected error for tampered amount != permitted, got nil")
	}
	// ValidateUptoPayload enforces strict equality; req != permitted
	// returns permit2_amount_mismatch regardless of direction.
	if !strings.Contains(err.Error(), ErrPermit2AmountMismatch) {
		t.Fatalf("error %q does not contain ErrPermit2AmountMismatch %q",
			err.Error(), ErrPermit2AmountMismatch)
	}
}

// ---------------------------------------------------------------------------
// Wrong facilitator → ErrUptoFacilitatorMismatch
// Client signs witness.facilitator = X. Server's requirements.Extra advertises
// facilitator = Y. Validate must reject.
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_WrongFacilitator(t *testing.T) {
	signer := newTestClientSigner(t)

	clientReq := makeRequirements(testFacilitatorAddress) // client signs over FaC1
	payload := buildClientPayload(t, signer, clientReq)

	// Server expects a DIFFERENT facilitator.
	tamperedReq := clientReq
	tamperedReq.Extra = map[string]interface{}{
		ExtraFacilitatorAddressKey: "0x000000000000000000000000000000000000DEAD",
	}

	err := ValidateUptoPayload(payload, tamperedReq)
	if err == nil {
		t.Fatal("expected error for wrong facilitator, got nil")
	}
	if !strings.Contains(err.Error(), ErrUptoFacilitatorMismatch) {
		t.Fatalf("error %q does not contain ErrUptoFacilitatorMismatch %q",
			err.Error(), ErrUptoFacilitatorMismatch)
	}
}

// ---------------------------------------------------------------------------
// Signature from a different key → ErrPermit2InvalidSignature
// We forge a payload by swapping the signature with one produced over a
// different message (or simply by tampering a byte).
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_SignatureFromWrongKey(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	payload := buildClientPayload(t, signer, req)

	// Decode signature, flip a byte, re-encode.
	auth := payload.Payload["permit2Authorization"].(map[string]interface{})
	_ = auth // keep linter happy if not needed below
	sigHex := payload.Payload["signature"].(string)
	sigBytes, err := evm.HexToBytes(sigHex)
	if err != nil {
		t.Fatalf("HexToBytes: %v", err)
	}
	// Flip the first byte of r — recovery now produces a different (or
	// nonsense) address. VerifyPermit2Witness must return (false, nil) or
	// an error; either way ValidateUptoPayload must surface
	// ErrPermit2InvalidSignature.
	sigBytes[0] ^= 0xFF
	payload.Payload["signature"] = evm.BytesToHex(sigBytes)

	err = ValidateUptoPayload(payload, req)
	if err == nil {
		t.Fatal("expected error for tampered signature, got nil")
	}
	if !strings.Contains(err.Error(), ErrPermit2InvalidSignature) {
		t.Fatalf("error %q does not contain ErrPermit2InvalidSignature %q",
			err.Error(), ErrPermit2InvalidSignature)
	}
}

// ---------------------------------------------------------------------------
// Expired deadline → ErrPermit2DeadlineExpired
// The client builds with now+MaxTimeoutSeconds. We rewrite the payload's
// deadline to a past timestamp and verify the validator catches it.
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_ExpiredDeadline(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	payload := buildClientPayload(t, signer, req)

	auth := payload.Payload["permit2Authorization"].(map[string]interface{})
	// Set deadline to one hour in the past.
	auth["deadline"] = "1" // a tiny unix timestamp — definitely past
	// validAfter is in the past already; that's fine.

	// Note: this also breaks the signature, but the deadline check runs
	// BEFORE the signature check, so ErrPermit2DeadlineExpired is what
	// surfaces. That ordering is documented in validate.go.
	err := ValidateUptoPayload(payload, req)
	if err == nil {
		t.Fatal("expected error for expired deadline, got nil")
	}
	if !strings.Contains(err.Error(), ErrPermit2DeadlineExpired) {
		t.Fatalf("error %q does not contain ErrPermit2DeadlineExpired %q",
			err.Error(), ErrPermit2DeadlineExpired)
	}
}

// ---------------------------------------------------------------------------
// Missing facilitator field in payload witness → ErrUptoInvalidPayloadFormat
// (the structural guard IsUptoPermit2Payload rejects payloads whose
// witness.facilitator is missing or empty).
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_MissingFacilitatorInWitness(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	payload := buildClientPayload(t, signer, req)

	auth := payload.Payload["permit2Authorization"].(map[string]interface{})
	witness := auth["witness"].(map[string]interface{})
	// Remove facilitator from the witness — IsUptoPermit2Payload should now
	// return false.
	delete(witness, "facilitator")

	err := ValidateUptoPayload(payload, req)
	if err == nil {
		t.Fatal("expected error for missing witness.facilitator, got nil")
	}
	if !strings.Contains(err.Error(), ErrUptoInvalidPayloadFormat) {
		t.Fatalf("error %q does not contain ErrUptoInvalidPayloadFormat %q",
			err.Error(), ErrUptoInvalidPayloadFormat)
	}
}

// ---------------------------------------------------------------------------
// Missing facilitatorAddress in requirements.Extra → ErrUptoFacilitatorMismatch
// (validate fails closed when the server did not publish a facilitator).
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_MissingFacilitatorInRequirements(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	payload := buildClientPayload(t, signer, req)

	// Drop facilitator from requirements (server misconfigured).
	tamperedReq := req
	tamperedReq.Extra = map[string]interface{}{}

	err := ValidateUptoPayload(payload, tamperedReq)
	if err == nil {
		t.Fatal("expected error for missing requirements.facilitatorAddress, got nil")
	}
	if !strings.Contains(err.Error(), ErrUptoFacilitatorMismatch) {
		t.Fatalf("error %q does not contain ErrUptoFacilitatorMismatch %q",
			err.Error(), ErrUptoFacilitatorMismatch)
	}
}

// ---------------------------------------------------------------------------
// Scheme name not "upto" → ErrUptoInvalidScheme
// Both the payload.Accepted.Scheme branch and the requirements.Scheme branch
// must trigger the sentinel.
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_WrongSchemeOnPayload(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	payload := buildClientPayload(t, signer, req)
	payload.Accepted.Scheme = "exact"

	err := ValidateUptoPayload(payload, req)
	if err == nil {
		t.Fatal("expected error for wrong scheme on payload, got nil")
	}
	if !strings.Contains(err.Error(), ErrUptoInvalidScheme) {
		t.Fatalf("error %q does not contain ErrUptoInvalidScheme %q",
			err.Error(), ErrUptoInvalidScheme)
	}
}

func TestValidateUptoPayload_WrongSchemeOnRequirements(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	payload := buildClientPayload(t, signer, req)
	req.Scheme = "exact"

	err := ValidateUptoPayload(payload, req)
	if err == nil {
		t.Fatal("expected error for wrong scheme on requirements, got nil")
	}
	if !strings.Contains(err.Error(), ErrUptoInvalidScheme) {
		t.Fatalf("error %q does not contain ErrUptoInvalidScheme %q",
			err.Error(), ErrUptoInvalidScheme)
	}
}

// ---------------------------------------------------------------------------
// Network mismatch → ErrUptoNetworkMismatch
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_NetworkMismatch(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	payload := buildClientPayload(t, signer, req)
	// Server says network X, payload says network Y.
	payload.Accepted.Network = "eip155:1"

	err := ValidateUptoPayload(payload, req)
	if err == nil {
		t.Fatal("expected error for network mismatch, got nil")
	}
	if !strings.Contains(err.Error(), ErrUptoNetworkMismatch) {
		t.Fatalf("error %q does not contain ErrUptoNetworkMismatch %q",
			err.Error(), ErrUptoNetworkMismatch)
	}
}

// ---------------------------------------------------------------------------
// Wrong spender (tampered to non-X402UptoPermit2Proxy address) →
// ErrPermit2InvalidSpender
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_WrongSpender(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	payload := buildClientPayload(t, signer, req)

	auth := payload.Payload["permit2Authorization"].(map[string]interface{})
	auth["spender"] = "0x0000000000000000000000000000000000000001"

	err := ValidateUptoPayload(payload, req)
	if err == nil {
		t.Fatal("expected error for wrong spender, got nil")
	}
	if !strings.Contains(err.Error(), ErrPermit2InvalidSpender) {
		t.Fatalf("error %q does not contain ErrPermit2InvalidSpender %q",
			err.Error(), ErrPermit2InvalidSpender)
	}
}

// ---------------------------------------------------------------------------
// Wrong recipient (witness.to != requirements.PayTo) →
// ErrPermit2RecipientMismatch
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_WrongRecipient(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	payload := buildClientPayload(t, signer, req)

	// Server now insists on a different payTo.
	tamperedReq := req
	tamperedReq.PayTo = "0x0000000000000000000000000000000000000099"

	err := ValidateUptoPayload(payload, tamperedReq)
	if err == nil {
		t.Fatal("expected error for recipient mismatch, got nil")
	}
	if !strings.Contains(err.Error(), ErrPermit2RecipientMismatch) {
		t.Fatalf("error %q does not contain ErrPermit2RecipientMismatch %q",
			err.Error(), ErrPermit2RecipientMismatch)
	}
}

// ---------------------------------------------------------------------------
// Wrong asset (permitted.token != requirements.Asset) → ErrPermit2TokenMismatch
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_WrongAsset(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	payload := buildClientPayload(t, signer, req)

	tamperedReq := req
	tamperedReq.Asset = "0x000000000000000000000000000000000000ABcD"

	err := ValidateUptoPayload(payload, tamperedReq)
	if err == nil {
		t.Fatal("expected error for token mismatch, got nil")
	}
	if !strings.Contains(err.Error(), ErrPermit2TokenMismatch) {
		t.Fatalf("error %q does not contain ErrPermit2TokenMismatch %q",
			err.Error(), ErrPermit2TokenMismatch)
	}
}

// ---------------------------------------------------------------------------
// Smoke test: signature verification ordering. ValidateUptoPayload runs a
// real EOA verify against the upto Permit2 domain — the signature MUST
// recover to auth.From. The happy-path test already covers this; here we
// double-check that swapping auth.From to a different address (without
// re-signing) is rejected as a signature error.
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_FromAddressMismatch(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	payload := buildClientPayload(t, signer, req)

	auth := payload.Payload["permit2Authorization"].(map[string]interface{})
	// Rewrite auth.from to a different address. The signature still recovers
	// to the original signer, so the validator must report invalid signature.
	auth["from"] = "0x0000000000000000000000000000000000000123"

	err := ValidateUptoPayload(payload, req)
	if err == nil {
		t.Fatal("expected error for from-address mismatch, got nil")
	}
	if !strings.Contains(err.Error(), ErrPermit2InvalidSignature) {
		t.Fatalf("error %q does not contain ErrPermit2InvalidSignature %q",
			err.Error(), ErrPermit2InvalidSignature)
	}
}

// ---------------------------------------------------------------------------
// ErrorStringsMatchTS — sanity check that the verbatim sentinel strings
// are not accidentally mutated. These constants are part of the cross-SDK
// contract; renaming them silently is a wire-break.
// ---------------------------------------------------------------------------

func TestUptoServerErrorStrings_MatchTSVerbatim(t *testing.T) {
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
	}
	for _, tc := range cases {
		if tc.got != tc.want {
			t.Errorf("%s = %q, want %q", tc.name, tc.got, tc.want)
		}
	}
}

// ---------------------------------------------------------------------------
// Sanity: NormalizeAddress / equalAddress treat checksummed and lowercased
// addresses as equal. The client emits checksum-cased witness addresses; the
// server compares case-insensitively against requirements (which may be
// lowercase). Lock this down so future refactors don't drift.
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_AddressCaseInsensitive(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(strings.ToLower(signer.address.Hex()))
	payload := buildClientPayload(t, signer, req)

	// Re-uppercase the facilitator on requirements after building (the client
	// already wrote a normalized lowercased version into witness.facilitator).
	// equalAddress in validate.go must collapse the case difference.
	req.Extra[ExtraFacilitatorAddressKey] = strings.ToUpper(signer.address.Hex())

	if err := ValidateUptoPayload(payload, req); err != nil {
		t.Fatalf("ValidateUptoPayload (case-insensitive): %v", err)
	}
}

// ---------------------------------------------------------------------------
// Sanity: validateUptoPayload runs in <100ms (no RPC). Cheap test that
// ensures no goroutine leak / hang in the validate path.
// ---------------------------------------------------------------------------

func TestValidateUptoPayload_NoNetworkCalls(t *testing.T) {
	signer := newTestClientSigner(t)
	req := makeRequirements(signer.address.Hex())
	payload := buildClientPayload(t, signer, req)

	start := time.Now()
	if err := ValidateUptoPayload(payload, req); err != nil {
		t.Fatalf("ValidateUptoPayload: %v", err)
	}
	if elapsed := time.Since(start); elapsed > 100*time.Millisecond {
		t.Fatalf("ValidateUptoPayload took %v — likely doing network I/O", elapsed)
	}
}

// ---------------------------------------------------------------------------
// Precision: ParsePrice on an 18-decimal token (MegaETH USDM) must NOT lose
// sub-microcent digits via the old %.6f round-trip. A 12-decimal input value
// would previously have been truncated to "0.000000" → atomic 0.
// ---------------------------------------------------------------------------

func TestUptoEvmScheme_ParsePrice_PreservesPrecisionOn18DecimalToken(t *testing.T) {
	s := NewUptoEvmScheme()
	network := x402.Network("eip155:4326") // MegaETH USDM, 18 decimals, permit2

	// 0.000000000001 USDM = 10^-12 * 10^18 = 1_000_000 atomic units.
	result, err := s.ParsePrice("0.000000000001", network)
	if err != nil {
		t.Fatalf("ParsePrice: %v", err)
	}
	if result.Amount != "1000000" {
		t.Fatalf("amount = %q, want %q (old %%.6f path would yield %q)",
			result.Amount, "1000000", "0")
	}
}

func TestUptoEvmScheme_ParsePrice_PreservesNonRoundDecimalOn18DecimalToken(t *testing.T) {
	s := NewUptoEvmScheme()
	network := x402.Network("eip155:4326")

	// 1.234567890123456789 USDM = 1234567890123456789 atomic units.
	// Old %.6f path: "1.234568" → 1234568000000000000 (off by ~890 trillion).
	result, err := s.ParsePrice("1.234567890123456789", network)
	if err != nil {
		t.Fatalf("ParsePrice: %v", err)
	}
	if result.Amount != "1234567890123456789" {
		t.Fatalf("amount = %q, want %q", result.Amount, "1234567890123456789")
	}
}
