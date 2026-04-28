# OKX x402 Java SDK — Examples

## Server (Spring Boot)

See `DemoServer.java` for a Spring Boot server with x402 payment middleware.

Required environment variables:
- `OKX_API_KEY` — OKX API key
- `OKX_SECRET_KEY` — OKX secret key
- `OKX_PASSPHRASE` — OKX passphrase
- `PAY_TO_ADDRESS` — Receiver wallet address

## Client

See `DemoClient.java` for a standalone client with automatic 402 payment handling.

Required environment variables:
- `PRIVATE_KEY` — 0x-prefixed hex private key

## Quick Start

1. Start the server:
   ```bash
   export OKX_API_KEY=your-key
   export OKX_SECRET_KEY=your-secret
   export OKX_PASSPHRASE=your-passphrase
   export PAY_TO_ADDRESS=0xYourAddress
   # Run with Spring Boot
   ```

2. Run the client:
   ```bash
   export PRIVATE_KEY=0xYourPrivateKey
   java -cp x402-java-jakarta.jar com.okx.x402.examples.DemoClient
   ```
