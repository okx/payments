# EVM Mechanisms

This directory contains payment mechanism implementations for **EVM (Ethereum Virtual Machine)** networks.

## What This Exports

This package provides scheme implementations for EVM-based blockchains (Ethereum, Base, Optimism, Arbitrum, Polygon, etc.) that can be used by clients, servers, and facilitators.

## Exact Payment Scheme

The **exact** scheme implementation enables fixed-amount payments using EIP-3009 `transferWithAuthorization` or Permit2 for USDC and compatible tokens.

### Export Paths

The exact scheme is organized by role:

#### For Clients

**Import Path:**

```
github.com/okx/payments/go/x402/mechanisms/evm/exact/client
```

**Exports:**

- `NewExactEvmScheme(signer, config)` - Creates client-side EVM exact payment mechanism
- Used for creating payment payloads that clients sign
- Pass `nil` config for signer-only mode
- Use config to provide explicit RPC URLs for extension enrichment (`RPCURL` or `RPCByChainID`)

#### For Servers

**Import Path:**

```
github.com/okx/payments/go/x402/mechanisms/evm/exact/server
```

**Exports:**

- `NewExactEvmScheme()` - Creates server-side EVM exact payment mechanism
- Used for building payment requirements and parsing prices
- Supports custom money parsers via `RegisterMoneyParser()`

#### For Facilitators

**Import Path:**

```
github.com/okx/payments/go/x402/mechanisms/evm/exact/facilitator
```

**Exports:**

- `NewExactEvmScheme(signer)` - Creates facilitator-side EVM exact payment mechanism
- Used for verifying signatures and settling payments on-chain
- Requires facilitator signer with blockchain RPC integration

## Supported Networks

All EVM networks are supported by default. The only consideration is how prices are transformed from money syntax (e.g. `"$0.10"`) to a stablecoin token.

**If prices are defined as `TokenAsset`:** Any EVM chain works out of the box—no additional configuration needed.

**If prices are defined as Money (a USD string like `"$0.10"`):** The server must either:

1. Register a custom money parser in their `ExactEvmScheme` via `RegisterMoneyParser()`, OR
2. Use a chain that has a default asset configuration

Networks with default assets configured:

- **Base Mainnet**: `eip155:8453` (USDC)
- **Base Sepolia**: `eip155:84532` (USDC)
- **MegaETH Mainnet**: `eip155:4326` (MegaUSD)
- **Monad Mainnet**: `eip155:143` (USDC)

To add default asset support for additional chains, see [DEFAULT_ASSET.md](./DEFAULT_ASSET.md).

## Scheme Implementation

The **exact** scheme implements fixed-amount payments:

- **Standard**: EIP-3009 `transferWithAuthorization` or Permit2 (per-asset configuration)
- **Token**: USDC and other stablecoins (EIP-3009 or any ERC-20 via Permit2)
- **Gas**: Paid by facilitator
- **Confirmation**: On-chain settlement with transaction hash

## Upto Payment Scheme

The **upto** scheme implementation enables variable-amount payments up to a signed cap using Permit2 (`PermitWitnessTransferFrom`) with a facilitator-authorized witness. The buyer signs a maximum spend; the facilitator settles for any amount `≤` the signed cap.

Key differences from exact:

- Always uses Permit2 (no EIP-3009 path).
- Witness embeds an extra `facilitator` field — only the listed facilitator address is authorized by the on-chain `x402UptoPermit2Proxy` to call `settle()`.
- Server propagates the facilitator's published address into `paymentRequirements.Extra.facilitatorAddress` so the client can sign the witness with it.
- Settlement amount is supplied separately from the signed cap, enforced both off-chain (verify) and on-chain (proxy revert `AmountExceedsPermitted`).

### Export Paths

The upto scheme is organized by role:

#### For Clients

**Import Path:**

```
github.com/okx/payments/go/x402/mechanisms/evm/upto/client
```

**Exports:**

