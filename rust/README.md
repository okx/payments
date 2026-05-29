# OKX Payments — Rust SDK

Rust SDKs for the **x402** and **MPP** payment protocols. Designed for
resource servers (sellers) that need to charge API consumers per request,
with support for both single-shot purchases and streaming sessions.

## Crates

| 目录 / Lib alias | Published name (crates.io) | Purpose |
|---|---|---|
| [`x402-core`](./x402/x402-core) | `okxweb3-app-x402-core` | x402 protocol core: types, server logic, facilitator client, HMAC signing |
| [`x402-evm`](./x402/x402-evm) | `okxweb3-app-x402-evm` | x402 EVM mechanism (X Layer + EVM-compatible chains) |
| [`x402-axum`](./x402/x402-axum) | `okxweb3-app-x402-axum` | x402 axum middleware (Tower Layer / Service) |
| [`mpp-evm`](./mpp) | `okxweb3-app-mpp` | OKX MPP EVM Seller SDK (SA API integration) |
| [`payment-router-axum`](./payment-router-axum) | `okxweb3-app-payment-router-axum` | Dual-protocol (x402 + MPP) axum router |

All crates publish under the `okxweb3-app-*` naming convention (org prefix
+ Agent Payments Protocol product identifier + protocol + variant). Each
crate is independently versioned and released — pick only what you need.

## Quick install

**Only x402** — any axum server that wants per-request payments via the
x402 protocol:

```toml
[dependencies]
okxweb3-app-x402-axum = "0.2"
```

**Only MPP** — OKX merchants integrating the Machine Payments Protocol
(channel-based pay-per-use):

```toml
[dependencies]
okxweb3-app-mpp = "0.2"
```

**Both protocols** — let HTTP clients pick whichever they support; the
router auto-detects the request scheme and dispatches to the right
backend:

```toml
[dependencies]
okxweb3-app-payment-router-axum = "0.2"
```

See each crate's own README for usage examples.

## Examples

Runnable example servers live alongside each protocol:

- [`x402/examples`](./x402/examples) — x402 photo / metered API servers.
- [`mpp/examples`](./mpp/examples) — MPP charge / session servers, plus a
  dual-protocol server using `payment-router-axum`.

```bash
cargo run -p x402-examples --example basic_server
cargo run -p mpp-examples --example mpp_photo_server
cargo run -p mpp-examples --example dual_protocol_server
```

## Cargo consumer caveat: submodule fetch

This monorepo contains Solidity submodules under `contracts/evm/lib/`
(forge-std, OpenZeppelin, Permit2) that Rust consumers don't need. Cargo
clones them by default, adding ~500 MB and a hard dependency on GitHub
availability.

If you hit network errors resolving `mpp-evm` or `x402-*` as a git
dependency, enable CLI-backed git fetch (which skips submodules when
`--recurse-submodules` is not requested) in `~/.cargo/config.toml`:

```toml
[net]
git-fetch-with-cli = true
```

Or run `cargo --offline` after a successful first fetch. A cleaner
long-term fix is to host the Rust crates in a dedicated repository.

## License

Licensed under the [Apache License, Version 2.0](./LICENSE). Note that
this differs from the typical Rust ecosystem default of `MIT OR
Apache-2.0` dual licensing — please factor that in when choosing whether
this SDK is compatible with your project.
