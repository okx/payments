# OKX Payments — Rust SDK

Rust SDKs for the **x402** and **MPP** payment protocols. Designed for
resource servers (sellers) that need to charge API consumers per request,
with support for both single-shot purchases and streaming sessions.

## Crates

| Crate | Purpose |
|---|---|
| [`x402-core`](./x402/x402-core)               | x402 protocol core: types, server logic, facilitator client, HMAC signing |
| [`x402-evm`](./x402/x402-evm)                 | x402 EVM mechanism (X Layer + EVM-compatible chains) |
| [`x402-axum`](./x402/x402-axum)               | x402 axum middleware (Tower Layer / Service) |
| [`mpp-evm`](./mpp)                            | OKX MPP EVM Seller SDK (SA API integration) |
| [`payment-router-axum`](./payment-router-axum) | Dual-protocol (x402 + MPP) axum router |

Each crate is independently versioned and released. Pick only what you
need — x402, MPP, or both.

## Quick install

> **Phase 1 distribution: GitHub only.** crates.io publication is on the
> roadmap; for now use a `git` dependency with a per-crate tag.

**Only x402** — any axum server that wants per-request payments via the
x402 protocol:

```toml
[dependencies]
x402-axum = { git = "https://github.com/okx/payments", tag = "x402-axum-v0.1.0" }
```

**Only MPP** — OKX merchants integrating the Machine Payments Protocol
(channel-based pay-per-use):

```toml
[dependencies]
mpp-evm = { git = "https://github.com/okx/payments", tag = "mpp-evm-v0.1.0" }
```

**Both protocols** — let HTTP clients pick whichever they support; the
router auto-detects the request scheme and dispatches to the right
backend:

```toml
[dependencies]
payment-router-axum = { git = "https://github.com/okx/payments", tag = "payment-router-axum-v0.1.0" }
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
