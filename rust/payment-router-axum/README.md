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

## Install

Published as `okxweb3-app-payment-router-axum`; `use` path is `payment_router_axum::*`.

```toml
[dependencies]
okxweb3-app-payment-router-axum = "0.2"
```

```rust
use payment_router_axum::PaymentRouterLayer;
```

`okxweb3-app-mpp`, `okxweb3-app-x402-axum`, `okxweb3-app-x402-core`, and `mpp` are pulled in transitively.

## Runnable example

[`examples/dual_protocol_server.rs`](examples/dual_protocol_server.rs) —
matches the TypeScript `payment-router/demo/dual.ts` shape: one route,
two adapters, single-source per-route typed configs.

```bash
export MPP_SA_URL=... MPP_SA_KEY=... MPP_SA_SECRET=... MPP_SA_PASSPHRASE=...
export MPP_SECRET_KEY=... MPP_REALM=... MPP_CURRENCY=0x... MPP_RECIPIENT=0x...
export X402_API_KEY=... X402_SECRET_KEY=... X402_PASSPHRASE=... X402_PAY_TO=0x...

cargo run -p payment-router-axum --example dual_protocol_server
```

## API shape (Go-aligned)

- **Service-level** (one `EvmMpp` per merchant): `chain_id`,
  `recipient`, `secret_key`, `realm` — set on `EvmConfig`.
- **Method-level** (on `EvmCharge` / `EvmSessionMethod`): SA client,
  `fee_payer`, `splits`, `resource_url`, session signer / store.
- **Per-route** (typed structs on `UnifiedRouteConfig`):
  - `MppRouteConfig { intent, amount, currency, description, ... }`
  - `X402RouteConfig { accepts, description, mime_type, ... }`

`intent: "charge"` or `"session"` picks the EVM method that handles
the route. Add session support by chaining
`.with_session(EvmSessionMethod::new(sa).with_signer(signer))` on the
`EvmMpp` builder.

## Testing

```bash
cargo test -p payment-router-axum
```

Adapter / detector / router / merger / conformance coverage. No external
services required.

## License

Apache-2.0. See [LICENSE](../LICENSE).
