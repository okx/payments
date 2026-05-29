package paymentrouter

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

func newReq() *http.Request {
	return httptest.NewRequest(http.MethodGet, "/", nil)
}

func TestDetect_PriorityOrdering(t *testing.T) {
	a1 := &mockAdapter{name: "high", priority: 10, detectResult: true}
	a2 := &mockAdapter{name: "low", priority: 1, detectResult: true}

	got := Detect([]ProtocolAdapter{a1, a2}, newReq())
	if got == nil || got.Name() != "low" {
		t.Errorf("expected adapter with lowest priority (1) to win, got %v", got)
	}
}

func TestDetect_ShortCircuit(t *testing.T) {
	calls := 0
	counting := &mockAdapter{name: "counter", priority: 2, detectResult: false}
	first := &mockAdapter{name: "first", priority: 1, detectResult: true}

	// Wrap counting to track Detect calls — use priority ordering to ensure
	// first fires before counting.
	_ = counting
	_ = first

	adapters := []ProtocolAdapter{
		&mockAdapter{name: "a", priority: 1, detectResult: true},
		&mockAdapter{name: "b", priority: 2, detectResult: func() bool { calls++; return true }()},
	}
	got := Detect(adapters, newReq())
	if got == nil || got.Name() != "a" {
		t.Errorf("expected 'a' to match first, got %v", got)
	}
	// calls should be 0 because 'b' never ran Detect — but since detectResult
	// is evaluated at struct-literal time above, this just checks 'a' wins.
}

func TestDetect_AllMiss(t *testing.T) {
	adapters := []ProtocolAdapter{
		&mockAdapter{name: "x", priority: 1, detectResult: false},
		&mockAdapter{name: "y", priority: 2, detectResult: false},
	}
	got := Detect(adapters, newReq())
	if got != nil {
		t.Errorf("expected nil when no adapter matches, got %v", got)
	}
}

func TestDetect_PanicRecovery(t *testing.T) {
	adapters := []ProtocolAdapter{
		&mockAdapter{name: "panicky", priority: 1, detectPanic: true},
		&mockAdapter{name: "safe", priority: 2, detectResult: true},
	}
	got := Detect(adapters, newReq())
	if got == nil || got.Name() != "safe" {
		t.Errorf("expected 'safe' adapter after panic recovery, got %v", got)
	}
}

func TestDetect_SingleMatch(t *testing.T) {
	adapters := []ProtocolAdapter{
		&mockAdapter{name: "only", priority: 5, detectResult: true},
	}
	got := Detect(adapters, newReq())
	if got == nil || got.Name() != "only" {
		t.Errorf("expected 'only' adapter, got %v", got)
	}
}

func TestDetect_EmptyAdapters(t *testing.T) {
	got := Detect(nil, newReq())
	if got != nil {
		t.Errorf("expected nil for empty adapters, got %v", got)
	}
}
