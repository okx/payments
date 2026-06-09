# okx-x402-demo

x402 seller demo on **X Layer testnet** (`eip155:1952`). Uses USDT0 as the default settlement asset (configured in `@okxweb3/app-x402-evm` default-asset registry).

## Setup

```bash
cp .env.example .env
# edit .env with your OKX API credentials and SELLER_ADDRESS
```

Required environment variables:

| Name | Description |
|---|---|
| `OKX_API_KEY` / `OKX_SECRET_KEY` / `OKX_PASSPHRASE` | OKX facilitator API credentials (HMAC-SHA256 signing) |
| `SELLER_ADDRESS` | Receiving address on X Layer testnet |

## Run

```bash
pnpm install                      # at workspace root
pnpm --filter okx-x402-demo start
```

Server listens on `http://localhost:3000` by default (override via `PORT`).

## Protected routes

| Route | Price | Description |
|---|---|---|
| `GET /weather` | `$0.01` | Single weather lookup |
| `GET /report` | `$0.05` | Detailed multi-day weather report |

`GET /` is a public health check; it returns `{ status, network, protectedRoutes }`.

## How it works

- The seller registers `ExactEvmScheme` for `eip155:1952`.
- Prices are USD strings; the SDK auto-converts to USDT0 atomic units using the testnet entry in `DEFAULT_STABLECOINS` (`0x9e29b3aada05bf2d2c827af80bd28dc0b9b4fb0c`, 6 decimals).
- The Express middleware intercepts requests to protected routes, returns 402 with `PAYMENT-REQUIRED` when no valid `X-PAYMENT` header is present, and forwards verified requests through to the handler.
- On a successful 2xx response, the middleware calls the facilitator's `settle` endpoint and attaches `PAYMENT-RESPONSE` to the client response.
