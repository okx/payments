package saclient

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"net/url"
	"time"

	mpperrors "github.com/okx/payments/go/mpp/errors"
)

const (
	headerAPIKey      = "OK-ACCESS-KEY"
	headerSign        = "OK-ACCESS-SIGN"
	headerTimestamp   = "OK-ACCESS-TIMESTAMP"
	headerPassphrase  = "OK-ACCESS-PASSPHRASE"
	headerContentType = "Content-Type"
	contentTypeJSON   = "application/json"
)

// OKXSAClient is the default SAClient implementation targeting the OKX SA API.
// Authenticates via OKX API key + HMAC-SHA256 signature + passphrase.
type OKXSAClient struct {
	baseURL    string
	apiKey     string
	secretKey  string
	passphrase string
	httpClient *http.Client
}

// Option is a functional option for OKXSAClient.
type Option func(*OKXSAClient)

// WithHTTPClient replaces the default http.Client.
func WithHTTPClient(c *http.Client) Option {
	return func(s *OKXSAClient) { s.httpClient = c }
}

// NewOKXSAClient constructs an OKXSAClient with the given base URL and API credentials.
func NewOKXSAClient(baseURL, apiKey, secretKey, passphrase string, opts ...Option) *OKXSAClient {
	c := &OKXSAClient{
		baseURL:    baseURL,
		apiKey:     apiKey,
		secretKey:  secretKey,
		passphrase: passphrase,
		httpClient: http.DefaultClient,
	}
	for _, o := range opts {
		o(c)
	}
	return c
}

// sign computes the OKX API HMAC-SHA256 signature.
// Format: HMAC-SHA256(secretKey, timestamp + method + requestPath + body)
func (c *OKXSAClient) sign(timestamp, method, requestPath, body string) string {
	prehash := timestamp + method + requestPath + body
	mac := hmac.New(sha256.New, []byte(c.secretKey))
	mac.Write([]byte(prehash))
	return base64.StdEncoding.EncodeToString(mac.Sum(nil))
}

// setAuthHeaders sets the OKX API authentication headers on the request.
func (c *OKXSAClient) setAuthHeaders(req *http.Request, body string) {
	req.Header.Set(headerAPIKey, c.apiKey)
	if c.secretKey != "" {
		ts := time.Now().UTC().Format(time.RFC3339)
		path := req.URL.Path
		if req.URL.RawQuery != "" {
			path += "?" + req.URL.RawQuery
		}
		sig := c.sign(ts, req.Method, path, body)
		req.Header.Set(headerTimestamp, ts)
		req.Header.Set(headerSign, sig)
		req.Header.Set(headerPassphrase, c.passphrase)
	}
}

// doPost marshals body to JSON, POSTs to path, and returns the parsed SAResponse.
func (c *OKXSAClient) doPost(ctx context.Context, path string, body any) (*SAResponse, error) {
	data, err := json.Marshal(body)
	if err != nil {
		return nil, &mpperrors.MppError{
			Code:    mpperrors.CodeJson,
			Message: "failed to marshal request body",
			Reason:  err.Error(),
		}
	}

	log.Printf("[SA DEBUG] POST %s%s\n%s", c.baseURL, path, string(data))

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+path, bytes.NewReader(data))
	if err != nil {
		return nil, &mpperrors.MppError{
			Code:    mpperrors.CodeHttp,
			Message: "failed to build HTTP request",
			Reason:  err.Error(),
		}
	}
	req.Header.Set(headerContentType, contentTypeJSON)
	c.setAuthHeaders(req, string(data))

	return c.execute(req)
}

// doGet issues a GET request to path with the provided query parameters.
func (c *OKXSAClient) doGet(ctx context.Context, path string, params map[string]string) (*SAResponse, error) {
	u, err := url.Parse(c.baseURL + path)
	if err != nil {
		return nil, &mpperrors.MppError{
			Code:    mpperrors.CodeHttp,
			Message: "failed to parse request URL",
			Reason:  err.Error(),
		}
	}
	if len(params) > 0 {
		q := u.Query()
		for k, v := range params {
			q.Set(k, v)
		}
		u.RawQuery = q.Encode()
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, u.String(), nil)
	if err != nil {
		return nil, &mpperrors.MppError{
			Code:    mpperrors.CodeHttp,
			Message: "failed to build HTTP request",
			Reason:  err.Error(),
		}
	}
	c.setAuthHeaders(req, "")

	return c.execute(req)
}

// execute sends req, reads the body, and decodes the SA envelope.
func (c *OKXSAClient) execute(req *http.Request) (*SAResponse, error) {
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, &mpperrors.MppError{
			Code:    mpperrors.CodeHttp,
			Message: "HTTP request failed",
			Reason:  err.Error(),
		}
	}
	defer resp.Body.Close()

	raw, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, &mpperrors.MppError{
			Code:    mpperrors.CodeHttp,
			Message: "failed to read response body",
			Reason:  err.Error(),
		}
	}

	if resp.StatusCode != http.StatusOK {
		return nil, &mpperrors.MppError{
			Code:    mpperrors.CodeHttp,
			Message: fmt.Sprintf("SA API returned HTTP %d", resp.StatusCode),
			Reason:  string(raw),
		}
	}

	var saResp SAResponse
	if err := json.Unmarshal(raw, &saResp); err != nil {
		return nil, &mpperrors.MppError{
			Code:    mpperrors.CodeJson,
			Message: "failed to decode SA API response",
			Reason:  err.Error(),
		}
	}

	if SAErrorCode(saResp.Code) != SACodeSuccess {
		return nil, c.mapSAError(SAErrorCode(saResp.Code), saResp.Msg+"; raw: "+string(raw))
	}

	return &saResp, nil
}

// mapSAError converts a non-zero SA API error code into an MppError.
func (c *OKXSAClient) mapSAError(code SAErrorCode, msg string) error {
	var mppCode mpperrors.MppErrorCode
	switch code {
	case SACodeInvalidParams:
		mppCode = mpperrors.CodeBadRequest
	case SACodeUnsupportedChain:
		mppCode = mpperrors.CodeInternal
	case SACodePayerBlocked:
		mppCode = mpperrors.CodeMalformedCredential
	case SACodeInvalidCredential:
		mppCode = mpperrors.CodeMalformedCredential
	case SACodeInvalidSignature:
		mppCode = mpperrors.CodeInvalidSignature
	case SACodeSplitSumExceedsTotal:
		mppCode = mpperrors.CodeInvalidSplit
	case SACodeSplitCountExceeded:
		mppCode = mpperrors.CodeInvalidSplit
	case SACodeTxNotConfirmed:
		mppCode = mpperrors.CodeInternal
	case SACodeChannelClosed:
		mppCode = mpperrors.CodeChannelClosed
	case SACodeChallengeInvalid:
		mppCode = mpperrors.CodeInvalidChallenge
	case SACodeChannelNotFound:
		mppCode = mpperrors.CodeChannelNotFound
	case SACodeGracePeriodTooShort:
		mppCode = mpperrors.CodeInternal
	case SACodeAmountExceedsDeposit:
		mppCode = mpperrors.CodeAmountExceedsDeposit
	case SACodeVoucherDeltaTooSmall:
		mppCode = mpperrors.CodeDeltaTooSmall
	case SACodeChannelClosing:
		mppCode = mpperrors.CodeChannelClosed
	case SACodeInternalError:
		mppCode = mpperrors.CodeInternal
	default:
		mppCode = mpperrors.CodeInternal
	}
	return &mpperrors.MppError{
		Code:    mppCode,
		Message: msg,
		Reason:  fmt.Sprintf("SA error code %d", code),
	}
}
