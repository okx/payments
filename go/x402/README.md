# x402 Go Package

Go implementation of the x402 protocol - a standard for HTTP 402 Payment Required responses with cryptocurrency micropayments.

## What is x402?

x402 is a protocol that enables HTTP resources to require cryptocurrency payments. When a client requests a paid resource, the server responds with `402 Payment Required` along with payment details. The client creates a payment, retries the request, and receives the resource after successful payment verification and settlement.

## Installation

```bash
go get github.com/okx/payments/go/x402
```

## What This Package Exports

This package provides modules to support the x402 protocol in Go applications.

### Core Classes

The package exports three core types that can be used by clients, servers, and facilitators:

- **`x402.X402Client`** - Creates payment payloads for clients making paid requests
- **`x402.X402ResourceServer`** - Verifies payments and builds requirements for servers accepting payments
- **`x402.X402Facilitator`** - Verifies and settles payments for facilitator services

These core classes are **framework-agnostic** and can be used in any context (HTTP, gRPC, WebSockets, CLI tools, etc.).

### HTTP Transport Wrappers

The package exports HTTP-specific wrappers around the core classes:

- **`x402http.HTTPClient`** - Wraps `http.Client` with automatic payment handling for clients
- **`x402http.HTTPServer`** - Integrates resource server with HTTP request processing
- **`x402http.HTTPFacilitatorClient`** - HTTP client for calling facilitator endpoints

These wrappers handle HTTP-specific concerns like headers, status codes, and request/response serialization.

### Middleware for Servers

Framework-specific middleware packages for easy server integration:

- **`http/gin`** - Gin framework middleware
- **`http/echo`** - Echo framework middleware
- **`http/nethttp`** - net/http standard library middleware

Additional framework middleware can be built using the HTTP transport wrappers as a foundation.

### Client Helper Packages

Helper packages to simplify client implementation:

- **`signers/evm`** - EVM signer helpers (creates signers from private keys)
- **`signers/svm`** - SVM signer helpers (creates signers from private keys)

These eliminate 95-99% of boilerplate code for creating signers.

### Mechanism Implementations (Schemes)

Payment scheme implementations that can be registered by clients, servers, and facilitators:

- **`mechanisms/evm/exact`** - Ethereum/EVM exact payment using EIP-3009 or Permit2
    - `client/` - Client-side payment creation
    - `server/` - Server-side payment verification
    - `facilitator/` - Facilitator-side payment settlement

- **`mechanisms/svm/exact`** - Solana exact payment using SPL token transfers
    - `client/` - Client-side payment creation
    - `server/` - Server-side payment verification
    - `facilitator/` - Facilitator-side payment settlement

Each role (client, server, facilitator) has its own mechanism implementation with appropriate functionality for that role.

### Extensions

Protocol extension implementations:

- **`extensions/bazaar`** - API discovery extension for making resources discoverable
- **`extensions/eip2612gassponsor`** - Gasless Permit2 approval via EIP-2612 permit signing
- **`extensions/erc20approvalgassponsor`** - Gasless ERC-20 approval for tokens without EIP-2612
- **`extensions/paymentidentifier`** - Payment identifier tracking for idempotency and auditing

## Architecture

The package is designed with extreme modularity:

### Layered Design

```
┌─────────────────────────────────────────┐
│         Your Application                │
└─────────────────────────────────────────┘
                  │
       ┌──────────┼──────────┐
       ▼          ▼          ▼
  [Client]   [Server]  [Facilitator]
       │          │          │
       ▼          ▼          ▼
┌─────────────────────────────────────────┐
│      HTTP Layer (Optional)              │
│  - HTTPClient wrapper                   │
│  - HTTPResourceServer                   │
│  - Middleware (Gin, Echo, net/http)      │
└─────────────────────────────────────────┘
                  │
       ┌──────────┼──────────┐
       ▼          ▼          ▼
┌─────────────────────────────────────────┐
│    Core Classes (Framework-Agnostic)    │
│  - X402Client                           │
│  - X402ResourceServer                   │
│  - X402Facilitator                      │
└─────────────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────┐
│         Mechanisms (Pluggable)          │
│  - EVM exact (client/server/facil.)    │
│  - SVM exact (client/server/facil.)    │
└─────────────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────┐
│         Signers (Helpers)               │
│  - EVM client signers                   │
│  - SVM client signers                   │
└─────────────────────────────────────────┘
```

