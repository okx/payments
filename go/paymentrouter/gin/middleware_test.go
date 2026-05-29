package gin_test

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"

	ginfw "github.com/gin-gonic/gin"
	pr "github.com/okx/payments/go/paymentrouter"
	prgin "github.com/okx/payments/go/paymentrouter/gin"
)

func init() { ginfw.SetMode(ginfw.TestMode) }

type mock struct {
	name      string
	priority  int
	detect    bool
	challenge http.Header
	chalErr   error
	handleErr error
	writeResp bool // if true, Handle writes a response (simulates session management)
}

func (m *mock) Name() string              { return m.name }
func (m *mock) Priority() int             { return m.priority }
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

func serve(mw ginfw.HandlerFunc, req *http.Request) *httptest.ResponseRecorder {
	w := httptest.NewRecorder()
	r := ginfw.New()
	r.GET("/api/weather", mw, func(c *ginfw.Context) { c.JSON(200, ginfw.H{"ok": true}) })
	r.ServeHTTP(w, req)
	return w
}

func TestFor_NoDetect_Returns402(t *testing.T) {
	h1 := make(http.Header)
	h1.Set("WWW-Authenticate", "mpp challenge")
	h2 := make(http.Header)
	h2.Set("WWW-Authenticate", "x402 challenge")

	gate := prgin.New([]pr.ProtocolAdapter{
		&mock{name: "a", challenge: h1},
		&mock{name: "b", challenge: h2},
	})
	mw := gate.For(pr.RouteConfig{"a": struct{}{}, "b": struct{}{}})
	w := serve(mw, httptest.NewRequest("GET", "/api/weather", nil))
	if w.Code != 402 {
		t.Errorf("got %d", w.Code)
	}
	if len(w.Header().Values("WWW-Authenticate")) != 2 {
		t.Errorf("expected 2 WWW-Authenticate values, got %v", w.Header().Values("WWW-Authenticate"))
	}
}

func TestFor_DetectHit_HandleSuccess(t *testing.T) {
	gate := prgin.New([]pr.ProtocolAdapter{
		&mock{name: "m", detect: true},
	})
	mw := gate.For(pr.RouteConfig{"m": struct{}{}})
	w := serve(mw, httptest.NewRequest("GET", "/api/weather", nil))
	if w.Code != 200 {
		t.Errorf("got %d", w.Code)
	}
}

func TestFor_DetectHit_HandleError(t *testing.T) {
	gate := prgin.New([]pr.ProtocolAdapter{
		&mock{name: "m", detect: true, handleErr: errors.New("bad")},
	})
	mw := gate.For(pr.RouteConfig{"m": struct{}{}})
	w := serve(mw, httptest.NewRequest("GET", "/api/weather", nil))
	if w.Code == 200 {
		t.Error("should not be 200 on handle error")
	}
}

func TestFor_DetectHit_WrittenAborts(t *testing.T) {
	gate := prgin.New([]pr.ProtocolAdapter{
		&mock{name: "m", detect: true, writeResp: true},
	})
	mw := gate.For(pr.RouteConfig{"m": struct{}{}})
	w := serve(mw, httptest.NewRequest("GET", "/api/weather", nil))
	if w.Code != 200 {
		t.Errorf("got %d, want 200 (from managed response)", w.Code)
	}
	if w.Body.String() != `{"managed":true}` {
		t.Errorf("expected managed response body, got %q", w.Body.String())
	}
}

func TestFor_OnError_Called(t *testing.T) {
	var gotErr error
	var gotPhase, gotProto string
	gate := prgin.New(
		[]pr.ProtocolAdapter{&mock{name: "m", detect: true, handleErr: errors.New("fail")}},
		prgin.WithOnError(func(err error, phase, protocol string) {
			gotErr = err
			gotPhase = phase
			gotProto = protocol
		}),
	)
	mw := gate.For(pr.RouteConfig{"m": struct{}{}})
	serve(mw, httptest.NewRequest("GET", "/api/weather", nil))

	if gotErr == nil {
		t.Fatal("expected OnError to be called")
	}
	if gotPhase != pr.PhaseHandle {
		t.Errorf("phase = %q, want %q", gotPhase, pr.PhaseHandle)
	}
	if gotProto != "m" {
		t.Errorf("proto = %q, want %q", gotProto, "m")
	}
}
