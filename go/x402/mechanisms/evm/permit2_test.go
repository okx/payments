package evm

import (
	"context"
	"crypto/ecdsa"
	"encoding/hex"
	"math/big"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/math"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/signer/core/apitypes"
)

// ---------------------------------------------------------------------------
// Test signer — minimal ClientEvmSigner backed by an ECDSA private key.
//
// Mirrors signers/evm.ClientSigner but lives in-package to avoid the import
// cycle that would result if the test imported the higher-level signer
// package (signers/evm depends on mechanisms/evm, not the other way around).
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

func newTestClientSignerFromHex(t *testing.T, hexKey string) *testClientSigner {
	t.Helper()
	pk, err := crypto.HexToECDSA(strings.TrimPrefix(hexKey, "0x"))
	if err != nil {
		t.Fatalf("parse hex key: %v", err)
	}
	return &testClientSigner{
		privateKey: pk,
		address:    crypto.PubkeyToAddress(pk.PublicKey),
	}
}

func (s *testClientSigner) Address() string { return s.address.Hex() }

func (s *testClientSigner) SignTypedData(
	ctx context.Context,
	domain TypedDataDomain,
	types map[string][]TypedDataField,
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
			{Name: "version", Type: "string"},
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
// Roundtrip — Exact Permit2 witness
// ---------------------------------------------------------------------------

func TestSignAndVerifyPermit2Witness_Exact_Roundtrip(t *testing.T) {
	signer := newTestClientSigner(t)
	chainID := big.NewInt(8453)

	auth := Permit2Authorization{
		From: signer.Address(),
		Permitted: Permit2TokenPermissions{
			Token:  "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
			Amount: "1000000",
		},
		Spender:  X402ExactPermit2ProxyAddress,
		Nonce:    "1234567890",
		Deadline: "1700000600",
		Witness: Permit2Witness{
			To:         "0x000000000000000000000000000000000000bEEF",
			ValidAfter: "1700000000",
		},
	}

	sig, hash, err := SignPermit2Witness(context.Background(), signer, auth, DefaultPermit2Domain(), chainID)
	if err != nil {
		t.Fatalf("SignPermit2Witness: %v", err)
	}
	if len(sig) != 65 {
		t.Fatalf("expected 65-byte signature, got %d", len(sig))
	}
	if len(hash) != 32 {
		t.Fatalf("expected 32-byte hash, got %d", len(hash))
	}

	ok, err := VerifyPermit2Witness(signer.address, sig, auth, DefaultPermit2Domain(), chainID)
	if err != nil {
		t.Fatalf("VerifyPermit2Witness: %v", err)
	}
	if !ok {
		t.Fatal("VerifyPermit2Witness returned false for self-signed witness")
	}
}

// ---------------------------------------------------------------------------
// Roundtrip — Upto Permit2 witness (includes facilitator field)
// ---------------------------------------------------------------------------

func TestSignAndVerifyPermit2Witness_Upto_Roundtrip(t *testing.T) {
	signer := newTestClientSigner(t)
	chainID := big.NewInt(8453)

	auth := UptoPermit2Authorization{
		From: signer.Address(),
		Permitted: Permit2TokenPermissions{
			Token:  "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
			Amount: "2500000",
		},
		Spender:  X402UptoPermit2ProxyAddress,
		Nonce:    "9876543210",
		Deadline: "1800000600",
		Witness: UptoPermit2Witness{
			To:          "0x000000000000000000000000000000000000bEEF",
			Facilitator: "0x000000000000000000000000000000000000FaC1",
			ValidAfter:  "1800000000",
		},
	}

	sig, hash, err := SignPermit2Witness(context.Background(), signer, auth, DefaultPermit2Domain(), chainID)
	if err != nil {
		t.Fatalf("SignPermit2Witness (upto): %v", err)
	}
	if len(sig) != 65 {
		t.Fatalf("expected 65-byte signature, got %d", len(sig))
	}
	if len(hash) != 32 {
		t.Fatalf("expected 32-byte hash, got %d", len(hash))
	}

	ok, err := VerifyPermit2Witness(signer.address, sig, auth, DefaultPermit2Domain(), chainID)
	if err != nil {
		t.Fatalf("VerifyPermit2Witness (upto): %v", err)
	}
	if !ok {
		t.Fatal("VerifyPermit2Witness (upto) returned false for self-signed witness")
	}
}

// TestBuildPermit2TypedData_RejectsInvalidInput verifies the shared builders
// reject malformed addresses and negative uint256 fields before signing —
// matching TS (viem getAddress / uint range) and Rust (Address / U256).
func TestBuildPermit2TypedData_RejectsInvalidInput(t *testing.T) {
	chainID := big.NewInt(196)
	valid := func() UptoPermit2Authorization {
		return UptoPermit2Authorization{
			From:      "0x000000000000000000000000000000000000bEEF",
			Permitted: Permit2TokenPermissions{Token: "0x036CbD53842c5426634e7929541eC2318f3dCF7e", Amount: "2500000"},
			Spender:   X402UptoPermit2ProxyAddress,
			Nonce:     "1",
			Deadline:  "1800000600",
			Witness:   UptoPermit2Witness{To: "0x000000000000000000000000000000000000bEEF", Facilitator: "0x000000000000000000000000000000000000FaC1", ValidAfter: "0"},
		}
	}

	cases := []struct {
		name   string
		mutate func(a *UptoPermit2Authorization)
	}{
		{"malformed token address", func(a *UptoPermit2Authorization) { a.Permitted.Token = "not-an-address" }},
		{"short token address", func(a *UptoPermit2Authorization) { a.Permitted.Token = "0x1234" }},
		{"malformed facilitator", func(a *UptoPermit2Authorization) { a.Witness.Facilitator = "0xZZZ" }},
		{"negative amount", func(a *UptoPermit2Authorization) { a.Permitted.Amount = "-1" }},
		{"negative nonce", func(a *UptoPermit2Authorization) { a.Nonce = "-5" }},
		{"negative validAfter", func(a *UptoPermit2Authorization) { a.Witness.ValidAfter = "-1" }},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			a := valid()
			tc.mutate(&a)
			if _, err := BuildPermit2TypedData(a, DefaultPermit2Domain(), chainID); err == nil {
				t.Fatalf("expected error for %s, got nil", tc.name)
			}
		})
	}

	// Sanity: the unmutated fixture still builds.
	if _, err := BuildPermit2TypedData(valid(), DefaultPermit2Domain(), chainID); err != nil {
		t.Fatalf("valid authorization should build: %v", err)
	}
}

