# x402-core

Core types, server logic, facilitator client, and HTTP utilities for the
[x402 payment protocol](https://x402.org).

Mirrors `@x402/core` from Coinbase's x402 TypeScript SDK.

## Most users want a higher-level crate

If you're building an axum HTTP service, depend on
[`x402-axum`](../x402-axum) instead — it re-exports the public surface
of `x402-core` you'll need (`RoutesConfig`, `X402ResourceServer`, hook
types, etc.) and adds the Tower middleware. Direct dependence on
`x402-core` is for:

- Implementing protocol extensions (custom schemes, custom mechanisms).
- Building middleware for non-axum frameworks (actix, warp, etc.).
- Embedding x402 verification into a non-HTTP context.

## Install

```toml
[dependencies]
x402-core = { git = "https://github.com/okx/payments", tag = "x402-core-v0.1.0" }
```

## What's inside

- `types` — `PaymentRequirements`, `PaymentPayload`, `Scheme`, `Network`,
  and the rest of the wire-format types from the x402 spec.
- `error` — Unified `X402Error` enum with all protocol-defined error codes.
- `schemas` — Validation logic (Rust equivalent of the TypeScript Zod
  schemas from upstream x402).
- `server` — `X402ResourceServer`: the per-call API for verifying and
  settling payments against a facilitator.
- `http` — Per-route configuration types and lifecycle hooks shared with
  HTTP-framework adapters (`x402-axum`).
- `http_facilitator_client` — HTTP client to a remote facilitator service.

See [`docs.rs`](https://docs.rs/x402-core) (when published) or
`cargo doc --open -p x402-core` for the full module tree.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
