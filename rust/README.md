# OKX Payments — Rust SDK

Rust implementations of payment protocols for resource servers (sellers).

## Layout

```
x402/         x402 payment protocol (Coinbase-compatible)
  ├─ x402-core     Core types, server logic, facilitator client, HMAC signing
  ├─ x402-axum     Axum middleware (Tower Layer/Service)
  ├─ x402-evm      EVM mechanism, X Layer asset pre-registration
  └─ examples      x402 example servers
mpp/          MPP (Machine Payments Protocol) — OKX
  ├─ (mpp-evm)     Seller SDK + SA API client
  └─ examples      MPP example servers
```

Each crate is independently publishable. Consumers can depend on only x402,
only MPP, or combine both via a higher-level crate.

## ⚠️ Cargo consumer caveat: submodule fetch

This monorepo contains Solidity submodules under `contracts/evm/lib/`
(forge-std, OpenZeppelin, Permit2) that Rust consumers don't need. Cargo
clones them by default, adding ~500 MB and a hard dependency on GitHub
availability.

If you hit network errors resolving `mpp-evm` or `x402-*` as a git dependency,
enable CLI-backed git fetch (which skips submodules when `--recurse-submodules`
is not requested) in `~/.cargo/config.toml`:

```toml
[net]
git-fetch-with-cli = true
```

Or run `cargo --offline` after a successful first fetch. A cleaner long-term
fix is to host the Rust crates in a dedicated repository.
