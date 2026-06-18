package x402

import "testing"

// TestResourceMatches_BackCompat_EmptyAllowlist verifies that, when AcceptedDomains
// is nil/empty, the helper falls back to strict full-URL equality.
func TestResourceMatches_BackCompat_EmptyAllowlist(t *testing.T) {
	t.Run("identical URLs match (nil allowlist)", func(t *testing.T) {
		if !ResourceMatches(
			"https://api.example.com/v1/foo",
			"https://api.example.com/v1/foo",
			nil,
		) {
			t.Fatal("expected identical URLs to match with nil allowlist")
		}
	})

	t.Run("identical URLs match (empty allowlist)", func(t *testing.T) {
		if !ResourceMatches(
			"https://api.example.com/v1/foo",
			"https://api.example.com/v1/foo",
			[]string{},
		) {
			t.Fatal("expected identical URLs to match with empty allowlist")
		}
	})

	t.Run("mismatched URLs do not match (nil allowlist)", func(t *testing.T) {
		if ResourceMatches(
			"https://api.example.com/v1/foo",
			"https://api.example.com/v1/bar",
			nil,
		) {
			t.Fatal("expected mismatched URLs not to match with nil allowlist")
		}
	})

	t.Run("different host without allowlist does not match", func(t *testing.T) {
		if ResourceMatches(
			"https://web3.okx.com/api/foo",
			"https://web3.ouyich.biz/api/foo",
			nil,
		) {
			t.Fatal("expected different hosts not to match with nil allowlist")
		}
	})
}

// TestResourceMatches_ProxyRewriteHappyPath verifies that a request rewritten by a
// reverse proxy / CDN to a different host still matches when the payload host is
// in the allowlist and the paths line up.
func TestResourceMatches_ProxyRewriteHappyPath(t *testing.T) {
	if !ResourceMatches(
		"https://web3.okx.com/api/foo",
		"https://web3.ouyich.biz/api/foo",
		[]string{"web3.okx.com"},
	) {
		t.Fatal("expected proxy-rewritten request to match with allowlisted payload host")
	}
}

// TestResourceMatches_HostNotInAllowlist verifies that a payload whose host is
// not in the allowlist is rejected.
func TestResourceMatches_HostNotInAllowlist(t *testing.T) {
	if ResourceMatches(
		"https://evil.example.com/api/foo",
		"https://web3.ouyich.biz/api/foo",
		[]string{"web3.okx.com"},
	) {
		t.Fatal("expected payload host outside allowlist to be rejected")
	}
}

// TestResourceMatches_PathMismatch verifies that even when the payload host is
// allowlisted, a path mismatch is rejected.
func TestResourceMatches_PathMismatch(t *testing.T) {
	if ResourceMatches(
		"https://web3.okx.com/api/foo",
		"https://web3.ouyich.biz/api/bar",
		[]string{"web3.okx.com"},
	) {
		t.Fatal("expected path mismatch to be rejected even with host in allowlist")
	}
}

// TestResourceMatches_CaseInsensitiveHost verifies that allowlist comparison is
// case-insensitive on the host name.
func TestResourceMatches_CaseInsensitiveHost(t *testing.T) {
	if !ResourceMatches(
		"https://web3.okx.com/api/foo",
		"https://web3.ouyich.biz/api/foo",
		[]string{"Web3.OKX.com"},
	) {
		t.Fatal("expected case-insensitive host match")
	}

	if !ResourceMatches(
		"https://WEB3.OKX.COM/api/foo",
		"https://web3.ouyich.biz/api/foo",
		[]string{"web3.okx.com"},
	) {
		t.Fatal("expected case-insensitive match when payload host is uppercase")
	}
}

// TestResourceMatches_MalformedPayloadURL verifies that a malformed payload URL
// returns false without panicking.
func TestResourceMatches_MalformedPayloadURL(t *testing.T) {
	defer func() {
		if r := recover(); r != nil {
			t.Fatalf("ResourceMatches panicked on malformed URL: %v", r)
		}
	}()

	// url.Parse is very permissive, so use a control character that it does reject.
	malformed := "http://exa\x7fmple.com/api"
	if ResourceMatches(
		malformed,
		"https://web3.ouyich.biz/api",
		[]string{"web3.okx.com"},
	) {
		t.Fatal("expected malformed payload URL to be rejected")
	}
}
