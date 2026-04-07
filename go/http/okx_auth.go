package http

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"time"
)

// OKXAuthConfig holds OKX API credentials.
type OKXAuthConfig struct {
	APIKey     string
	SecretKey  string
	Passphrase string
	BaseURL    string // default: "https://web3.okx.com"
	BasePath   string // e.g. "/api/v6/x402"
}

type okxAuthProvider struct {
	apiKey     string
	secretKey  string
	passphrase string
	baseURL    string
	basePath   string
}

func (p *okxAuthProvider) computeSignature(timestamp, method, path, body string) string {
	prehash := timestamp + method + path + body
	mac := hmac.New(sha256.New, []byte(p.secretKey))
	mac.Write([]byte(prehash))
	return base64.StdEncoding.EncodeToString(mac.Sum(nil))
}

func (p *okxAuthProvider) createHeaders(method, path, body string) map[string]string {
	timestamp := time.Now().UTC().Format("2006-01-02T15:04:05.000Z")
	sign := p.computeSignature(timestamp, method, path, body)

	return map[string]string{
		"OK-ACCESS-KEY":        p.apiKey,
		"OK-ACCESS-SIGN":       sign,
		"OK-ACCESS-TIMESTAMP":  timestamp,
		"OK-ACCESS-PASSPHRASE": p.passphrase,
	}
}
