# OKX Payments SDK

Multi-language SDK for [x402](https://x402.org) and [MPP](https://mpp.dev/) payment protocols, enabling cryptocurrency micropayments for API resources.

## Languages

| Language   | Path                           | x402 | MPP |
| ---------- | ------------------------------ | ---- | --- |
| Go         | [`go/`](go/)                   | ✅   | ✅  |
| Python     | [`python/`](python/)           | ✅   | ✅  |
| Rust       | [`rust/`](rust/)               | ✅   | ✅  |
| TypeScript | [`typescript/`](typescript/)   | ✅   | ✅  |
| Java       | [`java/`](java/)               | ✅   |     |

## How It Works

[x402](https://x402.org) and [MPP](https://mpp.dev/) both use HTTP 402 Payment Required to gate access to paid resources. When a client requests a resource, the server responds with `402` and payment details. The client signs a payment authorization, retries the request with proof of payment, and receives the resource after verification.

## Supported Networks

- X Layer (`eip155:196`) — USD₮0 (`0x779Ded0c9e1022225f8E0630b35a9b54bE713736`), auto-configured

## Supported Schemes

### x402

- **exact** — Transfer an exact amount per request via EIP-3009
- **aggr_deferred** — Aggregated deferred settlement via facilitator batching

### MPP

- **charge** — One-shot payment per request via EIP-3009
- **session** — Payment channel with incremental vouchers for streaming payments

## Acknowledgments

x402 support is built on the [x402 open standard](https://github.com/coinbase/x402) by Coinbase (Apache 2.0), with extensions for OKX X Layer, aggregated deferred settlement, and TEE wallet signing.

## License

Apache 2.0 — See [LICENSE](LICENSE) for details.