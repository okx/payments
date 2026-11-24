# x402 Go Seller SDK — AI Integration Guide

> This document is designed to be read by AI coding agents (Cursor, Claude Code, Copilot, etc.)
> to generate complete x402 payment integration code for Go servers.

## What is x402?

x402 is the HTTP 402 Payment Required protocol. It lets you charge for API access per-request. When a client requests a protected endpoint without payment, the server returns HTTP 402 with payment requirements. The client signs a payment, retries the request, and gets the resource.

## Install

```bash
go get github.com/okx/payments/go/x402
```

## Complete Example (Gin)

```go
package main

import (
	"fmt"
	"net/http"
	"os"
	"time"

	x402http "github.com/okx/payments/go/x402/http"
	ginmw "github.com/okx/payments/go/x402/http/gin"
	exact "github.com/okx/payments/go/x402/mechanisms/evm/exact/server"
	deferred "github.com/okx/payments/go/x402/mechanisms/evm/deferred/server"
	ginfw "github.com/gin-gonic/gin"
)

func main() {
	payTo := os.Getenv("PAY_TO_ADDRESS")
	if payTo == "" {
		fmt.Println("PAY_TO_ADDRESS required")
		os.Exit(1)
	}

	// 1. Create OKX Facilitator client
	syncClient, err := x402http.NewOKXFacilitatorClient(&x402http.OKXFacilitatorConfig{
		Auth: x402http.OKXAuthConfig{
			APIKey:     os.Getenv("OKX_API_KEY"),
			SecretKey:  os.Getenv("OKX_SECRET_KEY"),
			Passphrase: os.Getenv("OKX_PASSPHRASE"),
		},
		BaseURL: os.Getenv("OKX_BASE_URL"),
	})
	if err != nil {
		fmt.Printf("Failed to create client: %v\n", err)
		os.Exit(1)
	}

	// 2. Define which routes require payment
	routes := x402http.RoutesConfig{
		"GET /api/data": {
			Accepts: x402http.PaymentOptions{
				{Scheme: "exact", Price: "$0.01", Network: "eip155:196", PayTo: payTo},
				{Scheme: "aggr_deferred", Price: "$0.01", Network: "eip155:196", PayTo: payTo},
			},
			Description: "Premium data endpoint",
			MimeType:    "application/json",
		},
	}

	// 3. Register payment schemes
	schemes := []ginmw.SchemeConfig{
		{Network: "eip155:196", Server: exact.NewExactEvmScheme()},
		{Network: "eip155:196", Server: deferred.NewAggrDeferredEvmScheme()},
	}

	// 4. Create Gin router with payment middleware
	r := ginfw.Default()

	r.GET("/health", func(c *ginfw.Context) {
		c.JSON(http.StatusOK, ginfw.H{"status": "ok"})
	})

	apiGroup := r.Group("/")
	apiGroup.Use(ginmw.X402Payment(ginmw.Config{
		Routes:      routes,
		Facilitator: syncClient,
		Schemes:     schemes,
		Timeout:     30 * time.Second,
	}))
	apiGroup.GET("/api/data", func(c *ginfw.Context) {
		c.JSON(http.StatusOK, ginfw.H{
			"data":  "premium content",
			"price": "$0.01",
		})
	})

	fmt.Println("Server at http://localhost:3000")
	fmt.Println("  GET /health    - free")
	fmt.Println("  GET /api/data  - $0.01 USDT on X Layer")
	r.Run(":3000")
}
```

## API Reference

### OKXFacilitatorClient

```go
import x402http "github.com/okx/payments/go/x402/http"

client, err := x402http.NewOKXFacilitatorClient(&x402http.OKXFacilitatorConfig{
	Auth: x402http.OKXAuthConfig{
		APIKey:     "your-api-key",
		SecretKey:  "your-secret-key",
		Passphrase: "your-passphrase",
	},
	BaseURL:    "https://web3.okx.com",       // default if empty
	SyncSettle: &syncSettle,                 // nil/true=sync wait for confirm, false=async (syncSettle := true)
	HTTPClient: &http.Client{},              // optional custom HTTP client
	Timeout:    30 * time.Second,            // optional, default 30s (ignored if HTTPClient set)
})
```

