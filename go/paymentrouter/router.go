package paymentrouter

import (
	"net/url"
	"regexp"
	"strings"
)

// parseRouteKey splits a route key like "GET /api/path" into (method, path).
// If no method prefix is present, method defaults to "*".
func parseRouteKey(key string) (method, path string) {
	parts := strings.SplitN(key, " ", 2)
	if len(parts) == 2 {
		return strings.ToLower(parts[0]), parts[1]
	}
	return "*", key
}

// compilePath converts a route pattern into a regexp.
// Rules:
//   - "*" anywhere → .*? (lazy wildcard)
//   - :param segment → [^/]+
//   - everything else → regexp.QuoteMeta (literal)
func compilePath(pattern string) *regexp.Regexp {
	segments := strings.Split(pattern, "/")
	var sb strings.Builder
	sb.WriteString("^")
	for i, seg := range segments {
		if i > 0 {
			sb.WriteString("/")
		}
		switch {
		case seg == "*":
			sb.WriteString(".*?")
		case strings.HasPrefix(seg, ":"):
			sb.WriteString("[^/]+")
		default:
			sb.WriteString(regexp.QuoteMeta(seg))
		}
	}
	sb.WriteString("$")
	return regexp.MustCompile(sb.String())
}

// normalizePath strips query/fragment, collapses double slashes, and removes
// the trailing slash (except for root "/").
func normalizePath(rawPath string) string {
	// Strip query and fragment.
	if i := strings.IndexAny(rawPath, "?#"); i >= 0 {
		rawPath = rawPath[:i]
	}
	// URL-unescape best-effort; ignore errors.
	if unescaped, err := url.PathUnescape(rawPath); err == nil {
		rawPath = unescaped
	}
	// Collapse consecutive slashes.
	for strings.Contains(rawPath, "//") {
		rawPath = strings.ReplaceAll(rawPath, "//", "/")
	}
	// Strip trailing slash, but keep root.
	if len(rawPath) > 1 && strings.HasSuffix(rawPath, "/") {
		rawPath = rawPath[:len(rawPath)-1]
	}
	if rawPath == "" {
		rawPath = "/"
	}
	return rawPath
}

type RouteMatch struct {
	RouteKey string
	Config   RouteConfig
}

type compiledEntry struct {
	method   string // lowercase or "*"
	re       *regexp.Regexp
	routeKey string
	config   RouteConfig
}

type CompiledRouter struct {
	entries []compiledEntry
}

// NewCompiledRouter builds a CompiledRouter from a slice of RouteEntry values.
// Declaration order is preserved for precedence.
func NewCompiledRouter(routes []RouteEntry) *CompiledRouter {
	entries := make([]compiledEntry, 0, len(routes))
	for _, r := range routes {
		method, path := parseRouteKey(r.Pattern)
		entries = append(entries, compiledEntry{
			method:   method,
			re:       compilePath(path),
			routeKey: r.Pattern,
			config:   r.Config,
		})
	}
	return &CompiledRouter{entries: entries}
}

// Match returns the first RouteEntry whose method and path pattern match.
// method should already be lowercase. path is normalized internally.
func (cr *CompiledRouter) Match(method, path string) *RouteMatch {
	path = normalizePath(path)
	method = strings.ToLower(method)
	for _, e := range cr.entries {
		if e.method != "*" && e.method != method {
			continue
		}
		if e.re.MatchString(path) {
			return &RouteMatch{RouteKey: e.routeKey, Config: e.config}
		}
	}
	return nil
}
