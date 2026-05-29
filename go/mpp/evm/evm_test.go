package evm

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"math/big"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/common/math"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/signer/core/apitypes"
	"github.com/okx/payments/go/mpp/protocol"
)

// hardhatAddress is the checksummed address derived from the well-known Hardhat test key #0.
const hardhatAddress = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"

// ─────────────────────────────────────────────────────────────────────────────
// ParseAddress
// ─────────────────────────────────────────────────────────────────────────────

func TestParseAddress(t *testing.T) {
	// Expected normalized form (lowercase).
	hardhatLower := strings.ToLower(hardhatAddress)

	tests := []struct {
		name    string
		input   string
		want    string
		wantErr bool
	}{
		{
			name:  "valid with 0x prefix",
			input: hardhatAddress,
			want:  hardhatLower,
		},
		{
			name:  "valid without 0x prefix",
			input: hardhatAddress[2:],
			want:  hardhatLower,
		},
		{
			name:  "all zeros",
			input: "0x0000000000000000000000000000000000000000",
			want:  "0x0000000000000000000000000000000000000000",
		},
		{
			name:    "too short",
			input:   "0xdeadbeef",
			wantErr: true,
		},
		{
			name:    "too long",
			input:   "0x" + strings.Repeat("a", 42),
			wantErr: true,
		},
		{
			name:    "invalid hex chars",
			input:   "0x" + strings.Repeat("g", 40),
			wantErr: true,
		},
		{
			name:    "empty string",
			input:   "",
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ParseAddress(tt.input)
			if (err != nil) != tt.wantErr {
				t.Errorf("ParseAddress(%q) error = %v, wantErr %v", tt.input, err, tt.wantErr)
				return
			}
			if !tt.wantErr && got != tt.want {
				t.Errorf("ParseAddress(%q) = %s, want %s", tt.input, got, tt.want)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ParseAmount
// ─────────────────────────────────────────────────────────────────────────────

func TestParseAmount(t *testing.T) {
	tests := []struct {
		name    string
		input   string
		want    string
		wantErr bool
	}{
		{name: "zero", input: "0", want: "0"},
		{name: "positive integer", input: "1000000", want: "1000000"},
		{name: "large value", input: "999999999999999999999", want: "999999999999999999999"},
		{name: "negative", input: "-1", wantErr: true},
		{name: "decimal", input: "1.5", wantErr: true},
		{name: "empty", input: "", wantErr: true},
		{name: "letters", input: "abc", wantErr: true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ParseAmount(tt.input)
			if (err != nil) != tt.wantErr {
				t.Errorf("ParseAmount(%q) error = %v, wantErr %v", tt.input, err, tt.wantErr)
				return
			}
			if !tt.wantErr {
				want, _ := new(big.Int).SetString(tt.want, 10)
				if got.Cmp(want) != 0 {
					t.Errorf("ParseAmount(%q) = %s, want %s", tt.input, got.String(), want.String())
				}
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ParseMemoBytes
// ─────────────────────────────────────────────────────────────────────────────

func TestParseMemoBytes(t *testing.T) {
	valid32 := strings.Repeat("ab", 32) // 32 bytes = 64 hex chars

	tests := []struct {
		name    string
		input   string
		wantErr bool
	}{
		{name: "valid 32-byte hex", input: valid32},
		{name: "valid with 0x prefix", input: "0x" + valid32},
		{name: "too short (31 bytes)", input: strings.Repeat("ab", 31), wantErr: true},
		{name: "too long (33 bytes)", input: strings.Repeat("ab", 33), wantErr: true},
		{name: "invalid hex", input: strings.Repeat("zz", 32), wantErr: true},
		{name: "empty", input: "", wantErr: true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ParseMemoBytes(tt.input)
			if (err != nil) != tt.wantErr {
				t.Errorf("ParseMemoBytes(%q) error = %v, wantErr %v", tt.input, err, tt.wantErr)
				return
			}
			if !tt.wantErr {
				// First bytes should match decoded hex
				raw := tt.input
				if strings.HasPrefix(raw, "0x") || strings.HasPrefix(raw, "0X") {
					raw = raw[2:]
				}
				b, _ := hex.DecodeString(raw)
				var want [32]byte
				copy(want[:], b)
				if got != want {
					t.Errorf("ParseMemoBytes mismatch")
				}
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ParseUnits (evm package — errors on excess decimals)
// ─────────────────────────────────────────────────────────────────────────────

func TestParseUnits(t *testing.T) {
	tests := []struct {
		name     string
		amount   string
		decimals uint8
		want     string
		wantErr  bool
	}{
		{name: "integer 1 with 6 decimals", amount: "1", decimals: 6, want: "1000000"},
		{name: "1.5 with 6 decimals", amount: "1.5", decimals: 6, want: "1500000"},
		{name: "1.000001 with 6 decimals", amount: "1.000001", decimals: 6, want: "1000001"},
		{name: "zero decimals integer", amount: "42", decimals: 0, want: "42"},
		{name: "zero amount", amount: "0", decimals: 18, want: "0"},
		{name: "excess fractional digits", amount: "1.1234567", decimals: 6, wantErr: true},
		{name: "negative", amount: "-1", decimals: 6, wantErr: true},
		{name: "empty", amount: "", decimals: 6, wantErr: true},
		{name: "letters", amount: "abc", decimals: 6, wantErr: true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ParseUnits(tt.amount, tt.decimals)
			if (err != nil) != tt.wantErr {
				t.Errorf("ParseUnits(%q, %d) error = %v, wantErr %v", tt.amount, tt.decimals, err, tt.wantErr)
				return
			}
			if !tt.wantErr && got != tt.want {
				t.Errorf("ParseUnits(%q, %d) = %q, want %q", tt.amount, tt.decimals, got, tt.want)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// parseProofSource
// ─────────────────────────────────────────────────────────────────────────────

func TestParseProofSource(t *testing.T) {
	addrLower := strings.ToLower(hardhatAddress)

	tests := []struct {
		name      string
		input     string
		wantAddr  string
		wantChain uint64
		wantErr   bool
	}{
		{
			name:      "valid chain 1",
			input:     "did:pkh:eip155:1:" + hardhatAddress,
			wantAddr:  addrLower,
			wantChain: 1,
		},
		{
			name:      "valid XLayer",
			input:     "did:pkh:eip155:196:" + hardhatAddress,
			wantAddr:  addrLower,
			wantChain: 196,
		},
		{
			name:    "wrong prefix",
			input:   "did:ethr:1:" + hardhatAddress,
			wantErr: true,
		},
		{
			name:    "missing address separator",
			input:   "did:pkh:eip155:1",
			wantErr: true,
		},
		{
			name:    "invalid chain id",
			input:   "did:pkh:eip155:abc:" + hardhatAddress,
			wantErr: true,
		},
		{
			name:    "invalid address",
			input:   "did:pkh:eip155:1:notanaddress",
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := parseProofSource(tt.input)
			if (err != nil) != tt.wantErr {
				t.Errorf("parseProofSource(%q) error = %v, wantErr %v", tt.input, err, tt.wantErr)
				return
			}
			if !tt.wantErr {
				if got.Address != tt.wantAddr {
					t.Errorf("Address = %s, want %s", got.Address, tt.wantAddr)
				}
				if got.ChainID != tt.wantChain {
					t.Errorf("ChainID = %d, want %d", got.ChainID, tt.wantChain)
				}
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// EVMMethodDetails JSON + IsFeePayer
// ─────────────────────────────────────────────────────────────────────────────

func TestEVMMethodDetailsJSON(t *testing.T) {
	chainID := uint64(1)
	feePayer := true
	memo := "hello"

	details := &EVMMethodDetails{
		ChainID:  &chainID,
		FeePayer: &feePayer,
		Memo:     &memo,
		Splits: []Split{
			{Amount: "100", Recipient: hardhatAddress},
		},
	}

	data, err := json.Marshal(details)
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}

	var got EVMMethodDetails
	if err := json.Unmarshal(data, &got); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}

	if got.ChainID == nil || *got.ChainID != chainID {
		t.Errorf("ChainID = %v, want %d", got.ChainID, chainID)
	}
	if got.Memo == nil || *got.Memo != memo {
		t.Errorf("Memo = %v, want %q", got.Memo, memo)
	}
	if len(got.Splits) != 1 || got.Splits[0].Amount != "100" {
		t.Errorf("Splits = %v, want 1 split with amount 100", got.Splits)
	}
}

func TestEVMMethodDetailsIsFeePayer(t *testing.T) {
	tests := []struct {
		name    string
		details EVMMethodDetails
		wantFP  bool
	}{
		{
			name:    "nil FeePayer",
			details: EVMMethodDetails{},
			wantFP:  false,
		},
		{
			name:    "FeePayer false",
			details: EVMMethodDetails{FeePayer: func() *bool { b := false; return &b }()},
			wantFP:  false,
		},
		{
			name:    "FeePayer true",
			details: EVMMethodDetails{FeePayer: func() *bool { b := true; return &b }()},
			wantFP:  true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := tt.details.IsFeePayer()
			if got != tt.wantFP {
				t.Errorf("IsFeePayer() = %v, want %v", got, tt.wantFP)
			}
		})
	}
}

func TestEVMSessionMethodDetailsIsFeePayer(t *testing.T) {
	tests := []struct {
		name    string
		details EVMSessionMethodDetails
		wantFP  bool
	}{
		{
			name:    "nil FeePayer",
			details: EVMSessionMethodDetails{EscrowContract: "0xabc"},
			wantFP:  false,
		},
		{
			name: "FeePayer true",
			details: EVMSessionMethodDetails{
				EscrowContract: "0xabc",
				FeePayer:       func() *bool { b := true; return &b }(),
			},
			wantFP: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.details.IsFeePayer(); got != tt.wantFP {
				t.Errorf("IsFeePayer() = %v, want %v", got, tt.wantFP)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// GetTransfers split validation
// ─────────────────────────────────────────────────────────────────────────────

func TestGetTransfers(t *testing.T) {
	primary := strings.ToLower(hardhatAddress)
	secondary := strings.ToLower("0x70997970C51812dc3A010C7d01b50e0d17dc79C8")
	total := big.NewInt(1000)

	tests := []struct {
		name          string
		total         *big.Int
		splits        []Split
		wantErr       bool
		wantPrimary   int64
		wantSplitAmts []int64
	}{
		{
			name:        "no splits",
			total:       total,
			splits:      nil,
			wantPrimary: 1000,
		},
		{
			name:  "one split",
			total: total,
			splits: []Split{
				{Amount: "300", Recipient: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"},
			},
			wantPrimary:   700,
			wantSplitAmts: []int64{300},
		},
		{
			name:  "split equals total",
			total: total,
			splits: []Split{
				{Amount: "1000", Recipient: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"},
			},
			wantErr: true,
		},
		{
			name:  "split exceeds total",
			total: total,
			splits: []Split{
				{Amount: "1001", Recipient: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"},
			},
			wantErr: true,
		},
		{
			name:    "nil total",
			total:   nil,
			splits:  nil,
			wantErr: true,
		},
		{
			name:  "too many splits",
			total: big.NewInt(1_000_000),
			splits: func() []Split {
				s := make([]Split, MaxSplits+1)
				for i := range s {
					s[i] = Split{Amount: "1", Recipient: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"}
				}
				return s
			}(),
			wantErr: true,
		},
		{
			name:  "invalid split amount",
			total: total,
			splits: []Split{
				{Amount: "not-a-number", Recipient: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"},
			},
			wantErr: true,
		},
		{
			name:  "invalid split recipient",
			total: total,
			splits: []Split{
				{Amount: "100", Recipient: "not-an-address"},
			},
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			transfers, err := GetTransfers(tt.total, primary, nil, tt.splits)
			if (err != nil) != tt.wantErr {
				t.Errorf("GetTransfers error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if tt.wantErr {
				return
			}

			if len(transfers) != 1+len(tt.splits) {
				t.Fatalf("len(transfers) = %d, want %d", len(transfers), 1+len(tt.splits))
			}

			// Primary leg
			wantPrimary := big.NewInt(tt.wantPrimary)
			if transfers[0].Amount.Cmp(wantPrimary) != 0 {
				t.Errorf("primary amount = %s, want %s", transfers[0].Amount.String(), wantPrimary.String())
			}
			if transfers[0].Recipient != primary {
				t.Errorf("primary recipient = %s, want %s", transfers[0].Recipient, primary)
			}

			// Split legs
			for i, want := range tt.wantSplitAmts {
				wantAmt := big.NewInt(want)
				if transfers[i+1].Amount.Cmp(wantAmt) != 0 {
					t.Errorf("split[%d] amount = %s, want %s", i, transfers[i+1].Amount.String(), wantAmt.String())
				}
				if transfers[i+1].Recipient != secondary {
					t.Errorf("split[%d] recipient = %s, want %s", i, transfers[i+1].Recipient, secondary)
				}
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Session ext helpers
// ─────────────────────────────────────────────────────────────────────────────

func mkSessionJSON(t *testing.T, v interface{}) json.RawMessage {
	t.Helper()
	b, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("mkSessionJSON: %v", err)
	}
	return b
}

func TestSessionEscrowContract(t *testing.T) {
	tests := []struct {
		name    string
		json    json.RawMessage
		want    string
		wantErr bool
	}{
		{
			name: "present",
			json: mkSessionJSON(t, map[string]interface{}{"escrowContract": hardhatAddress}),
			want: hardhatAddress,
		},
		{
			name:    "absent (empty string fails escrowContract required check)",
			json:    mkSessionJSON(t, map[string]interface{}{"channelId": "0x1234"}),
			wantErr: true,
		},
		{
			name:    "invalid JSON",
			json:    json.RawMessage(`{invalid`),
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := SessionEscrowContract(tt.json)
			if (err != nil) != tt.wantErr {
				t.Errorf("SessionEscrowContract error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if !tt.wantErr && got != tt.want {
				t.Errorf("SessionEscrowContract = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestSessionChannelID(t *testing.T) {
	chanID := "0xdeadbeef"
	tests := []struct {
		name string
		json json.RawMessage
		want *string
	}{
		{
			name: "present",
			json: mkSessionJSON(t, map[string]interface{}{
				"escrowContract": hardhatAddress,
				"channelId":      chanID,
			}),
			want: &chanID,
		},
		{
			name: "absent",
			json: mkSessionJSON(t, map[string]interface{}{"escrowContract": hardhatAddress}),
			want: nil,
		},
		{
			name: "invalid JSON",
			json: json.RawMessage(`{invalid`),
			want: nil,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := SessionChannelID(tt.json)
			if tt.want == nil {
				if got != nil {
					t.Errorf("SessionChannelID = %v, want nil", got)
				}
			} else {
				if got == nil {
					t.Errorf("SessionChannelID = nil, want %q", *tt.want)
				} else if *got != *tt.want {
					t.Errorf("SessionChannelID = %q, want %q", *got, *tt.want)
				}
			}
		})
	}
}

func TestSessionMinVoucherDelta(t *testing.T) {
	delta := "5000"
	tests := []struct {
		name string
		json json.RawMessage
		want *string
	}{
		{
			name: "present",
			json: mkSessionJSON(t, map[string]interface{}{
				"escrowContract":  hardhatAddress,
				"minVoucherDelta": delta,
			}),
			want: &delta,
		},
		{
			name: "absent",
			json: mkSessionJSON(t, map[string]interface{}{"escrowContract": hardhatAddress}),
			want: nil,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := SessionMinVoucherDelta(tt.json)
			if tt.want == nil {
				if got != nil {
					t.Errorf("SessionMinVoucherDelta = %v, want nil", got)
				}
			} else {
				if got == nil {
					t.Errorf("SessionMinVoucherDelta = nil, want %q", *tt.want)
				} else if *got != *tt.want {
					t.Errorf("SessionMinVoucherDelta = %q, want %q", *got, *tt.want)
				}
			}
		})
	}
}

func TestSessionChainID(t *testing.T) {
	chainID := uint64(196)
	tests := []struct {
		name string
		json json.RawMessage
		want *uint64
	}{
		{
			name: "present",
			json: mkSessionJSON(t, map[string]interface{}{
				"escrowContract": hardhatAddress,
				"chainId":        chainID,
			}),
			want: &chainID,
		},
		{
			name: "absent",
			json: mkSessionJSON(t, map[string]interface{}{"escrowContract": hardhatAddress}),
			want: nil,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := SessionChainID(tt.json)
			if tt.want == nil {
				if got != nil {
					t.Errorf("SessionChainID = %v, want nil", got)
				}
			} else {
				if got == nil {
					t.Errorf("SessionChainID = nil, want %d", *tt.want)
				} else if *got != *tt.want {
					t.Errorf("SessionChainID = %d, want %d", *got, *tt.want)
				}
			}
		})
	}
}

func TestSessionFeePayer(t *testing.T) {
	tests := []struct {
		name string
		json json.RawMessage
		want bool
	}{
		{
			name: "true",
			json: mkSessionJSON(t, map[string]interface{}{
				"escrowContract": hardhatAddress,
				"feePayer":       true,
			}),
			want: true,
		},
		{
			name: "false",
			json: mkSessionJSON(t, map[string]interface{}{
				"escrowContract": hardhatAddress,
				"feePayer":       false,
			}),
			want: false,
		},
		{
			name: "absent",
			json: mkSessionJSON(t, map[string]interface{}{"escrowContract": hardhatAddress}),
			want: false,
		},
		{
			name: "invalid JSON",
			json: json.RawMessage(`{invalid`),
			want: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := SessionFeePayer(tt.json)
			if got != tt.want {
				t.Errorf("SessionFeePayer = %v, want %v", got, tt.want)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// EVMChargeMethod
// ─────────────────────────────────────────────────────────────────────────────

func TestEVMChargeMethodMethod(t *testing.T) {
	m := NewEVMChargeMethod()
	if got := m.Method(); got != MethodNameEVM {
		t.Errorf("Method() = %q, want %q", got, MethodNameEVM)
	}
}

func TestEVMChargeMethodWithFeePayer(t *testing.T) {
	m := NewEVMChargeMethod().WithFeePayer(true)
	if !m.feePayer {
		t.Error("feePayer should be true")
	}
	m2 := NewEVMChargeMethod().WithFeePayer(false)
	if m2.feePayer {
		t.Error("feePayer should be false")
	}
}

func TestEVMChargeMethodPrepareRequest_NoDecimals(t *testing.T) {
	m := NewEVMChargeMethod()
	req := protocol.ChargeRequest{Amount: "1000", Currency: "USDT"}
	result := m.PrepareRequest(req, nil)
	if result.Amount != "1000" {
		t.Errorf("Amount: got %s want 1000", result.Amount)
	}
}

func TestEVMChargeMethodPrepareRequest_WithDecimals(t *testing.T) {
	m := NewEVMChargeMethod()
	decimals := uint8(6)
	req := protocol.ChargeRequest{Amount: "1", Currency: "USDT", Decimals: &decimals}
	result := m.PrepareRequest(req, nil)
	if result.Amount != "1000000" {
		t.Errorf("Amount: got %s want 1000000", result.Amount)
	}
	if result.Decimals != nil {
		t.Error("Decimals should be nil after conversion")
	}
}

func TestEVMChargeMethodPrepareRequest_InvalidAmountFallback(t *testing.T) {
	m := NewEVMChargeMethod()
	decimals := uint8(6)
	req := protocol.ChargeRequest{Amount: "not-a-number", Currency: "USDT", Decimals: &decimals}
	result := m.PrepareRequest(req, nil)
	if result.Amount != "not-a-number000000" {
		t.Errorf("Amount: got %s want 'not-a-number000000'", result.Amount)
	}
}

func TestEVMChargeMethodVerify(t *testing.T) {
	challengeID := "test-challenge-001"
	chainID := uint64(1)

	proofSourceStr := "did:pkh:eip155:1:" + hardhatAddress
	echo := &protocol.ChallengeEcho{ID: challengeID}
	method := NewEVMChargeMethod().WithChainID(chainID).WithRecipient(hardhatAddress)
	req := &protocol.ChargeRequest{}

	tests := []struct {
		name    string
		cred    *protocol.PaymentCredential
		wantErr bool
	}{
		{
			name:    "nil credential",
			cred:    nil,
			wantErr: true,
		},
		{
			name: "nil echo",
			cred: &protocol.PaymentCredential{
				Payload: protocol.NewProofPayload("deadbeef"),
			},
			wantErr: true,
		},
		{
			name: "nil payload",
			cred: &protocol.PaymentCredential{
				Echo: echo,
			},
			wantErr: true,
		},
		{
			name: "proof payload valid (SA API handles sig verification)",
			cred: &protocol.PaymentCredential{
				Echo:    echo,
				Source:  proofSourceStr,
				Payload: protocol.NewProofPayload("deadbeef"),
			},
			wantErr: false,
		},
		{
			name: "transaction payload",
			cred: &protocol.PaymentCredential{
				Echo:    echo,
				Payload: protocol.NewTransactionPayload("0xdeadbeef"),
			},
			wantErr: false,
		},
		{
			name: "hash payload",
			cred: &protocol.PaymentCredential{
				Echo:    echo,
				Payload: protocol.NewHashPayload("0xdeadbeef"),
			},
			wantErr: false,
		},
		{
			name: "unsupported payload type",
			cred: &protocol.PaymentCredential{
				Echo: echo,
				Payload: &protocol.PaymentPayload{
					Type:    "unknown",
					Payload: "",
				},
			},
			wantErr: true,
		},
		{
			name: "proof payload missing source",
			cred: &protocol.PaymentCredential{
				Echo:    echo,
				Payload: protocol.NewProofPayload("deadbeef"),
			},
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			receipt, err := method.Verify(context.Background(), tt.cred, req)
			if (err != nil) != tt.wantErr {
				t.Errorf("Verify error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if !tt.wantErr {
				if receipt == nil {
					t.Error("Verify returned nil receipt without error")
				}
			}
		})
	}
}

func TestEVMChargeMethodVerifyChainIDMismatch(t *testing.T) {
	challengeID := "chal-chain-mismatch"
	signerChainID := uint64(1)
	methodChainID := uint64(2) // different

	proofSourceStr := "did:pkh:eip155:" + "1" + ":" + hardhatAddress

	method := NewEVMChargeMethod().WithChainID(methodChainID)
	cred := &protocol.PaymentCredential{
		Echo:    &protocol.ChallengeEcho{ID: challengeID},
		Source:  proofSourceStr,
		Payload: protocol.NewProofPayload("deadbeef"),
	}
	_ = signerChainID

	_, err := method.Verify(context.Background(), cred, &protocol.ChargeRequest{})
	if err == nil {
		t.Error("expected chain ID mismatch error, got nil")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// SessionReceipt.ToBaseReceipt
// ─────────────────────────────────────────────────────────────────────────────

func TestSessionReceiptToBaseReceipt(t *testing.T) {
	channelID := "0xchannel"
	cumAmount := "1000000"
	escrow := hardhatAddress
	status := "open"
	challengeID := "chal-session-001"

	sr := NewSessionReceipt(channelID, cumAmount, escrow, status, "")
	receipt, err := sr.ToBaseReceipt(challengeID)
	if err != nil {
		t.Fatalf("ToBaseReceipt: %v", err)
	}

	if receipt == nil {
		t.Fatal("receipt is nil")
	}
	if receipt.ID != challengeID {
		t.Errorf("receipt.ID = %q, want %q", receipt.ID, challengeID)
	}
	if string(receipt.Status) != "success" {
		t.Errorf("receipt.Status = %q, want success", receipt.Status)
	}
	if string(receipt.Method) != MethodNameEVM {
		t.Errorf("receipt.Method = %q, want %q", receipt.Method, MethodNameEVM)
	}
	if string(receipt.Intent) != protocol.IntentSession {
		t.Errorf("receipt.Intent = %q, want %q", receipt.Intent, protocol.IntentSession)
	}
	if receipt.Settlement == "" {
		t.Error("receipt.Settlement is empty")
	}

	// Decode settlement and verify round-trip.
	decoded, err := protocol.Base64URLDecode(receipt.Settlement)
	if err != nil {
		t.Fatalf("Base64URLDecode: %v", err)
	}

	var got SessionReceipt
	if err := json.Unmarshal(decoded, &got); err != nil {
		t.Fatalf("Unmarshal settlement: %v", err)
	}

	if got.ChannelID != channelID {
		t.Errorf("ChannelID = %q, want %q", got.ChannelID, channelID)
	}
	if got.CumulativeAmount != cumAmount {
		t.Errorf("CumulativeAmount = %q, want %q", got.CumulativeAmount, cumAmount)
	}
	if got.EscrowContract != escrow {
		t.Errorf("EscrowContract = %q, want %q", got.EscrowContract, escrow)
	}
	if got.Status != status {
		t.Errorf("Status = %q, want %q", got.Status, status)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ParseEVMMethodDetails / ParseEVMSessionMethodDetails
// ─────────────────────────────────────────────────────────────────────────────

func TestParseEVMMethodDetails(t *testing.T) {
	tests := []struct {
		name    string
		input   string
		wantErr bool
		check   func(*testing.T, *EVMMethodDetails)
	}{
		{
			name:  "minimal empty object",
			input: `{}`,
			check: func(t *testing.T, d *EVMMethodDetails) {
				if d.ChainID != nil {
					t.Error("expected nil ChainID")
				}
			},
		},
		{
			name:  "with chainId and feePayer",
			input: `{"chainId":1,"feePayer":true}`,
			check: func(t *testing.T, d *EVMMethodDetails) {
				if d.ChainID == nil || *d.ChainID != 1 {
					t.Errorf("ChainID = %v", d.ChainID)
				}
				if !d.IsFeePayer() {
					t.Error("IsFeePayer should be true")
				}
			},
		},
		{
			name:    "invalid JSON",
			input:   `{invalid`,
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ParseEVMMethodDetails(json.RawMessage(tt.input))
			if (err != nil) != tt.wantErr {
				t.Errorf("ParseEVMMethodDetails error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if !tt.wantErr && tt.check != nil {
				tt.check(t, got)
			}
		})
	}
}

func TestParseEVMSessionMethodDetails(t *testing.T) {
	tests := []struct {
		name    string
		input   string
		wantErr bool
		check   func(*testing.T, *EVMSessionMethodDetails)
	}{
		{
			name:  "full fields",
			input: `{"escrowContract":"` + hardhatAddress + `","channelId":"0x1234","chainId":1}`,
			check: func(t *testing.T, d *EVMSessionMethodDetails) {
				if d.EscrowContract != hardhatAddress {
					t.Errorf("EscrowContract = %q", d.EscrowContract)
				}
				if d.ChannelID == nil || *d.ChannelID != "0x1234" {
					t.Errorf("ChannelID = %v", d.ChannelID)
				}
			},
		},
		{
			name:    "missing escrowContract",
			input:   `{"channelId":"0x1234"}`,
			wantErr: true,
		},
		{
			name:    "invalid JSON",
			input:   `{invalid`,
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ParseEVMSessionMethodDetails(json.RawMessage(tt.input))
			if (err != nil) != tt.wantErr {
				t.Errorf("ParseEVMSessionMethodDetails error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if !tt.wantErr && tt.check != nil {
				tt.check(t, got)
			}
		})
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// PrivateKeySigner.SignTypedData
// ─────────────────────────────────────────────────────────────────────────────

func TestPrivateKeySignerSignTypedData(t *testing.T) {
	key, err := crypto.GenerateKey()
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	signer := NewPrivateKeySigner(key)

	td := apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": []apitypes.Type{
				{Name: "name", Type: "string"},
				{Name: "version", Type: "string"},
				{Name: "chainId", Type: "uint256"},
			},
			"Mail": []apitypes.Type{
				{Name: "from", Type: "string"},
				{Name: "contents", Type: "string"},
			},
		},
		PrimaryType: "Mail",
		Domain: apitypes.TypedDataDomain{
			Name:    "TestDApp",
			Version: "1",
			ChainId: math.NewHexOrDecimal256(1),
		},
		Message: apitypes.TypedDataMessage{
			"from":     "alice",
			"contents": "hello bob",
		},
	}

	sig, err := signer.SignTypedData(td)
	if err != nil {
		t.Fatalf("SignTypedData: %v", err)
	}

	if len(sig) != 65 {
		t.Fatalf("signature length = %d, want 65", len(sig))
	}

	v := sig[64]
	if v != 27 && v != 28 {
		t.Errorf("v = %d, want 27 or 28", v)
	}
}