HMAC-SHA256 signing is automatic on every Facilitator request.

### OKXAuthConfig

```go
type OKXAuthConfig struct {
	APIKey     string  // OKX API key (required)
	SecretKey  string  // OKX secret key for HMAC-SHA256 signing (required)
	Passphrase string  // OKX API passphrase (required)
	BaseURL    string  // Auth base URL (optional, default: "https://web3.okx.com")
	BasePath   string  // Auth base path (optional, e.g. "/api/v6/x402")
}
```

### RoutesConfig

```go
routes := x402http.RoutesConfig{
	"GET /api/data": {
		Accepts: x402http.PaymentOptions{
			{Scheme: "exact", Price: "$0.01", Network: "eip155:196", PayTo: "0xYourAddress"},
			{Scheme: "aggr_deferred", Price: "$0.01", Network: "eip155:196", PayTo: "0xYourAddress"},
		},
		Description: "Resource description",
		MimeType:    "application/json",
	},
}
```

### Gin Middleware

```go
import ginmw "github.com/okx/payments/go/x402/http/gin"

apiGroup := r.Group("/")
apiGroup.Use(ginmw.X402Payment(ginmw.Config{
	Routes:      routes,
	Facilitator: client,
	Schemes:     schemes,
	Timeout:     30 * time.Second,
}))
```

### Echo Middleware

```go
import echomw "github.com/okx/payments/go/x402/http/echo"

e := echo.New()
apiGroup := e.Group("/")
apiGroup.Use(echomw.X402Payment(echomw.Config{
	Routes:      routes,
	Facilitator: client,
	Schemes:     []echomw.SchemeConfig{
		{Network: "eip155:196", Server: exact.NewExactEvmScheme()},
		{Network: "eip155:196", Server: deferred.NewAggrDeferredEvmScheme()},
	},
	Timeout: 30 * time.Second,
}))
```

### net/http Middleware

```go
import nethttpmw "github.com/okx/payments/go/x402/http/nethttp"

mux := http.NewServeMux()
handler := nethttpmw.X402Payment(nethttpmw.Config{
	Routes:      routes,
	Facilitator: client,
	Schemes:     []nethttpmw.SchemeConfig{
		{Network: "eip155:196", Server: exact.NewExactEvmScheme()},
		{Network: "eip155:196", Server: deferred.NewAggrDeferredEvmScheme()},
	},
	Timeout: 30 * time.Second,
})(mux)
http.ListenAndServe(":3000", handler)
```

### Payment Schemes

```go
import exact "github.com/okx/payments/go/x402/mechanisms/evm/exact/server"
import deferred "github.com/okx/payments/go/x402/mechanisms/evm/deferred/server"

schemes := []ginmw.SchemeConfig{
	{Network: "eip155:196", Server: exact.NewExactEvmScheme()},
	{Network: "eip155:196", Server: deferred.NewAggrDeferredEvmScheme()},
}
```

| Scheme            | Constructor                           | Description                               |
| ----------------- | ------------------------------------- | ----------------------------------------- |
| `"exact"`         | `exact.NewExactEvmScheme()`           | Standard EIP-3009 on-chain payment        |
| `"aggr_deferred"` | `deferred.NewAggrDeferredEvmScheme()` | Session key signing, OKX batches on-chain |

## Supported Networks

Pre-configured networks with default assets. Use `eip155:*` wildcard to support all EVM chains.

| Chain        | Network ID     | Token | Contract                                     | Decimals | Transfer Method |
| ------------ | -------------- | ----- | -------------------------------------------- | -------- | --------------- |
| X Layer      | `eip155:196`   | USD₮0 | `0x779Ded0c9e1022225f8E0630b35a9b54bE713736` | 6        | EIP-3009        |
| Base         | `eip155:8453`  | USDC  | `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913` | 6        | EIP-3009        |
| Base Sepolia | `eip155:84532` | USDC  | `0x036CbD53842c5426634e7929541eC2318f3dCF7e` | 6        | EIP-3009        |
| MegaETH      | `eip155:4326`  | USDM  | `0xFAfDdbb3FC7688494971a79cc65DCa3EF82079E7` | 18       | Permit2         |
| Monad        | `eip155:143`   | USDC  | `0x754704Bc059F8C67012fEd69BC8A327a5aafb603` | 6        | EIP-3009        |
| Mezo Testnet | `eip155:31611` | mUSD  | `0x118917a40FAF1CD7a13dB0Ef56C86De7973Ac503` | 18       | Permit2         |
| Stable       | `eip155:988`   | USDT0 | `0x779Ded0c9e1022225f8E0630b35a9b54bE713736` | 6        | EIP-3009        |

