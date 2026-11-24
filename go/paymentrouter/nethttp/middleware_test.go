package nethttp_test

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"

	pr "github.com/okx/payments/go/paymentrouter"
	nhttp "github.com/okx/payments/go/paymentrouter/nethttp"
)

type mock struct {
	name      string
	priority  int
	detect    bool
	challenge http.Header
	chalErr   error
	handleErr error
	writeResp bool
}

func (m *mock) Name() string     { return m.name }
func (m *mock) Priority() int    { return m.priority }
func (m *mock) Detect(_ *http.Request) bool { return m.detect }
func (m *mock) GetChallenge(_ context.Context, _ *http.Request, _ any) (http.Header, error) {
	return m.challenge, m.chalErr
}
func (m *mock) Handle(w http.ResponseWriter, _ *http.Request, _ any) error {
	if m.handleErr != nil {
		w.WriteHeader(http.StatusUnauthorized)
		return m.handleErr
	}
	if m.writeResp {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte(`{"managed":true}`)) //nolint:errcheck
	}
	return nil
}

var okHandler = http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
	w.WriteHeader(http.StatusOK)
})

func TestFor_NoDetect_Returns402(t *testing.T) {
	challengeHeaders := http.Header{
		"X-Payment-Challenge": []string{"token123"},
	}
	gate := nhttp.New([]pr.ProtocolAdapter{
		&mock{name: "proto", priority: 1, challenge: challengeHeaders},
	})
	handler := gate.For(pr.RouteConfig{"proto": struct{}{}})(okHandler)

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)

	if rec.Code != http.StatusPaymentRequired {
		t.Fatalf("expected 402, got %d", rec.Code)
	}
	if got := rec.Header().Get("X-Payment-Challenge"); got != "token123" {
		t.Fatalf("expected challenge header %q, got %q", "token123", got)
	}
}

func TestFor_DetectHit_HandleSuccess(t *testing.T) {
	gate := nhttp.New([]pr.ProtocolAdapter{
		&mock{name: "proto", priority: 1, detect: true},
	})
	handler := gate.For(pr.RouteConfig{"proto": struct{}{}})(okHandler)

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", rec.Code)
	}
}

func TestFor_DetectHit_HandleError(t *testing.T) {
	handleErr := errors.New("handle failed")
	var gotErr error
	var gotPhase, gotProto string

	gate := nhttp.New(
		[]pr.ProtocolAdapter{&mock{name: "proto", priority: 1, detect: true, handleErr: handleErr}},
		nhttp.WithOnError(func(err error, phase, protocol string) {
			gotErr = err
			gotPhase = phase
			gotProto = protocol
		}),
	)
	handler := gate.For(pr.RouteConfig{"proto": struct{}{}})(okHandler)

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)

	if rec.Code == http.StatusOK {
		t.Fatal("expected non-200 when handle returns error")
	}
	if gotErr != handleErr {
		t.Fatalf("expected OnError called with handleErr, got %v", gotErr)
	}
	if gotPhase != pr.PhaseHandle {
		t.Fatalf("expected phase %q, got %q", pr.PhaseHandle, gotPhase)
	}
	if gotProto != "proto" {
		t.Fatalf("expected proto %q, got %q", "proto", gotProto)
	}
}

func TestFor_DetectHit_WrittenAborts(t *testing.T) {
	gate := nhttp.New([]pr.ProtocolAdapter{
		&mock{name: "proto", priority: 1, detect: true, writeResp: true},
	})
	handler := gate.For(pr.RouteConfig{"proto": struct{}{}})(okHandler)

	req := httptest.NewRequest(http.MethodGet, "/pay", nil)
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200 (from managed response), got %d", rec.Code)
	}
	if rec.Body.String() != `{"managed":true}` {
		t.Fatalf("expected managed response body, got %q", rec.Body.String())
	}
}
