package http

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/okx/payments/go/x402"
)

// ============================================================================
// OKX Facilitator Client — talks to OKX API at /api/v6/pay/x402/*
// ============================================================================

// OKXFacilitatorConfig configures the OKX facilitator client.
type OKXFacilitatorConfig struct {
	Auth       OKXAuthConfig
	BaseURL    string        // Default: "https://web3.okx.com"
	SyncSettle *bool         // Default: true
	HTTPClient *http.Client
	Timeout    time.Duration
}

// OKXFacilitatorClient implements FacilitatorClient for the OKX API.
type OKXFacilitatorClient struct {
	auth       *okxAuthProvider
	baseURL    string
	syncSettle bool
	httpClient *http.Client
}

const (
	okxDefaultBaseURL = "https://web3.okx.com"
	okxBasePath       = "/api/v6/pay/x402"
)

// okxEnvelope is the OKX response envelope.
// code is a number (0 = success, non-zero = error), data is a direct object (not array).
type okxEnvelope struct {
	Code         int             `json:"code"`
	Msg          string          `json:"msg,omitempty"`
	ErrorCode    string          `json:"error_code,omitempty"`
	ErrorMessage string          `json:"error_message,omitempty"`
	Data         json.RawMessage `json:"data"`
}

// NewOKXFacilitatorClient creates a new OKX facilitator client.
// Returns an error if required credentials (APIKey, SecretKey, Passphrase) are missing.
func NewOKXFacilitatorClient(config *OKXFacilitatorConfig) (*OKXFacilitatorClient, error) {
	if config == nil {
		return nil, fmt.Errorf("OKX facilitator config is required")
	}

	if config.Auth.APIKey == "" || config.Auth.SecretKey == "" || config.Auth.Passphrase == "" {
		return nil, fmt.Errorf("OKX API credentials (APIKey, SecretKey, Passphrase) are all required")
	}

	baseURL := config.BaseURL
	if baseURL == "" {
		baseURL = okxDefaultBaseURL
	}
	baseURL = strings.TrimRight(baseURL, "/")

	syncSettle := true
	if config.SyncSettle != nil {
		syncSettle = *config.SyncSettle
	}

	httpClient := config.HTTPClient
	if httpClient == nil {
		timeout := config.Timeout
		if timeout == 0 {
			timeout = 30 * time.Second
		}
		httpClient = &http.Client{Timeout: timeout}
	}

	return &OKXFacilitatorClient{
		auth: &okxAuthProvider{
			apiKey:     config.Auth.APIKey,
			secretKey:  config.Auth.SecretKey,
			passphrase: config.Auth.Passphrase,
			baseURL:    baseURL,
			basePath:   okxBasePath,
		},
		baseURL:    baseURL,
		syncSettle: syncSettle,
		httpClient: httpClient,
	}, nil
}

// ============================================================================
// FacilitatorClient interface implementation
// ============================================================================

// GetSupported returns supported payment kinds from the OKX API.
func (c *OKXFacilitatorClient) GetSupported(ctx context.Context) (x402.SupportedResponse, error) {
	data, err := c.doRequest(ctx, "GET", "/supported", nil)
	if err != nil {
		return x402.SupportedResponse{}, err
	}

	var result x402.SupportedResponse
	if err := json.Unmarshal(data, &result); err != nil {
		return x402.SupportedResponse{}, fmt.Errorf("failed to parse supported response: %w", err)
	}

	return result, nil
}

// Verify checks if a payment is valid via the OKX API.
func (c *OKXFacilitatorClient) Verify(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.VerifyResponse, error) {
	body, err := buildBody(payloadBytes, requirementsBytes, false, false)
	if err != nil {
		return nil, fmt.Errorf("failed to build verify request: %w", err)
	}

	data, err := c.doRequest(ctx, "POST", "/verify", body)
	if err != nil {
		return nil, err
	}

	var result x402.VerifyResponse
	if err := json.Unmarshal(data, &result); err != nil {
		return nil, fmt.Errorf("failed to parse verify response: %w", err)
	}

	return &result, nil
}