## Environment Variables

| Variable         | Required | Description                                           |
| ---------------- | -------- | ----------------------------------------------------- |
| `OKX_API_KEY`    | Yes      | OKX API key                                           |
| `OKX_SECRET_KEY` | Yes      | OKX secret key                                        |
| `OKX_PASSPHRASE` | Yes      | OKX API passphrase                                    |
| `PAY_TO_ADDRESS` | Yes      | Your wallet address to receive payments               |
| `OKX_BASE_URL`   | No       | Facilitator URL (default: `https://www.web3.okx.com`) |

## Running

```bash
OKX_API_KEY=your-key OKX_SECRET_KEY=your-secret OKX_PASSPHRASE='your-pass' \
OKX_BASE_URL=web3.okx.com \
PAY_TO_ADDRESS=0xYourAddress go run .
```

## Payment Flow

```
Client: GET /api/data (no payment)
  → Server: HTTP 402 + PAYMENT-REQUIRED header (base64-encoded PaymentRequired JSON)

Client: signs payment with wallet

Client: GET /api/data + PAYMENT-SIGNATURE header (base64-encoded PaymentPayload)
  → Server: verify → handler → settle → HTTP 200 + data + PAYMENT-RESPONSE header
```

## Multiple Routes with Different Prices

```go
routes := x402http.RoutesConfig{
	"GET /api/basic": {
		Accepts: x402http.PaymentOptions{
			{Scheme: "exact", Price: "$0.001", Network: "eip155:196", PayTo: payTo},
			{Scheme: "aggr_deferred", Price: "$0.001", Network: "eip155:196", PayTo: payTo},
		},
		Description: "Basic data",
		MimeType:    "application/json",
	},
	"GET /api/premium": {
		Accepts: x402http.PaymentOptions{
			{Scheme: "exact", Price: "$0.10", Network: "eip155:196", PayTo: payTo},
			{Scheme: "aggr_deferred", Price: "$0.10", Network: "eip155:196", PayTo: payTo},
		},
		Description: "Premium analytics",
		MimeType:    "application/json",
	},
}
```

## Multiple Payment Methods Per Route

```go
"GET /api/data": {
	Accepts: x402http.PaymentOptions{
		{Scheme: "exact", Price: "$0.01", Network: "eip155:196", PayTo: payTo},
		{Scheme: "aggr_deferred", Price: "$0.01", Network: "eip155:196", PayTo: payTo},
	},
	Description: "Accepts both exact and deferred payments",
	MimeType:    "application/json",
},
```

## Free + Paid Routes Together

Routes NOT in the middleware config are free:

```go
r.GET("/health", healthHandler)  // FREE — not in middleware group

apiGroup := r.Group("/")
apiGroup.Use(ginmw.X402Payment(config))
apiGroup.GET("/api/data", dataHandler)  // PAID
```

## Sync vs Async Settlement

Default is **sync** (`SyncSettle: nil` or `&true`).

```go
// Sync (default): settle waits for on-chain confirmation, returns status="pending"
client, _ := x402http.NewOKXFacilitatorClient(&x402http.OKXFacilitatorConfig{
	Auth:    authConfig,
	BaseURL: baseURL,
})

// Async: settle returns immediately with status="success", settles in background
asyncSettle := false
client, _ := x402http.NewOKXFacilitatorClient(&x402http.OKXFacilitatorConfig{
	Auth:       authConfig,
	BaseURL:    baseURL,
	SyncSettle: &asyncSettle,
})
```