### Key Design Principles

1. **Framework-Agnostic Core** - The core client/server/facilitator classes work independently of HTTP or any web framework

2. **HTTP as a Layer** - HTTP functionality is isolated in the `http` package, making the core reusable for other transports

3. **Pluggable Mechanisms** - Payment schemes are modular and can be registered independently by clients, servers, and facilitators

4. **Middleware Wraps Core** - Framework middleware (like Gin) internally uses the core primitives, keeping framework concerns separate

This architecture enables:

- Using core classes in non-HTTP contexts (gRPC, WebSockets, message queues)
- Building custom middleware for any framework
- Registering different mechanisms for different roles
- Mixing and matching components as needed

## Documentation by Role

This package serves three distinct roles. Choose the documentation for what you're building:

### 🔵 **[CLIENT.md](CLIENT.md)** - Building Payment-Enabled Clients

For applications that make requests to payment-protected resources.

**Topics covered:**

- Creating payment-enabled HTTP clients
- Registering payment mechanisms
- Using signer helpers
- Lifecycle hooks and error handling
- Advanced patterns (concurrency, retry logic, custom transports)

**See also:** [`examples/go/clients/`](../examples/go/clients/)

### 🟢 **[SERVER.md](SERVER.md)** - Building Payment-Accepting Servers

For services that protect resources with payment requirements.

**Topics covered:**

- Protecting HTTP endpoints with payments
- Route configuration and pattern matching
- Using middleware (Gin and custom implementations)
- Dynamic pricing and dynamic payment routing
- Verification and settlement handling
- Extensions (Bazaar discovery)

**See also:** [`examples/go/servers/`](../examples/go/servers/)

### 🟡 **[FACILITATOR.md](FACILITATOR.md)** - Building Payment Facilitators

For payment processing services that verify and settle payments.

**Topics covered:**

- Payment signature verification
- On-chain settlement
- Lifecycle hooks for logging and metrics
- Blockchain interaction
- Production deployment considerations
- Monitoring and alerting

**See also:** [`examples/go/facilitator/`](../examples/go/facilitator/), [`e2e/facilitators/go/`](../e2e/facilitators/go/)

## Package Structure

```
github.com/okx/payments/go/x402
│
├── Core (framework-agnostic)
│   ├── client.go              - x402.X402Client
│   ├── server.go              - x402.X402ResourceServer
│   ├── facilitator.go         - x402.X402Facilitator
│   ├── types.go               - Core types
│   └── *_hooks.go             - Lifecycle hooks
│
├── http/                      - HTTP transport layer
│   ├── http.go                - Type aliases and convenience functions
│   ├── client.go              - HTTP client wrapper
│   ├── server.go              - HTTP server integration
│   ├── facilitator_client.go  - Facilitator HTTP client
│   ├── okx_*.go               - OKX facilitator clients and auth
│   ├── gin/                   - Gin middleware
│   ├── echo/                  - Echo middleware
│   └── nethttp/               - net/http middleware
│
├── mechanisms/                - Payment schemes
│   ├── evm/exact/
│   │   ├── client/            - EVM client mechanism
│   │   ├── server/            - EVM server mechanism
│   │   └── facilitator/       - EVM facilitator mechanism
│   └── svm/exact/
│       ├── client/            - SVM client mechanism
│       ├── server/            - SVM server mechanism
│       └── facilitator/       - SVM facilitator mechanism
│
├── signers/                   - Signer helpers
│   ├── evm/                   - EVM client signers (+ OKX signer)
│   └── svm/                   - SVM client signers
│
├── extensions/                - Protocol extensions
│   ├── bazaar/                - API discovery
│   ├── eip2612gassponsor/     - EIP-2612 gas sponsoring
│   ├── erc20approvalgassponsor/ - ERC-20 approval gas sponsoring
│   ├── paymentidentifier/     - Payment identifier tracking
│   └── types/                 - Extension type definitions
│
├── mcp/                       - MCP (Model Context Protocol) integration
│
└── types/                     - Type definitions
    ├── v1.go                  - V1 protocol types
    ├── v2.go                  - V2 protocol types
    ├── helpers.go             - Version detection utilities
    ├── raw.go                 - Raw type handling
    └── extensions.go          - Extension type definitions
```

