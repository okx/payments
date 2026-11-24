package paymentrouter

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestMergeChallenges_MergesHeaders(t *testing.T) {
	h1 := http.Header{"X-Proto": []string{"proto1"}}
	h2 := http.Header{"X-Proto": []string{"proto2"}}

	adapters := []ProtocolAdapter{
		&mockAdapter{name: "a1", challengeH: h1},
		&mockAdapter{name: "a2", challengeH: h2},
	}
	cfg := RouteConfig{
		"a1": struct{}{},
		"a2": struct{}{},
	}

	r := httptest.NewRequest(http.MethodGet, "/", nil)
	merged := MergeChallenges(context.Background(), adapters, r, cfg, nil)

	vals := merged["X-Proto"]
	if len(vals) != 2 {
		t.Errorf("expected 2 X-Proto values, got %v", vals)
	}
}

func TestMergeChallenges_SkipsMissingConfig(t *testing.T) {
	h := http.Header{"X-Present": []string{"yes"}}
	adapters := []ProtocolAdapter{
		&mockAdapter{name: "present", challengeH: h},
		&mockAdapter{name: "absent", challengeH: http.Header{"X-Absent": []string{"no"}}},
	}
	cfg := RouteConfig{
		"present": struct{}{},
		// "absent" intentionally not in cfg
	}

	r := httptest.NewRequest(http.MethodGet, "/", nil)
	merged := MergeChallenges(context.Background(), adapters, r, cfg, nil)

	if merged.Get("X-Absent") != "" {
		t.Error("expected 'absent' adapter to be skipped")
	}
	if merged.Get("X-Present") != "yes" {
		t.Error("expected 'present' adapter headers to be included")
	}
}

func TestMergeChallenges_ErrorDoesNotBlock(t *testing.T) {
	errored := errors.New("challenge failed")
	adapters := []ProtocolAdapter{
		&mockAdapter{name: "bad", challengeErr: errored},
		&mockAdapter{name: "good", challengeH: http.Header{"X-Ok": []string{"ok"}}},
	}
	cfg := RouteConfig{
		"bad":  struct{}{},
		"good": struct{}{},
	}

	var gotErr error
	var gotProto string
	r := httptest.NewRequest(http.MethodGet, "/", nil)
	merged := MergeChallenges(context.Background(), adapters, r, cfg, func(err error, protocol string) {
		gotErr = err
		gotProto = protocol
	})

	if gotErr == nil {
		t.Error("expected onError to be called")
	}
	if gotProto != "bad" {
		t.Errorf("expected protocol 'bad', got %q", gotProto)
	}
	if merged.Get("X-Ok") != "ok" {
		t.Error("expected 'good' adapter headers despite 'bad' error")
	}
}

func TestMergeChallenges_NilReturnSkipped(t *testing.T) {
	adapters := []ProtocolAdapter{
		&mockAdapter{name: "nilheader", challengeH: nil},
		&mockAdapter{name: "realheader", challengeH: http.Header{"X-Real": []string{"v"}}},
	}
	cfg := RouteConfig{
		"nilheader":  struct{}{},
		"realheader": struct{}{},
	}

	r := httptest.NewRequest(http.MethodGet, "/", nil)
	merged := MergeChallenges(context.Background(), adapters, r, cfg, nil)

	if merged.Get("X-Real") != "v" {
		t.Error("expected real header to be present")
	}
	// Nil header from 'nilheader' adapter should not cause a panic and result
	// in an empty contribution.
}

func TestMergeChallenges_MultiLineWWWAuthenticate(t *testing.T) {
	// Both adapters contribute a WWW-Authenticate value; they must both appear.
	adapters := []ProtocolAdapter{
		&mockAdapter{name: "proto1", challengeH: http.Header{"Www-Authenticate": []string{"Bearer realm=proto1"}}},
		&mockAdapter{name: "proto2", challengeH: http.Header{"Www-Authenticate": []string{"X402 realm=proto2"}}},
	}
	cfg := RouteConfig{
		"proto1": struct{}{},
		"proto2": struct{}{},
	}

	r := httptest.NewRequest(http.MethodGet, "/", nil)
	merged := MergeChallenges(context.Background(), adapters, r, cfg, nil)

	vals := merged["Www-Authenticate"]
	if len(vals) != 2 {
		t.Errorf("expected 2 WWW-Authenticate values, got %v", vals)
	}
}

func TestMergeChallenges_EmptyAdapters(t *testing.T) {
	r := httptest.NewRequest(http.MethodGet, "/", nil)
	merged := MergeChallenges(context.Background(), nil, r, RouteConfig{}, nil)
	if len(merged) != 0 {
		t.Errorf("expected empty merged headers, got %v", merged)
	}
}
