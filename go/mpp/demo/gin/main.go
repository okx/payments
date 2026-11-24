// demo-server demonstrates the MPP Go SDK with both charge and session middleware.
//
// Usage:
//
//	go run ./demo/okx-server
//
// Endpoints:
//
//	GET /free       — no payment required
//	GET /charge     — one-shot charge: 0.01 USDT per request
//	GET /session    — session payment: open channel, then replay voucher for multiple requests
//
// Session flow:
//
//  1. GET /session (no auth)          → 402 + WWW-Authenticate challenge
//  2. GET /session (open credential)  → 200 + channel ID in receipt
//  3. GET /session (voucher, amt=50)  → 200, deducts 10 per request (perRequestCost)
//  4. GET /session (same voucher)     → 200, replay: deducts 10, no sig check
//  5. ...repeat until balance exhausted...
//  6. GET /session (same voucher)     → 402 "insufficient balance, send new voucher"
//  7. GET /session (voucher, amt=100) → 200, new voucher accepted, deducts 10
package main

import (
	"context"
	"fmt"
	"log"
	"math/big"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/okx/payments/go/mpp/evm"
	mppgin "github.com/okx/payments/go/mpp/http/gin"
	"github.com/okx/payments/go/mpp/saclient"
	"github.com/okx/payments/go/mpp/server"
	"github.com/okx/payments/go/mpp/store"
)

