# x402-evm

EVM mechanism implementation for the [x402 payment protocol](https://x402.org).

Supports X Layer chain configuration and the deferred (aggregator) scheme.

## Most users want a higher-level crate

If you're running an axum HTTP service that accepts EVM-token payments,
depend on [`x402-axum`](../x402-axum) — it pulls `x402-evm` in
transitively and exposes the EVM mechanism through the standard
middleware surface. Direct dependence on `x402-evm` is for:

- Building non-HTTP servers (RPC, gRPC, custom protocol).
- Implementing alternative settlement strategies on EVM chains.
- Extending EVM scheme support beyond the bundled set.

## Install

Published as `okxweb3-app-x402-evm`; `use` path is `x402_evm::*`.

```toml
[dependencies]
okxweb3-app-x402-evm = "0.2"
```

```rust
use x402_evm::ExactEvmScheme;
```

## What's inside

- `types` — EVM-specific payload types (EIP-3009 authorization, Permit2).
- `constants` — Chain configurations for X Layer and other supported
  EVM chains, plus default stablecoin pre-registration.
- `exact` — Server implementation of the `exact` scheme on EVM (single
  EIP-3009 transferWithAuthorization per request).

See `cargo doc --open -p x402-evm` for the full module tree.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
