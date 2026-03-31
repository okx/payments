# OKX x402 Java SDK

[![Java Version](https://img.shields.io/badge/java-17%2B-orange)](https://github.com/nicklhw/payments-sdk/java)

Java implementation of the [x402](https://www.x402.org/) payment protocol with OKX X Layer support.

## Quick Start

```bash
# Build and test
mvn clean install

# Run tests
mvn test

# Check code quality
mvn checkstyle:check
mvn spotbugs:check
```

## Overview

x402 is a system for decentralized payments for API calls, web content, and other HTTP resources. The `402` stands for the HTTP status code `Payment Required`.

This SDK provides:

- **V2 Protocol** — Full x402 V2 support with CAIP-2 network IDs and `PAYMENT-SIGNATURE` headers
- **X Layer Native** — Built-in support for OKX X Layer (eip155:196) with USDT EIP-3009 signing
- **OKX Facilitator** — Integrated OKX facilitator client with HMAC-SHA256 auth
- **Auto-402 Client** — `OKXHttpClient` that automatically handles 402 → sign → retry flows
- **Server Middleware** — `PaymentFilter` servlet filter with route-level pricing

## Installation

Add the dependency to your Maven project:

```xml
<dependency>
    <groupId>com.okx</groupId>
    <artifactId>x402-java</artifactId>
    <version>1.0.0-SNAPSHOT</version>
</dependency>
```

## Usage

### Server Side — Requiring Payments

```java
import com.okx.x402.facilitator.FacilitatorRouter;
import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.server.PaymentFilter;

import java.util.Map;

// 1. Build facilitator with OKX credentials
FacilitatorClient facilitator = FacilitatorRouter.builder()
        .okx(System.getenv("OKX_API_KEY"),
             System.getenv("OKX_SECRET_KEY"),
             System.getenv("OKX_PASSPHRASE"))
        .build();

// 2. Configure route pricing
PaymentFilter.RouteConfig xlayerUsdt = new PaymentFilter.RouteConfig();
xlayerUsdt.network = "eip155:196";     // X Layer Mainnet
xlayerUsdt.payTo = "0xYourAddress";
xlayerUsdt.price = "$0.01";            // Auto-resolves to USDT atomic units

// 3. One-line middleware
PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /protected/content", xlayerUsdt
));

// 4. Register as servlet filter (Spring Boot, Jetty, Tomcat, etc.)
```

### Client Side — Making Paid Requests

```java
import com.okx.x402.client.OKXHttpClient;
import com.okx.x402.crypto.OKXEvmSigner;
import com.okx.x402.crypto.OKXSignerFactory;
import com.okx.x402.crypto.OKXSignerFactory.OKXSignerConfig;

import java.net.URI;
import java.net.http.HttpResponse;

// 1. Create signer from private key
OKXEvmSigner signer = OKXSignerFactory.createOKXSigner(
        new OKXSignerConfig().privateKey(System.getenv("PRIVATE_KEY")));

// 2. Create auto-402 handling client
OKXHttpClient client = new OKXHttpClient(signer, "eip155:196");

// 3. GET — auto-handles 402 → sign → retry → 200
HttpResponse<String> response = client.get(
        URI.create("https://api.example.com/protected/content"));

System.out.println("Status: " + response.statusCode());  // 200
System.out.println("Body: " + response.body());
```

## Architecture

```
com.okx.x402
├── client/                    # HTTP clients (buyer side)
│   ├── OKXHttpClient          ← V2 auto-402 client
│   ├── HttpFacilitatorClient  ← V1 compat (Coinbase CDP)
│   └── X402HttpClient         ← V1 compat
├── facilitator/               # Facilitator client implementations
│   ├── FacilitatorClient      ← V2 interface
│   ├── OKXFacilitatorClient   ← OKX /api/v6 with HMAC auth
│   └── FacilitatorRouter      ← Network-based routing
├── crypto/                    # Signing
│   ├── EvmSigner              ← V2 EVM signer interface
│   ├── OKXEvmSigner           ← EIP-3009 signing (web3j)
│   ├── OKXSignerFactory       ← Factory with config
│   └── CryptoSigner           ← V1 compat interface
├── model/
│   ├── v2/                    ← V2 protocol types
│   └── v1/                    ← V1 compat types
├── server/
│   └── PaymentFilter          ← V2 servlet filter
├── config/
│   ├── AssetRegistry          ← X Layer USDT/chain config
│   └── AssetConfig            ← Token metadata
└── util/
    ├── Json                   ← Jackson ObjectMapper
    └── OKXAuth                ← HMAC-SHA256 auth headers
```

## Supported Chains

| Chain | Network ID | Token | Contract |
|-------|-----------|-------|----------|
| X Layer Mainnet | `eip155:196` | USDT | `0x779ded0c9e1022225f8e0630b35a9b54be713736` |
| X Layer Testnet | `eip155:195` | USDT | TBD |

Custom chains/tokens can be registered via `AssetRegistry.register()`.

## Payment Flow

```
Client                    Server                   OKX Facilitator
  │                         │                           │
  │── GET /resource ───────>│                           │
  │<── 402 + PaymentRequired│                           │
  │                         │                           │
  │  (sign EIP-3009)        │                           │
  │                         │                           │
  │── GET + PAYMENT-SIGNATURE>                          │
  │                         │── verify ────────────────>│
  │                         │<── isValid: true ─────────│
  │                         │                           │
  │<── 200 + content ───────│                           │
  │  + PAYMENT-RESPONSE     │── settle ────────────────>│
  │                         │<── success + txHash ──────│
```

## Error Handling

| HTTP Status | Scenario |
|------------|----------|
| 402 | Payment required / invalid / settlement failed |
| 500 | Facilitator unreachable / internal error |

OKX error codes (e.g. `50103`) are mapped to readable messages.

## Examples

See the [`examples/`](examples/) directory for complete demo server and client code.

## Environment Variables

| Variable | Usage |
|----------|-------|
| `OKX_API_KEY` | OKX API key (server) |
| `OKX_SECRET_KEY` | OKX secret key (server) |
| `OKX_PASSPHRASE` | OKX passphrase (server) |
| `PRIVATE_KEY` | 0x-prefixed hex private key (client) |