// ---------------------------------------------------------------------------
// Known EIP-712 vectors
//
// Vectors generated externally using the Anvil/Hardhat test mnemonic key #0:
//
//   privateKey  = 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
//   address     = 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
//
// If any constant here changes (constants.go field reorder, type-string drift,
// etc.) these tests will fail — they lock the Go EIP-712 output to known
// reference values byte-for-byte.
// ---------------------------------------------------------------------------

const (
	testVectorPrivKey = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"

	// Exact Permit2 witness on Base mainnet, see gen-vec.ts vector 1.
	vectorExactHash      = "0xd801ba901eae839b027c0b6425caf6dfc10fc8fd73bc2f95eafd597fb98c910b"
	vectorExactSignature = "0x0a6578063042c6eb627310e79eb8009295cc32a02a2ef22c1e62af129fee45ff3c8a09fb86d2e17d5d2270bd59d9b14974c5362a361a7a83648acf40072013651b"

	// Upto Permit2 witness on Base mainnet, see gen-vec.ts vector 2.
	vectorUptoHash      = "0x2605741f2b85f06df8263f2422c49574ce4b5af2a6f957f34184e2ae5a346e10"
	vectorUptoSignature = "0xc66ef5cbcd4272cd00e661bf24dc7ba9b1ef81350e9eef1adeb5b33e9af0b05c493608508d528a7f0650752407262aa85093fb6118225abb0f2aad1a2c5b71461b"

	// EIP-2612 permit on USDC ("USD Coin" v2) chain 84532, see gen-vec.ts vector 3.
	vectorEip2612Hash      = "0xe3b54396b928981f7218a2a38e389ef66bd2b8ba4ce57c5c71aa14a0f7a4dc4d"
	vectorEip2612Signature = "0x5f9cd9e49354f51f21a055b400e527e09502529933dfd02e0af047182c456122436aeeb4f963363e338b98a3479d08b3aab015d99dcc271e4a8b7c95b6dc92391b"
)

