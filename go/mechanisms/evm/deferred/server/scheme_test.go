package server

import (
	"testing"
)

func TestAggrDeferredEvmScheme_Scheme(t *testing.T) {
	scheme := NewAggrDeferredEvmScheme()
	if scheme.Scheme() != "aggr_deferred" {
		t.Errorf("expected scheme 'aggr_deferred', got '%s'", scheme.Scheme())
	}
}

func TestAggrDeferredEvmScheme_ParsePrice(t *testing.T) {
	scheme := NewAggrDeferredEvmScheme()
	result, err := scheme.ParsePrice("$0.01", "eip155:196")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.Amount == "" {
		t.Error("expected non-empty amount")
	}
}
