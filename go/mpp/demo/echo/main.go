// demo-server demonstrates the MPP Go SDK with both charge and session middleware (Echo).
//
// Endpoints match mpp-gin exactly:
//
//	GET /free                — no payment required
//	GET /charge/tx/primary   — charge, tx mode
//	GET /charge/tx/split     — charge with splits, tx mode
//	GET /charge/hash/primary — charge, hash mode
//	GET /charge/hash/split   — charge with splits, hash mode
//	GET /session/tx          — session, feePayer=true
//	GET /session/hash        — session, feePayer=false
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

	"github.com/labstack/echo/v4"

	"github.com/okx/payments/go/mpp/evm"
	mppecho "github.com/okx/payments/go/mpp/http/echo"
	"github.com/okx/payments/go/mpp/saclient"
	"github.com/okx/payments/go/mpp/server"
	"github.com/okx/payments/go/mpp/store"
)

func main() {
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

	chargeMethodTx := evm.NewEVMChargeMethod().
		WithChainID(cfg.ChainID).WithRecipient(cfg.Recipient).WithSAClient(saClient).WithFeePayer(true)
	chargeMethodHash := evm.NewEVMChargeMethod().
		WithChainID(cfg.ChainID).WithRecipient(cfg.Recipient).WithSAClient(saClient).WithFeePayer(false)

	channelDir := envOrDefault("CHANNEL_STORE_DIR", "./mpp-data/channels")
	channelStore, err := store.NewFileStore[store.ChannelState](channelDir)
	if err != nil {
		log.Fatalf("filestore: %v", err)
	}
	log.Printf("Channel store: %s", channelDir)

	escrowContract := envOrDefault("ESCROW_CONTRACT", evm.DefaultEscrowContract)
	log.Printf("Escrow contract: %s", escrowContract)

	sessionMethodTx, err := evm.NewEVMSessionMethod(evm.EVMSessionMethodConfig{
		Recipient: cfg.Recipient, SAClient: saClient, Signer: demoSigner, Store: channelStore,
		PerRequestCost: big.NewInt(10), MinVoucherDelta: big.NewInt(30),
		FeePayer: true, EscrowContract: escrowContract,
	})
	if err != nil {
		log.Fatalf("session method (tx): %v", err)
	}

	sessionMethodHash, err := evm.NewEVMSessionMethod(evm.EVMSessionMethodConfig{
		Recipient: cfg.Recipient, SAClient: saClient, Signer: demoSigner, Store: channelStore,
		PerRequestCost: big.NewInt(10), MinVoucherDelta: big.NewInt(30),
		FeePayer: false, EscrowContract: escrowContract,
	})
	if err != nil {
		log.Fatalf("session method (hash): %v", err)
	}

	mppTx := server.NewMpp(cfg, chargeMethodTx, sessionMethodTx)
	mppHash := server.NewMpp(cfg, chargeMethodHash, sessionMethodHash)

	e := echo.New()
	e.HideBanner = true

	chargeHandler := func(c echo.Context) error {
		receipt := mppecho.GetReceipt(c)
		return c.JSON(http.StatusOK, map[string]any{"message": "Charge payment received!", "receipt": receipt})
	}
	sessionHandler := func(c echo.Context) error {
		receipt := mppecho.GetReceipt(c)
		return c.JSON(http.StatusOK, map[string]any{"message": "Session request served!", "receipt": receipt})
	}

	e.GET("/free", func(c echo.Context) error {
		return c.JSON(http.StatusOK, map[string]string{"message": "This endpoint is free!"})
	})

	e.GET("/charge/tx/primary", chargeHandler, mppecho.ChargeMiddleware(mppTx, server.ChargeRouteConfig{
		Amount: "0.00001", Currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", Decimals: 6,
		Description: "Charge tx/primary: 0.00001 USDT",
	}))
	e.GET("/charge/tx/split", chargeHandler, mppecho.ChargeMiddleware(mppTx, server.ChargeRouteConfig{
		Amount: "0.00003", Currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", Decimals: 6,
		Description: "Charge tx/split: 0.00003 USDT (10 primary + 20 split)",
		Splits:      []evm.Split{{Recipient: payToAddrSplit, Amount: "20"}},
	}))
	e.GET("/charge/hash/primary", chargeHandler, mppecho.ChargeMiddleware(mppHash, server.ChargeRouteConfig{
		Amount: "0.00001", Currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", Decimals: 6,
		Description: "Charge hash/primary: 0.00001 USDT",
	}))
	e.GET("/charge/hash/split", chargeHandler, mppecho.ChargeMiddleware(mppHash, server.ChargeRouteConfig{
		Amount: "0.00003", Currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", Decimals: 6,
		Description: "Charge hash/split: 0.00003 USDT (10 primary + 20 split)",
		Splits:      []evm.Split{{Recipient: payToAddrSplit, Amount: "20"}},
	}))

	e.GET("/session/tx", sessionHandler, mppecho.SessionMiddleware(mppTx, server.SessionRouteConfig{
		Amount: "0.00001", Currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", Decimals: 6,
		Description: "Session tx: 0.00001 USDT per request", UnitType: "request", SuggestedDeposit: "60",
	}))
	e.GET("/session/hash", sessionHandler, mppecho.SessionMiddleware(mppHash, server.SessionRouteConfig{
		Amount: "0.00001", Currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", Decimals: 6,
		Description: "Session hash: 0.00001 USDT per request", UnitType: "request", SuggestedDeposit: "60",
	}))

	addr := envOrDefault("ADDR", ":8080")
	srv := &http.Server{Addr: addr, Handler: e}

	go func() {
		log.Printf("MPP Echo demo listening on %s", addr)
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