## Supported Networks

### EVM (Ethereum Virtual Machine)

EVM-compatible chains with pre-configured defaults (CAIP-2 identifiers):

- Base Mainnet (`eip155:8453`) — USDC, EIP-3009
- Base Sepolia (`eip155:84532`) — USDC, EIP-3009
- MegaETH (`eip155:4326`) — USDM, Permit2
- Monad (`eip155:143`) — USDC, EIP-3009
- Mezo Testnet (`eip155:31611`) — mUSD, Permit2
- X Layer (`eip155:196`) — USDT, EIP-3009
- Stable (`eip155:988`) — USDT0, EIP-3009

Use `eip155:*` wildcard to support all EVM chains.

### SVM (Solana Virtual Machine)

All Solana networks using CAIP-2 identifiers:

- Solana Mainnet (`solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp`)
- Solana Devnet (`solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1`)
- Solana Testnet (`solana:4uhcVJyU9pJkvQyS88uRDiswHXSCkY3z`)

Use `solana:*` wildcard to support all Solana networks.

## Supported Schemes

### Exact Payment

Transfer an exact amount to access a resource:

- **EVM**: Uses EIP-3009 `transferWithAuthorization` (default for USDC-like tokens) or Permit2 via `x402Permit2Proxy` (for ERC-20 tokens without EIP-3009)
- **SVM**: Uses Solana SPL token transfers with memo (USDC SPL token)

## Features

- ✅ Protocol v2 with v1 backward compatibility
- ✅ Multi-chain support (EVM and SVM)
- ✅ Modular architecture - use core primitives directly or with helpers
- ✅ Type safe with strong typing throughout
- ✅ Framework agnostic core
- ✅ Concurrent safe operations
- ✅ Context-aware with proper cancellation support
- ✅ Extensible plugin architecture
- ✅ Production ready with comprehensive testing
- ✅ Lifecycle hooks for customization

## Package Documentation

### Core Documentation

- **[CLIENT.md](CLIENT.md)** - Building payment-enabled clients
- **[SERVER.md](SERVER.md)** - Building payment-accepting servers
- **[FACILITATOR.md](FACILITATOR.md)** - Building payment facilitators

### Component Documentation

- **[signers/](signers/README.md)** - Signer helper utilities
- **[mechanisms/evm/](mechanisms/evm/README.md)** - EVM payment mechanisms
- **[mechanisms/svm/](mechanisms/svm/README.md)** - SVM payment mechanisms
- **[extensions/](extensions/)** - Protocol extensions
- **[mcp/](mcp/README.md)** - MCP (Model Context Protocol) integration

### Examples

- **[examples/go/clients/](../examples/go/clients/)** - Client implementation examples
- **[examples/go/servers/](../examples/go/servers/)** - Server implementation examples
- **[examples/go/facilitator/](../examples/go/facilitator/)** - Facilitator example

## Testing

```bash
# Run unit tests
make test
# or: go test -race -cover ./...

# Run with HTML coverage report
make test-cover

# Run integration tests (requires .env with keys/RPCs)
make test-integration
# or: go test -v -race -tags="integration,mcp" ./test/integration/...

# Run integration tests without MCP
make test-integration-no-mcp
# or: go test -v -race -tags=integration ./test/integration/...

# Run e2e tests
make test-e2e
```

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for contribution guidelines.

## License

Apache 2.0 - See [LICENSE](../LICENSE) for details.
