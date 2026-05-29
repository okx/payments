package http

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestOKXAuthProvider_SignatureFormat(t *testing.T) {
	provider := &okxAuthProvider{
		apiKey:     "key",
		secretKey:  "secret",
		passphrase: "pass",
		baseURL:    "https://web3.okx.com",
		basePath:   "/api/v6/x402",
	}

	timestamp := "2026-03-30T12:00:00.000Z"
	method := "POST"
	path := "/api/v6/x402/verify"
	body := `{"test":"data"}`

	sign := provider.computeSignature(timestamp, method, path, body)

	// Manually compute expected signature
	prehash := timestamp + method + path + body
	mac := hmac.New(sha256.New, []byte("secret"))
	mac.Write([]byte(prehash))
	expected := base64.StdEncoding.EncodeToString(mac.Sum(nil))

	assert.Equal(t, expected, sign)
}

func TestOKXAuthProvider_EmptyBody(t *testing.T) {
	provider := &okxAuthProvider{
		apiKey:     "key",
		secretKey:  "secret",
		passphrase: "pass",
		basePath:   "/api/v6/x402",
	}

	timestamp := "2026-03-30T12:00:00.000Z"
	sign := provider.computeSignature(timestamp, "GET", "/api/v6/x402/supported", "")

	// Prehash for GET has no body
	prehash := timestamp + "GET" + "/api/v6/x402/supported"
	mac := hmac.New(sha256.New, []byte("secret"))
	mac.Write([]byte(prehash))
	expected := base64.StdEncoding.EncodeToString(mac.Sum(nil))

	assert.Equal(t, expected, sign)
}