func TestPermit2KnownVector_Exact(t *testing.T) {
	signer := newTestClientSignerFromHex(t, testVectorPrivKey)
	chainID := big.NewInt(8453)

	auth := Permit2Authorization{
		From: signer.Address(),
		Permitted: Permit2TokenPermissions{
			Token:  "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
			Amount: "1000000",
		},
		Spender:  X402ExactPermit2ProxyAddress,
		Nonce:    "12345678901234567890",
		Deadline: "1700000600",
		Witness: Permit2Witness{
			To:         "0x000000000000000000000000000000000000bEEF",
			ValidAfter: "1700000000",
		},
	}

	sig, hash, err := SignPermit2Witness(context.Background(), signer, auth, DefaultPermit2Domain(), chainID)
	if err != nil {
		t.Fatalf("SignPermit2Witness: %v", err)
	}
	if got := "0x" + hex.EncodeToString(hash); got != vectorExactHash {
		t.Fatalf("exact hash mismatch: got %s want %s", got, vectorExactHash)
	}
	if got := "0x" + hex.EncodeToString(sig); got != vectorExactSignature {
		t.Fatalf("exact signature mismatch: got %s want %s", got, vectorExactSignature)
	}
}

func TestPermit2KnownVector_Upto(t *testing.T) {
	signer := newTestClientSignerFromHex(t, testVectorPrivKey)
	chainID := big.NewInt(8453)

	auth := UptoPermit2Authorization{
		From: signer.Address(),
		Permitted: Permit2TokenPermissions{
			Token:  "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
			Amount: "2500000",
		},
		Spender:  X402UptoPermit2ProxyAddress,
		Nonce:    "98765432109876543210",
		Deadline: "1800000600",
		Witness: UptoPermit2Witness{
			To:          "0x000000000000000000000000000000000000bEEF",
			Facilitator: "0x000000000000000000000000000000000000FaC1",
			ValidAfter:  "1800000000",
		},
	}

	sig, hash, err := SignPermit2Witness(context.Background(), signer, auth, DefaultPermit2Domain(), chainID)
	if err != nil {
		t.Fatalf("SignPermit2Witness (upto): %v", err)
	}
	if got := "0x" + hex.EncodeToString(hash); got != vectorUptoHash {
		t.Fatalf("upto hash mismatch: got %s want %s", got, vectorUptoHash)
	}
	if got := "0x" + hex.EncodeToString(sig); got != vectorUptoSignature {
		t.Fatalf("upto signature mismatch: got %s want %s", got, vectorUptoSignature)
	}
}

func TestEip2612KnownVector(t *testing.T) {
	signer := newTestClientSignerFromHex(t, testVectorPrivKey)
	chainID := big.NewInt(84532)

	token := Eip2612TokenInfo{
		Name:    "USD Coin",
		Version: "2",
		Address: common.HexToAddress("0x036CbD53842c5426634e7929541eC2318f3dCF7e"),
	}

	td, err := BuildEip2612PermitTypedData(
		token,
		common.HexToAddress(signer.Address()),
		common.HexToAddress(PERMIT2Address),
		big.NewInt(1_000_000),
		big.NewInt(7),
		big.NewInt(1_900_000_000),
		chainID,
	)
	if err != nil {
		t.Fatalf("BuildEip2612PermitTypedData: %v", err)
	}
	// Compute the hash via the internal helper for parity check.
	digest, err := hashPermit2TypedData(td)
	if err != nil {
		t.Fatalf("hashPermit2TypedData: %v", err)
	}
	if got := "0x" + hex.EncodeToString(digest); got != vectorEip2612Hash {
		t.Fatalf("eip2612 hash mismatch: got %s want %s", got, vectorEip2612Hash)
	}

	sig, err := SignEip2612Permit(
		context.Background(),
		signer,
		token,
		common.HexToAddress(signer.Address()),
		common.HexToAddress(PERMIT2Address),
		big.NewInt(1_000_000),
		big.NewInt(7),
		big.NewInt(1_900_000_000),
		chainID,
	)
	if err != nil {
		t.Fatalf("SignEip2612Permit: %v", err)
	}
	if got := "0x" + hex.EncodeToString(sig); got != vectorEip2612Signature {
		t.Fatalf("eip2612 signature mismatch: got %s want %s", got, vectorEip2612Signature)
	}
}

// ---------------------------------------------------------------------------
// Negative — signature from a different private key fails verification.
// ---------------------------------------------------------------------------