- `NewUptoEvmScheme(signer)` - Creates client-side EVM upto payment mechanism
- Used for creating Permit2 payment payloads (with facilitator-authorized witness) that clients sign
- Reads `paymentRequirements.Extra["facilitatorAddress"]` to bind the witness to the authorized facilitator

#### For Servers

**Import Path:**

```
github.com/okx/payments/go/x402/mechanisms/evm/upto/server
```

**Exports:**

- `NewUptoEvmScheme()` - Creates server-side EVM upto payment mechanism
- Used for building payment requirements and parsing prices
- Always stamps `extra.assetTransferMethod = "permit2"` and propagates `extra.facilitatorAddress` from the facilitator's supported kind into requirements
- Supports custom money parsers via `RegisterMoneyParser()`
- Also exposes `ValidateUptoPayload(payload, requirements)` — RPC-free off-chain validator used by the facilitator's verify path

#### For Facilitators

**Import Path:**

```
github.com/okx/payments/go/x402/mechanisms/evm/upto/facilitator
```

**Exports:**

- `NewUptoEvmScheme(signer, config)` - Creates facilitator-side EVM upto payment mechanism
- Used for verifying signatures and settling upto payments on-chain via `x402UptoPermit2Proxy.settle(...)`
- Pass `nil` config to accept defaults; supply `&UptoEvmSchemeConfig{SimulateInSettle: true}` to re-simulate at settle time
- Requires facilitator signer with blockchain RPC integration. The first signer address is published via `GetExtra` under the `facilitatorAddress` key

### Caller-Driven Registration

Both schemes register the same way through the public `x402` surface — there is no central registry to edit when adding a new scheme. Each role-specific factory returns a value satisfying the corresponding `SchemeNetworkClient` / `SchemeNetworkServer` / `SchemeNetworkFacilitator` interface, which is then registered against `(network, scheme)`:

```go
// Client
client := x402.Newx402Client()
client.Register("eip155:*", evmexactclient.NewExactEvmScheme(signer, nil))
client.Register("eip155:*", evmuptoclient.NewUptoEvmScheme(signer))

// Resource server
server := x402.Newx402ResourceServer(...)
server.Register("eip155:*", evmexactserver.NewExactEvmScheme())
server.Register("eip155:*", evmuptoserver.NewUptoEvmScheme())

// Facilitator
facil := x402.Newx402Facilitator()
facil.Register([]x402.Network{"eip155:*"}, evmexactfac.NewExactEvmScheme(signer, nil))
facil.Register([]x402.Network{"eip155:*"}, evmuptofac.NewUptoEvmScheme(signer, nil))
```

The scheme identifier each role advertises (`Scheme()`) routes incoming payments: `"exact"` payloads flow to the exact factories, `"upto"` payloads flow to the upto factories.

## Future Schemes

As new payment schemes are developed for EVM networks, they will be added here alongside the exact and upto implementations:

```
evm/
├── exact/          - Fixed amount payments (current)
├── upto/           - Variable amount up to a Permit2-signed cap (current)
├── subscription/   - Recurring payments (planned)
└── batch/          - Batched payments (planned)
```

Each new scheme follows the same three-role structure (client, server, facilitator).

## Contributing New Schemes

We welcome contributions of new payment scheme implementations for EVM networks!

To contribute a new scheme:

1. Create directory structure: `evm/{scheme_name}/client/`, `evm/{scheme_name}/server/`, `evm/{scheme_name}/facilitator/`
2. Implement the required interfaces for each role
3. Add comprehensive tests
4. Document the scheme specification
5. Provide usage examples

See [CONTRIBUTING.md](../../../CONTRIBUTING.md) for more details.

## Related Documentation

- **[Mechanisms Overview](../README.md)** - About mechanisms in general
- **[SVM Mechanisms](../svm/README.md)** - Solana implementations
- **[Exact Scheme Specification](../../../specs/schemes/exact/scheme_exact_evm.md)** - EVM exact scheme spec
