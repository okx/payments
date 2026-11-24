# x402 demo servers

Three runnable HTTP servers exposing identical x402-paid endpoints across
`net/http`, Gin, and Echo:

- `nethttp/` — `go run ./demo/x402/nethttp`
- `gin/`     — `go run ./demo/x402/gin`
- `echo/`    — `go run ./demo/x402/echo`

Common endpoints:

- `GET /health`         — health check (no payment)
- `GET /resource/sync`  — `exact` / `aggr_deferred` paid resource (sync settle, OKX facilitator)
- `GET /resource/async` — `exact` / `aggr_deferred` paid resource (async settle, OKX facilitator)
- `GET /resource/upto`  — `upto`-scheme paid resource (Permit2 + EIP-2612 gas-sponsored settle)

The sync/async routes require `OKX_BASE_URL` (plus the OKX_* credentials)
and are skipped when unset. The upto route requires `FACILITATOR_ADDRESS`
(the on-chain address authorized to settle via `x402UptoPermit2Proxy`) and
is skipped when unset — so each route is independently opt-in.

## upto demo

Start (example, nethttp):

```sh
PAY_TO_ADDRESS=0x... \
PAY_TO_ADDRESS_ASYNC=0x... \
FACILITATOR_ADDRESS=0x... \
PORT=8402 \
go run -a ./demo/x402/nethttp
```

Probe the upto endpoint:

```sh
curl -i http://localhost:8402/resource/upto
# expect: 402 Payment Required
# body lists "scheme":"upto" with extra.facilitatorAddress
```

Paying: see `go/x402/mechanisms/evm/upto/client/` — the client
`CreateUptoPermit2Payload` produces a base64-JSON X-PAYMENT header that the
server validates and the facilitator settles via `x402UptoPermit2Proxy`.

The same `PAY_TO_ADDRESS` / `FACILITATOR_ADDRESS` env vars work for the
`gin` and `echo` demos — just swap the package path:

```sh
go run -a ./demo/x402/gin
go run -a ./demo/x402/echo
```
