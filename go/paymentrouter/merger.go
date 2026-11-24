package paymentrouter

import (
	"context"
	"net/http"
	"sync"
)

// MergeChallenges calls GetChallenge concurrently for every adapter that has
// an entry in routeCfg. Results are merged with http.Header.Add (not Set) so
// that multi-value headers (e.g. WWW-Authenticate) accumulate. A single
// failure invokes onError and is skipped; nil headers are skipped silently.
func MergeChallenges(
	ctx context.Context,
	adapters []ProtocolAdapter,
	r *http.Request,
	routeCfg RouteConfig,
	onError func(err error, protocol string),
) http.Header {
	// Filter to adapters that have config in this route.
	type result struct {
		h   http.Header
		err error
		a   ProtocolAdapter
	}

	filtered := make([]ProtocolAdapter, 0, len(adapters))
	for _, a := range adapters {
		if _, ok := routeCfg[a.Name()]; ok {
			filtered = append(filtered, a)
		}
	}

	results := make([]result, len(filtered))
	var wg sync.WaitGroup
	wg.Add(len(filtered))

	for i, a := range filtered {
		i, a := i, a
		go func() {
			defer wg.Done()
			cfg := routeCfg[a.Name()]
			h, err := a.GetChallenge(ctx, r, cfg)
			results[i] = result{h: h, err: err, a: a}
		}()
	}
	wg.Wait()

	merged := http.Header{}
	for _, res := range results {
		if res.err != nil {
			if onError != nil {
				onError(res.err, res.a.Name())
			}
			continue
		}
		if res.h == nil {
			continue
		}
		for key, vals := range res.h {
			for _, v := range vals {
				merged.Add(key, v)
			}
		}
	}
	return merged
}
