package adapters

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	x402http "github.com/okx/payments/go/x402/http"
)

func newTestAdapter() *X402Adapter {
	server := x402http.Newx402HTTPResourceServer(x402http.RoutesConfig{})
	return NewX402Adapter(server)
}

func TestX402Adapter_Name(t *testing.T) {
	a := newTestAdapter()
	assert.Equal(t, "x402", a.Name())
}

func TestX402Adapter_Priority(t *testing.T) {
	a := newTestAdapter()
	assert.Equal(t, 20, a.Priority())
}

func TestX402Adapter_Detect(t *testing.T) {
	a := newTestAdapter()

	tests := []struct {
		name   string
		header string
		value  string
		want   bool
	}{
		{"PAYMENT-SIGNATURE present", "PAYMENT-SIGNATURE", "abc123", true},
		{"X-Payment present", "X-Payment", "abc123", true},
		{"no payment header", "", "", false},
		{"unrelated header only", "Authorization", "Bearer token", false},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			req := httptest.NewRequest(http.MethodGet, "/resource", nil)
			if tc.header != "" {
				req.Header.Set(tc.header, tc.value)
			}
			got := a.Detect(req)
			assert.Equal(t, tc.want, got)
		})
	}
}

func TestNewX402Adapter_NilServerPanics(t *testing.T) {
	// Passing nil should not panic at construction time (panics only at use time)
	require.NotPanics(t, func() {
		_ = NewX402Adapter(nil)
	})
}

func TestX402Adapter_ImplementsProtocolAdapter(t *testing.T) {
	// Compile-time assertion is already in x402_adapter.go via var _ check;
	// this test documents the intent explicitly.
	a := newTestAdapter()
	require.NotNil(t, a)
}
