package protocol

import (
	"fmt"
	"time"
)

// ChallengeEcho is a copy of the challenge fields sent back in the Authorization header.
// Expires is an RFC3339 string on the wire; use ParseExpiresUnix() to get the uint64 value.
type ChallengeEcho struct {
	ID          string `json:"id"`
	Realm       string `json:"realm"`
	Method      string `json:"method"`
	Intent      string `json:"intent"`
	Request     string `json:"request"`
	Expires     string `json:"expires,omitempty"`
	Description string `json:"description,omitempty"`
	Digest      string `json:"digest,omitempty"`
	Opaque      string `json:"opaque,omitempty"`
}

// ParseExpiresUnix parses the Expires string (RFC3339 or unix) into a uint64 unix timestamp.
// Returns 0 if empty or unparseable.
func (e *ChallengeEcho) ParseExpiresUnix() uint64 {
	if e.Expires == "" {
		return 0
	}
	if t, err := time.Parse(time.RFC3339, e.Expires); err == nil {
		return uint64(t.Unix())
	}
	return 0
}

// PaymentChallenge represents a parsed WWW-Authenticate PaymentAuth challenge.
type PaymentChallenge struct {
	ID          string `json:"id"`
	Realm       string `json:"realm"`
	Method      string `json:"method"`
	Intent      string `json:"intent"`
	Request     string `json:"request"`
	Expires     uint64 `json:"expires,omitempty"`
	Description string `json:"description,omitempty"`
	Digest      string `json:"digest,omitempty"`
	Opaque      string `json:"opaque,omitempty"`
	SecretKey   string `json:"-"`
}

// NewPaymentChallenge creates a PaymentChallenge with the given fields. If SecretKey
// is set later via WithSecretKey before the ID is used the caller should recompute it;
// at construction time SecretKey is empty so ComputeChallengeID uses an empty key.
func NewPaymentChallenge(realm, method, intent, request string) *PaymentChallenge {
	c := &PaymentChallenge{
		Realm:   realm,
		Method:  method,
		Intent:  intent,
		Request: request,
	}
	c.ID = ComputeChallengeID("", realm, method, intent, request, 0, "", "")
	return c
}

// WithSecretKey sets the secret key and recomputes the challenge ID.
func (c *PaymentChallenge) WithSecretKey(key string) *PaymentChallenge {
	c.SecretKey = key
	c.ID = ComputeChallengeID(key, c.Realm, c.Method, c.Intent, c.Request, c.Expires, c.Digest, c.Opaque)
	return c
}

// WithExpires sets the expiration unix timestamp and recomputes the ID.
func (c *PaymentChallenge) WithExpires(expires uint64) *PaymentChallenge {
	c.Expires = expires
	c.ID = ComputeChallengeID(c.SecretKey, c.Realm, c.Method, c.Intent, c.Request, c.Expires, c.Digest, c.Opaque)
	return c
}

// WithDescription sets the human-readable description field.
func (c *PaymentChallenge) WithDescription(desc string) *PaymentChallenge {
	c.Description = desc
	return c
}

// WithDigest sets the digest field and recomputes the ID.
func (c *PaymentChallenge) WithDigest(digest string) *PaymentChallenge {
	c.Digest = digest
	c.ID = ComputeChallengeID(c.SecretKey, c.Realm, c.Method, c.Intent, c.Request, c.Expires, c.Digest, c.Opaque)
	return c
}

// WithOpaque sets the opaque field and recomputes the ID.
func (c *PaymentChallenge) WithOpaque(opaque string) *PaymentChallenge {
	c.Opaque = opaque
	c.ID = ComputeChallengeID(c.SecretKey, c.Realm, c.Method, c.Intent, c.Request, c.Expires, c.Digest, c.Opaque)
	return c
}

// EffectiveExpires returns Expires if non-zero, otherwise now + 5 minutes.
func (c *PaymentChallenge) EffectiveExpires() uint64 {
	if c.Expires != 0 {
		return c.Expires
	}
	return uint64(time.Now().Add(5 * time.Minute).Unix())
}

// IsExpired reports whether the challenge has passed its expiry time.
// If Expires is 0 (not set) the challenge is treated as non-expired.
func (c *PaymentChallenge) IsExpired() bool {
	if c.Expires == 0 {
		return false
	}
	return uint64(time.Now().Unix()) > c.Expires
}

// ExpiresRFC3339 returns Expires as an RFC3339 string, or empty if zero.
func (c *PaymentChallenge) ExpiresRFC3339() string {
	if c.Expires == 0 {
		return ""
	}
	return time.Unix(int64(c.Expires), 0).UTC().Format(time.RFC3339)
}

// ToEcho returns a ChallengeEcho with all exported fields copied from the challenge.
func (c *PaymentChallenge) ToEcho() *ChallengeEcho {
	return &ChallengeEcho{
		ID:          c.ID,
		Realm:       c.Realm,
		Method:      c.Method,
		Intent:      c.Intent,
		Request:     c.Request,
		Expires:     c.ExpiresRFC3339(),
		Description: c.Description,
		Digest:      c.Digest,
		Opaque:      c.Opaque,
	}
}

// ToHeader formats the challenge as a WWW-Authenticate header value.
func (c *PaymentChallenge) ToHeader() (string, error) {
	return FormatWWWAuthenticate(c)
}

// Verify checks that the credential's echo matches this challenge and that the
// challenge has not expired.
func (c *PaymentChallenge) Verify(cred *PaymentCredential) error {
	if cred == nil {
		return fmt.Errorf("credential is nil")
	}
	if cred.Echo == nil {
		return fmt.Errorf("credential echo is nil")
	}
	echo := cred.Echo
	if echo.ID != c.ID {
		return fmt.Errorf("echo id mismatch: got %q, want %q", echo.ID, c.ID)
	}
	if echo.Realm != c.Realm {
		return fmt.Errorf("echo realm mismatch: got %q, want %q", echo.Realm, c.Realm)
	}
	if echo.Method != c.Method {
		return fmt.Errorf("echo method mismatch: got %q, want %q", echo.Method, c.Method)
	}
	if echo.Intent != c.Intent {
		return fmt.Errorf("echo intent mismatch: got %q, want %q", echo.Intent, c.Intent)
	}
	if echo.Request != c.Request {
		return fmt.Errorf("echo request mismatch")
	}
	if c.IsExpired() {
		return fmt.Errorf("payment challenge has expired")
	}
	return nil
}

// ValidateForCharge returns an error if the challenge intent is not "charge".
func (c *PaymentChallenge) ValidateForCharge() error {
	if c.Intent != IntentCharge {
		return fmt.Errorf("challenge intent is %q, expected %q", c.Intent, IntentCharge)
	}
	return nil
}

// ValidateForSession returns an error if the challenge intent is not "session".
func (c *PaymentChallenge) ValidateForSession() error {
	if c.Intent != IntentSession {
		return fmt.Errorf("challenge intent is %q, expected %q", c.Intent, IntentSession)
	}
	return nil
}

// PaymentChallengeFromHeader parses a WWW-Authenticate header value into a
// PaymentChallenge.
func PaymentChallengeFromHeader(header string) (*PaymentChallenge, error) {
	return ParseWWWAuthenticate(header)
}

// PaymentChallengeFromResponse parses the WWW-Authenticate value from an HTTP
// response that should carry a 402 status code.
func PaymentChallengeFromResponse(status int, wwwAuth string) (*PaymentChallenge, error) {
	if status != 402 {
		return nil, fmt.Errorf("expected HTTP 402, got %d", status)
	}
	return ParseWWWAuthenticate(wwwAuth)
}
