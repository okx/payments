package paymentrouter

import "fmt"

// ValidateRouteKeys panics if any route's config references an adapter Name
// that is not present in the protocols slice. Call this once at startup to
// catch misconfiguration early.
func ValidateRouteKeys(routes []RouteEntry, protocols []ProtocolAdapter) {
	known := make(map[string]bool, len(protocols))
	for _, p := range protocols {
		known[p.Name()] = true
	}
	for _, route := range routes {
		for key := range route.Config {
			if !known[key] {
				panic(fmt.Sprintf("paymentrouter: route %q references unknown adapter %q", route.Pattern, key))
			}
		}
	}
}