// Settle executes a payment via the OKX API.
func (c *OKXFacilitatorClient) Settle(ctx context.Context, payloadBytes []byte, requirementsBytes []byte) (*x402.SettleResponse, error) {
	body, err := buildBody(payloadBytes, requirementsBytes, true, c.syncSettle)
	if err != nil {
		return nil, fmt.Errorf("failed to build settle request: %w", err)
	}

	data, err := c.doRequest(ctx, "POST", "/settle", body)
	if err != nil {
		return nil, err
	}

	var result x402.SettleResponse
	if err := json.Unmarshal(data, &result); err != nil {
		return nil, fmt.Errorf("failed to parse settle response: %w", err)
	}

	return &result, nil
}

// GetSettleStatus queries on-chain settlement status by transaction hash.
// Implements the SettleStatusChecker interface for timeout recovery polling.
func (c *OKXFacilitatorClient) GetSettleStatus(ctx context.Context, txHash string) (*x402.SettleStatusResponse, error) {
	endpoint := "/settle/status?txHash=" + txHash
	data, err := c.doRequest(ctx, "GET", endpoint, nil)
	if err != nil {
		return nil, err
	}

	var result x402.SettleStatusResponse
	if err := json.Unmarshal(data, &result); err != nil {
		return nil, fmt.Errorf("failed to parse settle status response: %w", err)
	}

	return &result, nil
}

// ============================================================================
// Internal helpers
// ============================================================================

// doRequest performs an authenticated HTTP request to the OKX API and unwraps the envelope.
func (c *OKXFacilitatorClient) doRequest(ctx context.Context, method, endpoint string, body []byte) (json.RawMessage, error) {
	path := okxBasePath + endpoint
	url := c.baseURL + path

	var bodyReader io.Reader
	bodyStr := ""
	if body != nil {
		bodyReader = bytes.NewReader(body)
		bodyStr = string(body)
	}

	req, err := http.NewRequestWithContext(ctx, method, url, bodyReader)
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("Content-Type", "application/json")

	headers := c.auth.createHeaders(method, path, bodyStr)
	for k, v := range headers {
		req.Header.Set(k, v)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("request to %s failed: %w", endpoint, err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("failed to read response from %s: %w", endpoint, err)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("OKX API error from %s: HTTP %d: %s", endpoint, resp.StatusCode, string(respBody))
	}

	return unwrapEnvelope(respBody, endpoint)
}

// unwrapEnvelope parses an OKX {code, data, msg} envelope and returns data.
// Handles both enveloped responses ({code: 0, data: {...}}) and direct responses
// (no envelope, body IS the data) — mirrors TS logic: `json.data ?? json`.
func unwrapEnvelope(body []byte, endpoint string) (json.RawMessage, error) {
	var env okxEnvelope
	if err := json.Unmarshal(body, &env); err != nil {
		return nil, fmt.Errorf("failed to parse OKX response from %s: %w", endpoint, err)
	}

	// Check code first — a non-zero code is always an error, regardless of data presence
	if env.Code != 0 {
		msg := env.Msg
		if msg == "" {
			msg = env.ErrorMessage
		}
		if msg == "" {
			msg = "unknown error"
		}
		return nil, fmt.Errorf("OKX API error (code=%d) from %s: %s", env.Code, endpoint, msg)
	}

	// If data field is present and non-null, return the unwrapped data
	if len(env.Data) > 0 && string(env.Data) != "null" {
		return env.Data, nil
	}

	// No envelope — body is the data directly (e.g. mock facilitator)
	return json.RawMessage(body), nil
}

// buildBody wraps payloadBytes and requirementsBytes as raw JSON into a request body.
// No field conversion — passes through as-is.
func buildBody(payloadBytes, requirementsBytes []byte, includeSyncSettle bool, syncSettle bool) ([]byte, error) {
	type v2Body struct {
		X402Version         int             `json:"x402Version"`
		PaymentPayload      json.RawMessage `json:"paymentPayload"`
		PaymentRequirements json.RawMessage `json:"paymentRequirements"`
		SyncSettle          *bool           `json:"syncSettle,omitempty"`
	}

	b := v2Body{
		X402Version:         2,
		PaymentPayload:      json.RawMessage(payloadBytes),
		PaymentRequirements: json.RawMessage(requirementsBytes),
	}

	if includeSyncSettle {
		ss := syncSettle
		b.SyncSettle = &ss
	}

	return json.Marshal(b)
}
