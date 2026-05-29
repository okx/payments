package paymentrouter

import (
	"net/http"
	"sort"
)

// Detect returns the first ProtocolAdapter (sorted by ascending Priority) whose
// Detect method returns true for the request. Panics in Detect are recovered
// and treated as a miss. Returns nil if no adapter matches.
func Detect(adapters []ProtocolAdapter, r *http.Request) ProtocolAdapter {
	sorted := make([]ProtocolAdapter, len(adapters))
	copy(sorted, adapters)
	sort.SliceStable(sorted, func(i, j int) bool {
		return sorted[i].Priority() < sorted[j].Priority()
	})

	for _, a := range sorted {
		if safeDetect(a, r) {
			return a
		}
	}
	return nil
}

// safeDetect calls a.Detect(r) and returns false if a panic occurs.
func safeDetect(a ProtocolAdapter, r *http.Request) (result bool) {
	defer func() {
		if rec := recover(); rec != nil {
			result = false
		}
	}()
	return a.Detect(r)
}
