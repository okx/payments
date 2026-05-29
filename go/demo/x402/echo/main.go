// x402 Echo demo — matches x402-gin endpoints exactly.
//
//	GET /health         — health check
//	GET /resource/sync  — x402 paid (sync settle)
//	GET /resource/async — x402 paid (async settle)
package main

import (
	"fmt"
	"log"
	"net/http"
	"os"
	"time"

	"github.com/labstack/echo/v4"

	echomw "github.com/okx/payments/go/x402/http/echo"
	x402http "github.com/okx/payments/go/x402/http"
	deferred "github.com/okx/payments/go/x402/mechanisms/evm/deferred/server"
	exact "github.com/okx/payments/go/x402/mechanisms/evm/exact/server"
)

func boolPtr(b bool) *bool { return &b }

func makeRoute(payTo, description string) x402http.RouteConfig {
	return x402http.RouteConfig{
		Accepts: x402http.PaymentOptions{
			{Scheme: "exact", Price: "$0.00001", Network: "eip155:196", PayTo: payTo, MaxTimeoutSeconds: 300},
			{Scheme: "aggr_deferred", Price: "$0.00001", Network: "eip155:196", PayTo: payTo, MaxTimeoutSeconds: 300},
		},
		Description: description,
		MimeType:    "application/json",
	}
}

func schemes() []echomw.SchemeConfig {
	return []echomw.SchemeConfig{
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

	e := echo.New()
	e.HideBanner = true

	e.GET("/health", func(c echo.Context) error {
		return c.JSON(http.StatusOK, map[string]string{"status": "ok"})
	})

	if baseURL := os.Getenv("OKX_BASE_URL"); baseURL != "" {
		syncClient, err := x402http.NewOKXFacilitatorClient(&x402http.OKXFacilitatorConfig{
			Auth: x402http.OKXAuthConfig{
				APIKey:     os.Getenv("OKX_API_KEY"),
				SecretKey:  os.Getenv("OKX_SECRET_KEY"),
				Passphrase: os.Getenv("OKX_PASSPHRASE"),
			},
			BaseURL:    baseURL,
			SyncSettle: boolPtr(true),
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
		})
		if err != nil {
			log.Fatalf("Failed to create async client: %v", err)
		}

		syncRoutes := x402http.RoutesConfig{
			"GET /resource/sync": makeRoute(payTo, "Premium X Layer data (sync)"),
		}
		syncGroup := e.Group("/", echomw.X402Payment(echomw.Config{
			Routes:      syncRoutes,
			Facilitator: syncClient,
			Schemes:     schemes(),
			Timeout:     300 * time.Second,
		}))
		syncGroup.GET("/resource/sync", func(c echo.Context) error {
			return c.JSON(http.StatusOK, map[string]any{
				"message":     "Payment successful! Here is your premium X Layer data (sync).",
				"network":     "eip155:196",
				"settle_mode": "sync",
			})
		})

		asyncRoutes := x402http.RoutesConfig{
			"GET /resource/async": makeRoute(payToAsync, "Premium X Layer data (async)"),
		}
		asyncGroup := e.Group("/", echomw.X402Payment(echomw.Config{
			Routes:      asyncRoutes,
			Facilitator: asyncClient,
			Schemes:     schemes(),
			Timeout:     300 * time.Second,
		}))
		asyncGroup.GET("/resource/async", func(c echo.Context) error {
			return c.JSON(http.StatusOK, map[string]any{
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
	fmt.Printf("x402 Echo server listening on :%s\n", port)
	if err := e.Start(":" + port); err != nil && err != http.ErrServerClosed {
		fmt.Printf("Error starting server: %v\n", err)
		os.Exit(1)
	}
}
