# payment-router-axum

Dual-protocol (MPP + x402) payment router Tower Layer for axum. Lets a single
axum app serve both protocols on the same URL via the **Adapter pattern**:

- `Authorization: Payment <b64>` → MPP flow (handled by `MppAdapter`).
- `X-Payment: <b64>` → x402 flow (handled by `X402Adapter`).
- Neither header → 402 with multi-row `WWW-Authenticate` + `PAYMENT-REQUIRED`
  challenges generated in parallel.

The business handler is registered **once** and is protocol-agnostic. Each
adapter wraps the inner service with its own native middleware (no MPP /
x402 SDK is patched or duplicated).

See the crate-level docs (`cargo doc --open -p payment-router-axum`) for the
trait surface and route-matching contract.

## Runnable example

A complete dual-protocol server is in the MPP examples crate at
[`mpp/examples/src/dual_protocol_server.rs`](../mpp/examples/src/dual_protocol_server.rs).

Run with:

```bash
# All MPP credentials required; x402 portion configured per-route.
export MPP_SA_URL=... MPP_SA_KEY=... MPP_SA_SECRET=... MPP_SA_PASSPHRASE=...
export MPP_SECRET_KEY=... MPP_REALM=... MPP_CURRENCY=0x... MPP_RECIPIENT=0x...
export X402_API_KEY=... X402_SECRET_KEY=... X402_PASSPHRASE=...
export X402_PAY_TO=0x...

cargo run --example dual_protocol_server
```

The example lives under `mpp/examples/` rather than this crate's `examples/`
directory so that `payment-router-axum`'s production dependency graph stays
clean — the router is an abstraction layer and shouldn't require either MPP
or x402 SDK as dev-dependencies.

## Testing

```bash
cargo test -p payment-router-axum
```

30 unit tests + adapter / detector / router / merger coverage. No external
services required.
