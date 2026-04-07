package main

import (
	"bytes"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"sync"
	"time"

	ginfw "github.com/gin-gonic/gin"
	x402http "github.com/okx/payments/go/http"
	ginmw "github.com/okx/payments/go/http/gin"
	deferred "github.com/okx/payments/go/mechanisms/evm/deferred/server"
	exact "github.com/okx/payments/go/mechanisms/evm/exact/server"
)

// debugTransport wraps http.RoundTripper to capture raw OKX API request/response bodies.
type debugTransport struct {
	inner http.RoundTripper
	mu    sync.Mutex
	calls []debugHTTPCall
}

type debugHTTPCall struct {
	Method       string `json:"method"`
	URL          string `json:"url"`
	RequestBody  string `json:"requestBody,omitempty"`
	StatusCode   int    `json:"statusCode"`
	ResponseBody string `json:"responseBody"`
}

func (t *debugTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	call := debugHTTPCall{
		Method: req.Method,
		URL:    req.URL.String(),
	}

	// Capture request body
	if req.Body != nil {
		bodyBytes, _ := io.ReadAll(req.Body)
		req.Body.Close()
		call.RequestBody = string(bodyBytes)
		req.Body = io.NopCloser(bytes.NewReader(bodyBytes))
	}

	resp, err := t.inner.RoundTrip(req)
	if err != nil {
		return resp, err
	}

	// Capture response body
	respBytes, _ := io.ReadAll(resp.Body)
	resp.Body.Close()
	call.StatusCode = resp.StatusCode
	call.ResponseBody = string(respBytes)
	resp.Body = io.NopCloser(bytes.NewReader(respBytes))

	t.mu.Lock()
	t.calls = append(t.calls, call)
	t.mu.Unlock()

	return resp, nil
}

func (t *debugTransport) consumeCalls() []debugHTTPCall {
	t.mu.Lock()
	defer t.mu.Unlock()
	calls := t.calls
	t.calls = nil
	return calls
}

// Global list of all debug transports so /debug/last can drain them.
var allTransports []*debugTransport

func newDebugHTTPClient() *http.Client {
	dt := &debugTransport{inner: http.DefaultTransport}
	allTransports = append(allTransports, dt)
	return &http.Client{Transport: dt, Timeout: 30 * time.Second}
}

func boolPtr(b bool) *bool { return &b }

func makeRoute(payTo, description string) x402http.RouteConfig {
	return x402http.RouteConfig{
		Accepts: x402http.PaymentOptions{
			{Scheme: "exact", Price: "$0.001", Network: "eip155:196", PayTo: payTo, MaxTimeoutSeconds: 300},
			{Scheme: "aggr_deferred", Price: "$0.001", Network: "eip155:196", PayTo: payTo, MaxTimeoutSeconds: 300},
		},
		Description: description,
		MimeType:    "application/json",
	}
}

func schemes() []ginmw.SchemeConfig {
	return []ginmw.SchemeConfig{
		{Network: "eip155:196", Server: exact.NewExactEvmScheme()},
		{Network: "eip155:196", Server: deferred.NewAggrDeferredEvmScheme()},
	}
}

func main() {
	payTo := os.Getenv("PAY_TO_ADDRESS")
	if payTo == "" {
		fmt.Println("PAY_TO_ADDRESS environment variable is required")
		os.Exit(1)
	}
	payToAsync := os.Getenv("PAY_TO_ADDRESS_ASYNC")
	if payToAsync == "" {
		payToAsync = payTo
	}

	r := ginfw.Default()

	r.GET("/health", func(c *ginfw.Context) {
		c.JSON(http.StatusOK, ginfw.H{"status": "ok"})
	})

	// Debug endpoint: returns raw OKX API calls captured since last drain
	r.GET("/debug/last", func(c *ginfw.Context) {
		var all []debugHTTPCall
		for _, dt := range allTransports {
			all = append(all, dt.consumeCalls()...)
		}
		c.JSON(http.StatusOK, all)
	})

	// OKX routes — only if OKX_BASE_URL is set
	if baseURL := os.Getenv("OKX_BASE_URL"); baseURL != "" {
		syncClient, err := x402http.NewOKXFacilitatorClient(&x402http.OKXFacilitatorConfig{
			Auth: x402http.OKXAuthConfig{
				APIKey:     os.Getenv("OKX_API_KEY"),
				SecretKey:  os.Getenv("OKX_SECRET_KEY"),
				Passphrase: os.Getenv("OKX_PASSPHRASE"),
			},
			BaseURL:    baseURL,
			SyncSettle: boolPtr(true),
			HTTPClient: newDebugHTTPClient(),
		})
		if err != nil {
			log.Fatalf("Failed to create sync client: %v", err)
		}

		asyncClient, err := x402http.NewOKXFacilitatorClient(&x402http.OKXFacilitatorConfig{
			Auth: x402http.OKXAuthConfig{
				APIKey:     os.Getenv("OKX_API_KEY"),
				SecretKey:  os.Getenv("OKX_SECRET_KEY"),
				Passphrase: os.Getenv("OKX_PASSPHRASE"),
			},
			BaseURL:    baseURL,
			SyncSettle: boolPtr(false),
			HTTPClient: newDebugHTTPClient(),
		})
		if err != nil {
			log.Fatalf("Failed to create async client: %v", err)
		}

		syncRoutes := x402http.RoutesConfig{
			"GET /resource/sync": makeRoute(payTo, "Premium X Layer data (sync)"),
		}
		syncGroup := r.Group("/")
		syncGroup.Use(ginmw.X402Payment(ginmw.Config{
			Routes:      syncRoutes,
			Facilitator: syncClient,
			Schemes:     schemes(),
			Timeout:     300 * time.Second,
		}))
		syncGroup.GET("/resource/sync", func(c *ginfw.Context) {
			c.JSON(http.StatusOK, ginfw.H{
				"message":     "Payment successful! Here is your premium X Layer data (sync).",
				"network":     "eip155:196",
				"settle_mode": "sync",
			})
		})

		asyncRoutes := x402http.RoutesConfig{
			"GET /resource/async": makeRoute(payToAsync, "Premium X Layer data (async)"),
		}
		asyncGroup := r.Group("/")
		asyncGroup.Use(ginmw.X402Payment(ginmw.Config{
			Routes:      asyncRoutes,
			Facilitator: asyncClient,
			Schemes:     schemes(),
			Timeout:     300 * time.Second,
		}))
		asyncGroup.GET("/resource/async", func(c *ginfw.Context) {
			c.JSON(http.StatusOK, ginfw.H{
				"message":     "Payment successful! Here is your premium X Layer data (async).",
				"network":     "eip155:196",
				"settle_mode": "async",
			})
		})

		fmt.Println("OKX routes enabled: GET /resource/sync, GET /resource/async")
	} else {
		fmt.Println("OKX routes disabled (OKX_BASE_URL not set)")
	}

	port := os.Getenv("PORT")
	if port == "" {
		port = "4001"
	}
	fmt.Printf("Seller server listening on :%s\n", port)
	if err := r.Run(":" + port); err != nil {
		fmt.Printf("Error starting server: %v\n", err)
		os.Exit(1)
	}
}
