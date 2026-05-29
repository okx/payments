package server

import (
	"strings"
	"testing"

	"github.com/okx/payments/go/mpp/protocol"
)

// ─────────────────────────────────────────────────────────────────────────────
// ParseDollarAmount
// ─────────────────────────────────────────────────────────────────────────────

func TestParseDollarAmount_IntegerNoDecimals(t *testing.T) {
	got, err := ParseDollarAmount("100", 0)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got != "100" {
		t.Errorf("got %q, want %q", got, "100")
	}
}

func TestParseDollarAmount_DecimalSixPlaces(t *testing.T) {
	got, err := ParseDollarAmount("1.50", 6)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got != "1500000" {
		t.Errorf("got %q, want %q", got, "1500000")
	}
}

func TestParseDollarAmount_SmallFraction(t *testing.T) {
	got, err := ParseDollarAmount("0.01", 2)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got != "1" {
		t.Errorf("got %q, want %q", got, "1")
	}
}

func TestParseDollarAmount_IntegerWithDecimals(t *testing.T) {
	// "5" with 3 decimal places → 5000
	got, err := ParseDollarAmount("5", 3)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got != "5000" {
		t.Errorf("got %q, want %q", got, "5000")
	}
}

func TestParseDollarAmount_ZeroAmount(t *testing.T) {
	got, err := ParseDollarAmount("0.00", 2)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got != "0" {
		t.Errorf("got %q, want %q", got, "0")
	}
}

func TestParseDollarAmount_ExactPrecision(t *testing.T) {
	// "1.000001" with 6 places → 1000001
	got, err := ParseDollarAmount("1.000001", 6)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got != "1000001" {
		t.Errorf("got %q, want %q", got, "1000001")
	}
}

func TestParseDollarAmount_EmptyString(t *testing.T) {
	_, err := ParseDollarAmount("", 6)
	if err == nil {
		t.Fatal("expected error for empty amount")
	}
	if !strings.Contains(err.Error(), "amount is empty") {
		t.Errorf("unexpected error message: %v", err)
	}
}

func TestParseDollarAmount_MultipleDecimalPoints(t *testing.T) {
	_, err := ParseDollarAmount("1.2.3", 6)
	if err == nil {
		t.Fatal("expected error for multiple decimal points")
	}
	if !strings.Contains(err.Error(), "multiple decimal points") {
		t.Errorf("unexpected error message: %v", err)
	}
}

func TestParseDollarAmount_InvalidCharacter(t *testing.T) {
	_, err := ParseDollarAmount("1a5", 6)
	if err == nil {
		t.Fatal("expected error for invalid character")
	}
	if !strings.Contains(err.Error(), "invalid character") {
		t.Errorf("unexpected error message: %v", err)
	}
}

func TestParseDollarAmount_NegativeSign(t *testing.T) {
	_, err := ParseDollarAmount("-1.50", 6)
	if err == nil {
		t.Fatal("expected error for negative sign")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// ChargeRouteConfig / SessionRouteConfig struct construction
// ─────────────────────────────────────────────────────────────────────────────

func TestChargeRouteConfig_ZeroValue(t *testing.T) {
	cfg := ChargeRouteConfig{}
	if cfg.Amount != "" {
		t.Errorf("expected empty amount, got %q", cfg.Amount)
	}
	if cfg.Description != "" {
		t.Errorf("expected empty description, got %q", cfg.Description)
	}
	if cfg.ExternalID != "" {
		t.Errorf("expected empty externalID, got %q", cfg.ExternalID)
	}
}

func TestSessionRouteConfig_ZeroValue(t *testing.T) {
	cfg := SessionRouteConfig{}
	if cfg.Amount != "" {
		t.Errorf("expected empty amount, got %q", cfg.Amount)
	}
	if cfg.UnitType != "" {
		t.Errorf("expected empty unitType, got %q", cfg.UnitType)
	}
	if cfg.SuggestedDeposit != "" {
		t.Errorf("expected empty suggestedDeposit, got %q", cfg.SuggestedDeposit)
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// Mpp nil-verifier guard paths
// ─────────────────────────────────────────────────────────────────────────────

func TestMpp_Charge_NilVerifier(t *testing.T) {
	m := NewMpp(EVMConfig{}, nil, nil)
	_, err := m.Charge(nil, ChargeRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	if err == nil {
		t.Fatal("expected error when charge verifier is nil")
	}
	if !strings.Contains(err.Error(), "charge verifier not configured") {
		t.Errorf("unexpected error: %v", err)
	}
}

func TestMpp_ChargeWithOptions_NilVerifier(t *testing.T) {
	m := NewMpp(EVMConfig{}, nil, nil)
	_, err := m.ChargeWithOptions(nil, protocol.ChargeRequest{}, ChargeOptions{})
	if err == nil {
		t.Fatal("expected error when charge verifier is nil")
	}
	if !strings.Contains(err.Error(), "charge verifier not configured") {
		t.Errorf("unexpected error: %v", err)
	}
}

func TestMpp_SessionChallenge_NilVerifier(t *testing.T) {
	m := NewMpp(EVMConfig{}, nil, nil)
	_, err := m.SessionChallenge(nil, SessionRouteConfig{Amount: "1.00", Currency: "USDC", Decimals: 6})
	if err == nil {
		t.Fatal("expected error when session verifier is nil")
	}
	if !strings.Contains(err.Error(), "session verifier not configured") {
		t.Errorf("unexpected error: %v", err)
	}
}

func TestMpp_SessionChallengeWithDetails_NilVerifier(t *testing.T) {
	m := NewMpp(EVMConfig{}, nil, nil)
	_, err := m.SessionChallengeWithDetails(nil, protocol.SessionRequest{}, SessionChallengeOptions{})
	if err == nil {
		t.Fatal("expected error when session verifier is nil")
	}
	if !strings.Contains(err.Error(), "session verifier not configured") {
		t.Errorf("unexpected error: %v", err)
	}
}

func TestMpp_VerifyCredential_NilVerifier(t *testing.T) {
	m := NewMpp(EVMConfig{}, nil, nil)
	_, err := m.VerifyCredential(nil, "challenge", "auth")
	if err == nil {
		t.Fatal("expected error when charge verifier is nil")
	}
	if !strings.Contains(err.Error(), "charge verifier not configured") {
		t.Errorf("unexpected error: %v", err)
	}
}

func TestMpp_VerifySession_NilVerifier(t *testing.T) {
	m := NewMpp(EVMConfig{}, nil, nil)
	_, err := m.VerifySession(nil, "challenge", "auth")
	if err == nil {
		t.Fatal("expected error when session verifier is nil")
	}
	if !strings.Contains(err.Error(), "session verifier not configured") {
		t.Errorf("unexpected error: %v", err)
	}
}

