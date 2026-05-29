package paymentrouter

import (
	"testing"
)

// --- parseRouteKey tests ---

func TestParseRouteKey_WithMethod(t *testing.T) {
	method, path := parseRouteKey("GET /api/weather")
	if method != "get" {
		t.Errorf("expected method 'get', got %q", method)
	}
	if path != "/api/weather" {
		t.Errorf("expected path '/api/weather', got %q", path)
	}
}

func TestParseRouteKey_WithoutMethod(t *testing.T) {
	method, path := parseRouteKey("/api/weather")
	if method != "*" {
		t.Errorf("expected method '*', got %q", method)
	}
	if path != "/api/weather" {
		t.Errorf("expected path '/api/weather', got %q", path)
	}
}

func TestParseRouteKey_LowercaseMethod(t *testing.T) {
	method, _ := parseRouteKey("POST /foo")
	if method != "post" {
		t.Errorf("expected 'post', got %q", method)
	}
}

func TestParseRouteKey_MixedCaseMethod(t *testing.T) {
	method, path := parseRouteKey("Delete /items/1")
	if method != "delete" {
		t.Errorf("expected 'delete', got %q", method)
	}
	if path != "/items/1" {
		t.Errorf("expected '/items/1', got %q", path)
	}
}

// --- normalizePath tests ---

func TestNormalizePath_TrailingSlash(t *testing.T) {
	got := normalizePath("/api/v1/")
	if got != "/api/v1" {
		t.Errorf("expected '/api/v1', got %q", got)
	}
}

func TestNormalizePath_DoubleSlash(t *testing.T) {
	got := normalizePath("/api//v1")
	if got != "/api/v1" {
		t.Errorf("expected '/api/v1', got %q", got)
	}
}

func TestNormalizePath_Query(t *testing.T) {
	got := normalizePath("/api/v1?foo=bar")
	if got != "/api/v1" {
		t.Errorf("expected '/api/v1', got %q", got)
	}
}

func TestNormalizePath_Fragment(t *testing.T) {
	got := normalizePath("/api/v1#section")
	if got != "/api/v1" {
		t.Errorf("expected '/api/v1', got %q", got)
	}
}

func TestNormalizePath_Root(t *testing.T) {
	got := normalizePath("/")
	if got != "/" {
		t.Errorf("expected '/', got %q", got)
	}
}

func TestNormalizePath_Empty(t *testing.T) {
	got := normalizePath("")
	if got != "/" {
		t.Errorf("expected '/', got %q", got)
	}
}

// --- Match tests ---

func makeRouter(patterns ...string) *CompiledRouter {
	entries := make([]RouteEntry, len(patterns))
	for i, p := range patterns {
		entries[i] = RouteEntry{Pattern: p, Config: RouteConfig{"key": p}}
	}
	return NewCompiledRouter(entries)
}

func TestMatch_Exact(t *testing.T) {
	cr := makeRouter("GET /api/weather")
	m := cr.Match("GET", "/api/weather")
	if m == nil {
		t.Fatal("expected a match")
	}
	if m.RouteKey != "GET /api/weather" {
		t.Errorf("unexpected route key %q", m.RouteKey)
	}
}

func TestMatch_NoMatch(t *testing.T) {
	cr := makeRouter("GET /api/weather")
	m := cr.Match("GET", "/api/other")
	if m != nil {
		t.Errorf("expected no match, got %+v", m)
	}
}

func TestMatch_WrongMethod(t *testing.T) {
	cr := makeRouter("GET /api/weather")
	m := cr.Match("POST", "/api/weather")
	if m != nil {
		t.Errorf("expected no match for wrong method, got %+v", m)
	}
}

func TestMatch_Wildcard(t *testing.T) {
	cr := makeRouter("/api/*")
	m := cr.Match("GET", "/api/anything/here")
	if m == nil {
		t.Fatal("expected wildcard match")
	}
}

func TestMatch_Param(t *testing.T) {
	cr := makeRouter("GET /users/:id")
	m := cr.Match("GET", "/users/42")
	if m == nil {
		t.Fatal("expected :param match")
	}
}

func TestMatch_ParamNoSlashCross(t *testing.T) {
	cr := makeRouter("GET /users/:id")
	// :id should not cross slash boundary
	m := cr.Match("GET", "/users/42/extra")
	if m != nil {
		t.Errorf("expected no match across slash boundary, got %+v", m)
	}
}

func TestMatch_DeclarationOrder(t *testing.T) {
	// First entry should win when both match.
	cr := NewCompiledRouter([]RouteEntry{
		{Pattern: "GET /api/weather", Config: RouteConfig{"winner": "first"}},
		{Pattern: "GET /api/*", Config: RouteConfig{"winner": "second"}},
	})
	m := cr.Match("GET", "/api/weather")
	if m == nil {
		t.Fatal("expected a match")
	}
	if m.Config["winner"] != "first" {
		t.Errorf("expected first entry to win, got config %v", m.Config)
	}
}

func TestMatch_WildcardMethod(t *testing.T) {
	cr := makeRouter("/api/weather")
	for _, method := range []string{"GET", "POST", "DELETE", "PATCH"} {
		m := cr.Match(method, "/api/weather")
		if m == nil {
			t.Errorf("expected wildcard method to match %s", method)
		}
	}
}

func TestMatch_NormalizesPath(t *testing.T) {
	cr := makeRouter("GET /api/v1")
	m := cr.Match("GET", "/api/v1/?foo=bar")
	if m == nil {
		t.Fatal("expected match after path normalization")
	}
}
