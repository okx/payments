package paymentrouter

// RouteConfig maps adapter name to adapter-specific config. Each adapter
// type-asserts to its own config struct in GetChallenge/Handle.
type RouteConfig map[string]any

type RouteEntry struct {
	Pattern string
	Config  RouteConfig
}

type Config struct {
	Routes    []RouteEntry
	Protocols []ProtocolAdapter
	OnError   func(err error, phase, protocol string)
}

const (
	PhaseDetect    = "detect"
	PhaseChallenge = "challenge"
	PhaseHandle    = "handle"
)
