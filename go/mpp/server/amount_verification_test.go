package server

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/okx/payments/go/mpp/protocol"
)

type routeProbe struct{ calls int }

func (p *routeProbe) Method() string { return "evm" }
func (p *routeProbe) PrepareRequest(r protocol.ChargeRequest, _ *protocol.PaymentCredential) protocol.ChargeRequest {
	return r
}
func (p *routeProbe) Verify(_ context.Context, c *protocol.PaymentCredential, _ *protocol.ChargeRequest) (*protocol.Receipt, error) {
	p.calls++
	return protocol.NewSuccessReceipt(c.Echo.ID, protocol.MethodName("evm"), protocol.IntentCharge, "fixture"), nil
}
func (p *routeProbe) ChallengeMethodDetails() json.RawMessage { return nil }
func (p *routeProbe) VerifySession(_ context.Context, c *protocol.PaymentCredential, _ *protocol.SessionRequest) (*protocol.Receipt, error) {
	p.calls++
	return protocol.NewSuccessReceipt(c.Echo.ID, protocol.MethodName("evm"), protocol.IntentSession, "fixture"), nil
}
func (p *routeProbe) Respond(_ *protocol.PaymentCredential, _ *protocol.Receipt) any { return nil }

func routeAuth(t *testing.T, header string) string {
	t.Helper()
	challenge, err := protocol.PaymentChallengeFromHeader(header)
	if err != nil {
		t.Fatal(err)
	}
	auth, err := protocol.FormatAuthorization(protocol.NewPaymentCredential(challenge.ToEcho(), protocol.NewHashPayload(`{"hash":"fixture"}`)))
	if err != nil {
		t.Fatal(err)
	}
	return auth
}

func TestRouteBoundChargeRejectsCheapCredentialBeforeVerifier(t *testing.T) {
	probe := &routeProbe{}
	m := NewMpp(EVMConfig{Recipient: "merchant", ChainID: 196}, probe, nil)
	cheap := ChargeRouteConfig{Amount: "0.01", Currency: "token", Decimals: 6}
	header, err := m.Charge(context.Background(), cheap)
	if err != nil {
		t.Fatal(err)
	}
	auth := routeAuth(t, header)
	expensive := cheap
	expensive.Amount = "5"
	if _, err := m.VerifyCredentialForRoute(context.Background(), expensive, auth); err == nil {
		t.Fatal("accepted cheap route credential")
	}
	if probe.calls != 0 {
		t.Fatal("verifier called for a route mismatch")
	}
	if _, err := m.VerifyCredentialForRoute(context.Background(), cheap, auth); err != nil {
		t.Fatal(err)
	}
	if probe.calls != 1 {
		t.Fatalf("calls=%d", probe.calls)
	}
}

func TestRouteBoundSessionRejectsDifferentAmount(t *testing.T) {
	probe := &routeProbe{}
	m := NewMpp(EVMConfig{Recipient: "merchant", ChainID: 196}, nil, probe)
	cfg := SessionRouteConfig{Amount: "10", Currency: "token"}
	header, err := m.SessionChallenge(context.Background(), cfg)
	if err != nil {
		t.Fatal(err)
	}
	auth := routeAuth(t, header)
	other := SessionRouteConfig{Amount: "5000", Currency: "token"}
	if _, err := m.VerifySessionForRoute(context.Background(), other, auth); err == nil {
		t.Fatal("accepted different route amount")
	}
	if probe.calls != 0 {
		t.Fatal("session verifier called for a route mismatch")
	}
}