func TestVerifyPermit2Witness_WrongKey(t *testing.T) {
	realSigner := newTestClientSigner(t)
	attacker := newTestClientSigner(t)
	chainID := big.NewInt(1)

	auth := Permit2Authorization{
		From: realSigner.Address(),
		Permitted: Permit2TokenPermissions{
			Token:  "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
			Amount: "1000000",
		},
		Spender:  X402ExactPermit2ProxyAddress,
		Nonce:    "42",
		Deadline: "1700000600",
		Witness: Permit2Witness{
			To:         "0x000000000000000000000000000000000000bEEF",
			ValidAfter: "1700000000",
		},
	}

	// Sign with attacker's key — verification against real signer must fail.
	sig, _, err := SignPermit2Witness(context.Background(), attacker, auth, DefaultPermit2Domain(), chainID)
	if err != nil {
		t.Fatalf("attacker sign: %v", err)
	}
	ok, err := VerifyPermit2Witness(realSigner.address, sig, auth, DefaultPermit2Domain(), chainID)
	if err != nil {
		t.Fatalf("VerifyPermit2Witness: %v", err)
	}
	if ok {
		t.Fatal("VerifyPermit2Witness returned true for signature from a different key")
	}
}

// ---------------------------------------------------------------------------
// Negative — malformed signature length.
//
// Documented behavior: VerifyPermit2Witness returns (false, nil) — NOT an
// error — when the signature is not exactly 65 bytes. This matches the TS
// verifyTypedData contract (a malformed sig is just "not valid", not an
// exception).
// ---------------------------------------------------------------------------

func TestVerifyPermit2Witness_MalformedSigLength(t *testing.T) {
	signer := newTestClientSigner(t)
	chainID := big.NewInt(1)
	auth := Permit2Authorization{
		From: signer.Address(),
		Permitted: Permit2TokenPermissions{
			Token:  "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
			Amount: "1",
		},
		Spender:  X402ExactPermit2ProxyAddress,
		Nonce:    "1",
		Deadline: "2",
		Witness:  Permit2Witness{To: signer.Address(), ValidAfter: "0"},
	}

	tooShort := make([]byte, 64) // 64 bytes — one short of EOA sig length
	ok, err := VerifyPermit2Witness(signer.address, tooShort, auth, DefaultPermit2Domain(), chainID)
	if err != nil {
		t.Fatalf("malformed sig length should not error, got: %v", err)
	}
	if ok {
		t.Fatal("VerifyPermit2Witness returned true for 64-byte signature")
	}

	tooLong := make([]byte, 66)
	ok, err = VerifyPermit2Witness(signer.address, tooLong, auth, DefaultPermit2Domain(), chainID)
	if err != nil {
		t.Fatalf("malformed sig length should not error, got: %v", err)
	}
	if ok {
		t.Fatal("VerifyPermit2Witness returned true for 66-byte signature")
	}

	empty := []byte{}
	ok, err = VerifyPermit2Witness(signer.address, empty, auth, DefaultPermit2Domain(), chainID)
	if err != nil {
		t.Fatalf("empty sig should not error, got: %v", err)
	}
	if ok {
		t.Fatal("VerifyPermit2Witness returned true for empty signature")
	}
}

// ---------------------------------------------------------------------------
// Witness type-string identity — sanity check that the WitnessTypeString
// method on the auth values matches the canonical TS constants in
// constants.go.
// ---------------------------------------------------------------------------

func TestWitnessTypeStrings_MatchTSConstants(t *testing.T) {
	if got := (Permit2Authorization{}).WitnessTypeString(); got != Permit2ExactWitnessTypeString {
		t.Fatalf("exact witness type string: got %q want %q", got, Permit2ExactWitnessTypeString)
	}
	if got := (UptoPermit2Authorization{}).WitnessTypeString(); got != Permit2UptoWitnessTypeString {
		t.Fatalf("upto witness type string: got %q want %q", got, Permit2UptoWitnessTypeString)
	}
	// Sanity: the upto string MUST contain "facilitator" — the discriminator
	// between exact and upto on-chain. Catches accidental witness-type drift.
	if !strings.Contains(Permit2UptoWitnessTypeString, "facilitator") {
		t.Fatal("upto witness type string is missing 'facilitator' field — drift vs TS")
	}
}
