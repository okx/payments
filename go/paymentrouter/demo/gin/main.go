// demo-server demonstrates the PaymentGate middleware with both MPP and x402
// adapters on the same endpoints.
//
// Usage:
//
//	go run -a ./paymentrouter/gin
//
// Endpoints:
//
//	GET /free              — no payment required
//	GET /api/onetime       — dual protocol (MPP charge + x402), one-shot
//	GET /api/batch         — dual protocol (MPP session + x402), streaming
package main

import (
	"log"
	"math/big"
	"net/http"
	"os"

	"github.com/gin-gonic/gin"

	mppadapters "github.com/okx/payments/go/mpp/adapters"
	"github.com/okx/payments/go/mpp/evm"
	"github.com/okx/payments/go/mpp/saclient"
	"github.com/okx/payments/go/mpp/server"
	"github.com/okx/payments/go/mpp/store"
	pr "github.com/okx/payments/go/paymentrouter"
	prgin "github.com/okx/payments/go/paymentrouter/gin"
	"github.com/okx/payments/go/x402"
	x402adapters "github.com/okx/payments/go/x402/adapters"
	x402http "github.com/okx/payments/go/x402/http"
	deferred "github.com/okx/payments/go/x402/mechanisms/evm/deferred/server"
	exact "github.com/okx/payments/go/x402/mechanisms/evm/exact/server"
)

func main() {
	payTo := envOrDefault("PAY_TO_ADDRESS", "0xYourRecipientAddress")

	var payeeSigner evm.Signer
	if pk := os.Getenv("PRIVATE_KEY"); pk != "" {
		var signerErr error
		payeeSigner, signerErr = evm.NewPrivateKeySignerFromHex(pk)
		if signerErr != nil {
			log.Fatalf("payee signer: %v", signerErr)
		}
	}

	// ── MPP setup ──
	mppCfg := server.EVMConfig{
		ChainID:   196,
		Recipient: payTo,
		SecretKey: envOrDefault("MPP_SECRET_KEY", "demo-secret-key"),
	}

	saClient := saclient.NewOKXSAClient(
		envOrDefault("OKX_BASE_URL", ""),
		envOrDefault("OKX_API_KEY", ""),
		envOrDefault("OKX_SECRET_KEY", ""),
		envOrDefault("OKX_PASSPHRASE", ""),
	)

	chargeMethod := evm.NewEVMChargeMethod().
		WithChainID(mppCfg.ChainID).
		WithRecipient(payTo).
		WithSAClient(saClient).
		WithFeePayer(true)

	channelDir := envOrDefault("CHANNEL_DIR", "mpp-data")
	channelStore, err := store.NewFileStore[store.ChannelState](channelDir)
	if err != nil {
		log.Fatalf("filestore: %v", err)
	}
	log.Printf("Channel store: %s", channelDir)

	sessionMethod, err := evm.NewEVMSessionMethod(evm.EVMSessionMethodConfig{
		ChainID:         mppCfg.ChainID,
		Recipient:       payTo,
		SAClient:        saClient,
		Signer:          payeeSigner,
		Store:           channelStore,
		PerRequestCost:  big.NewInt(10),
		MinVoucherDelta: big.NewInt(10),
		FeePayer:        true,
	})
	if err != nil {
		log.Fatalf("session method: %v", err)
	}

	mpp := server.NewMpp(mppCfg, chargeMethod, sessionMethod)

	// ── x402 setup ──
	facilitator, err := x402http.NewOKXFacilitatorClient(&x402http.OKXFacilitatorConfig{
		Auth: x402http.OKXAuthConfig{
			APIKey:     envOrDefault("OKX_API_KEY", "mock-api-key"),
			SecretKey:  envOrDefault("OKX_SECRET_KEY", "mock-secret-key"),
			Passphrase: envOrDefault("OKX_PASSPHRASE", "mock-passphrase"),
		},
		BaseURL:    envOrDefault("OKX_BASE_URL", "https://www.web3.okx.com"),
		SyncSettle: boolPtr(true),
	})
	if err != nil {
		log.Fatalf("x402 facilitator: %v", err)
	}

	x402Server := x402http.Newx402HTTPResourceServer(nil, x402.WithFacilitatorClient(facilitator))
	x402Server.Register("eip155:196", exact.NewExactEvmScheme())
	x402Server.Register("eip155:196", deferred.NewAggrDeferredEvmScheme())

	// ── Adapters ──
	mppAdapter := mppadapters.NewMppAdapter(mpp)
	x402Adapter := x402adapters.NewX402Adapter(x402Server)

	// ── Gin + PaymentGate ──
	gin.SetMode(gin.ReleaseMode)
	r := gin.Default()

	r.GET("/free", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{"message": "This endpoint is free!"})
	})

	paid := prgin.New([]pr.ProtocolAdapter{mppAdapter, x402Adapter},
		prgin.WithOnError(func(err error, phase, protocol string) {
			log.Printf("[%s] %s: %v", protocol, phase, err)
		}),
	)

	onetimeCfg := pr.RouteConfig{
		"mpp": mppadapters.MppRouteConfig{
			Intent:      "charge",
			Amount:      "0.00001",
			Currency:    "0x779ded0c9e1022225f8e0630b35a9b54be713736",
			Decimals:    6,
			Description: "One-time payment",
		},
		"x402": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{{
				Scheme:            "exact",
				Price:             "$0.00001",
				Network:           "eip155:196",
				PayTo:             payTo,
				MaxTimeoutSeconds: 300,
			}},
			Description: "One-time payment",
			MimeType:    "application/json",
		},
	}

	batchCfg := pr.RouteConfig{
		"mpp": mppadapters.MppRouteConfig{
			Intent:           "session",
			Amount:           "0.00001",
			Currency:         "0x779ded0c9e1022225f8e0630b35a9b54be713736",
			Decimals:         6,
			Description:      "Batch session",
			UnitType:         "request",
			SuggestedDeposit: "60",
		},
		"x402": x402http.RouteConfig{
			Accepts: x402http.PaymentOptions{{
				Scheme:            "aggr_deferred",
				Price:             "$0.00001",
				Network:           "eip155:196",
				PayTo:             payTo,
				MaxTimeoutSeconds: 300,
			}},
			Description: "Batch session",
			MimeType:    "application/json",
		},
	}

	r.GET("/api/onetime", paid.For(onetimeCfg), func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{"message": "One-time payment received!"})
	})

	r.GET("/api/batch", paid.For(batchCfg), func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{"message": "Batch request served!"})
	})

	port := envOrDefault("PORT", "8080")
	log.Printf("PaymentRouter demo listening on :%s", port)
	log.Printf("  GET /free          — no payment")
	log.Printf("  GET /api/onetime   — dual (MPP charge + x402 exact)")
	log.Printf("  GET /api/batch     — dual (MPP session + x402 aggr_deferred)")

	if err := r.Run(":" + port); err != nil {
		log.Fatalf("server: %v", err)
	}
}

func envOrDefault(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func boolPtr(b bool) *bool { return &b }