func main() {
	// ── Payee signer (from PRIVATE_KEY env var) ──
	payeeKeyHex := os.Getenv("PRIVATE_KEY")
	if payeeKeyHex == "" {
		log.Fatal("PRIVATE_KEY env var is required")
	}
	demoSigner, err := evm.NewPrivateKeySignerFromHex(payeeKeyHex)
	if err != nil {
		log.Fatal("invalid PRIVATE_KEY")
	}

	payToAddr := envOrDefault("PAY_TO_ADDRESS", demoSigner.Address().Hex())
	payToAddrSplit := envOrDefault("PAY_TO_ADDRESS_SPLIT", payToAddr)

	cfg := server.EVMConfig{
		ChainID:   196,
		Recipient: payToAddr,
		SecretKey: envOrDefault("MPP_SECRET_KEY", "demo-secret-key"),
	}

	saClient := saclient.NewOKXSAClient(
		envOrDefault("OKX_BASE_URL", "https://web3.okx.com"),
		envOrDefault("OKX_API_KEY", ""),
		envOrDefault("OKX_SECRET_KEY", ""),
		envOrDefault("OKX_PASSPHRASE", ""),
	)

	// ── Charge methods ──
	// Each request = one EIP-3009 authorization, settled via SA API.
	chargeMethodTx := evm.NewEVMChargeMethod().
		WithChainID(cfg.ChainID).
		WithRecipient(cfg.Recipient).
		WithSAClient(saClient).
		WithFeePayer(true)

	chargeMethodHash := evm.NewEVMChargeMethod().
		WithChainID(cfg.ChainID).
		WithRecipient(cfg.Recipient).
		WithSAClient(saClient).
		WithFeePayer(false)

	// ── Session method ──
	// Client opens a payment channel, then sends vouchers authorizing cumulative spend.
	// Server deducts perRequestCost (10 base units) from the voucher balance on each request.
	// Client only needs to sign a new voucher when balance is exhausted.
	channelDir := envOrDefault("CHANNEL_STORE_DIR", "./mpp-data/channels")
	channelStore, err := store.NewFileStore[store.ChannelState](channelDir)
	if err != nil {
		log.Fatalf("filestore: %v", err)
	}
	log.Printf("Channel store: %s", channelDir)

	escrowContract := envOrDefault("ESCROW_CONTRACT", evm.DefaultEscrowContract)
	log.Printf("Escrow contract: %s", escrowContract)

	// Session method (feePayer=true, server pays gas)
	sessionMethodTx, err := evm.NewEVMSessionMethod(evm.EVMSessionMethodConfig{
		Recipient:       cfg.Recipient,
		SAClient:        saClient,
		Signer:          demoSigner,
		Store:           channelStore,
		PerRequestCost:  big.NewInt(10),
		MinVoucherDelta: big.NewInt(30),
		FeePayer:        true,
		EscrowContract:  escrowContract,
	})
	if err != nil {
		log.Fatalf("session method (tx): %v", err)
	}

	// Session method (feePayer=false, client pays gas)
	sessionMethodHash, err := evm.NewEVMSessionMethod(evm.EVMSessionMethodConfig{
		Recipient:       cfg.Recipient,
		SAClient:        saClient,
		Signer:          demoSigner,
		Store:           channelStore,
		PerRequestCost:  big.NewInt(10),
		MinVoucherDelta: big.NewInt(30),
		FeePayer:        false,
		EscrowContract:  escrowContract,
	})
	if err != nil {
		log.Fatalf("session method (hash): %v", err)
	}

	mppTx := server.NewMpp(cfg, chargeMethodTx, sessionMethodTx)
	mppHash := server.NewMpp(cfg, chargeMethodHash, sessionMethodHash)

	gin.SetMode(gin.ReleaseMode)
	r := gin.Default()

	// ── Free endpoint ──
	r.GET("/free", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{"message": "This endpoint is free!"})
	})

	// ── Charge endpoints ──
	// /charge/:mode/:variant where mode=tx|hash, variant=primary|split
	chargeHandler := func(c *gin.Context) {
		receipt := mppgin.GetReceipt(c)
		c.JSON(http.StatusOK, gin.H{
			"message": "Charge payment received!",
			"receipt": receipt,
		})
	}

	// /charge/tx/primary — transaction mode, single recipient
	r.GET("/charge/tx/primary",
		mppgin.ChargeMiddleware(mppTx, server.ChargeRouteConfig{
			Amount: "0.00001", Currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", Decimals: 6,
			Description: "Charge tx/primary: 0.00001 USDT",
		}),
		chargeHandler,
	)

	// /charge/tx/split — transaction mode, with splits
	r.GET("/charge/tx/split",
		mppgin.ChargeMiddleware(mppTx, server.ChargeRouteConfig{
			Amount: "0.00003", Currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", Decimals: 6,
			Description: "Charge tx/split: 0.00003 USDT (10 primary + 20 split)",
			Splits: []evm.Split{
				{Recipient: payToAddrSplit, Amount: "20"},
			},
		}),
		chargeHandler,
	)

	// /charge/hash/primary — hash mode, single recipient
	r.GET("/charge/hash/primary",
		mppgin.ChargeMiddleware(mppHash, server.ChargeRouteConfig{
			Amount: "0.00001", Currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", Decimals: 6,
			Description: "Charge hash/primary: 0.00001 USDT",
		}),
		chargeHandler,
	)

	// /charge/hash/split — hash mode, with splits
	r.GET("/charge/hash/split",
		mppgin.ChargeMiddleware(mppHash, server.ChargeRouteConfig{
			Amount: "0.00003", Currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", Decimals: 6,
			Description: "Charge hash/split: 0.00003 USDT (10 primary + 20 split)",
			Splits: []evm.Split{
				{Recipient: payToAddrSplit, Amount: "20"},
			},
		}),
		chargeHandler,
	)

	// ── Session endpoints ──
	sessionHandler := func(c *gin.Context) {
		receipt := mppgin.GetReceipt(c)
		c.JSON(http.StatusOK, gin.H{
			"message": "Session request served!",
			"receipt": receipt,
		})
	}

	// /session/tx — feePayer=true, server pays gas for open/topup
	r.GET("/session/tx",
		mppgin.SessionMiddleware(mppTx, server.SessionRouteConfig{
			Amount: "0.00001", Currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", Decimals: 6,
			Description:      "Session tx: 0.00001 USDT per request",
			UnitType:         "request",
			SuggestedDeposit: "60",
		}),
		sessionHandler,
	)

	// /session/hash — feePayer=false, client broadcasts open/topup
	r.GET("/session/hash",
		mppgin.SessionMiddleware(mppHash, server.SessionRouteConfig{
			Amount: "0.00001", Currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", Decimals: 6,
			Description:      "Session hash: 0.00001 USDT per request",
			UnitType:         "request",
			SuggestedDeposit: "60",
		}),
		sessionHandler,
	)

	addr := envOrDefault("ADDR", ":8080")
	srv := &http.Server{Addr: addr, Handler: r}

	go func() {
		log.Printf("Demo server listening on %s", addr)
		log.Printf("  GET %s/free                — no payment", addr)
		log.Printf("  GET %s/charge/tx/primary   — charge, tx mode", addr)
		log.Printf("  GET %s/charge/tx/split     — charge with splits, tx mode", addr)
		log.Printf("  GET %s/charge/hash/primary — charge, hash mode", addr)
		log.Printf("  GET %s/charge/hash/split   — charge with splits, hash mode", addr)
		log.Printf("  GET %s/session/tx          — session, feePayer=true", addr)
		log.Printf("  GET %s/session/hash        — session, feePayer=false", addr)
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatalf("listen: %v", err)
		}
	}()

	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit

	fmt.Println("\nShutting down...")

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := srv.Shutdown(ctx); err != nil {
		log.Fatalf("server shutdown: %v", err)
	}

	fmt.Println("Server stopped.")
}

func envOrDefault(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}
