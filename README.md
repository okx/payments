# OKX Payments SDK

Multi-language SDK for the [x402](https://x402.org) HTTP payment protocol, enabling cryptocurrency micropayments for API resources.

Built on the [x402 open standard](https://github.com/coinbase/x402), with extensions for OKX X Layer, aggregated deferred settlement, and TEE wallet signing.

## Languages

| Language | Path             | Status |
| -------- | ---------------- | ------ |
| Go       | [`go/`](go/)     | ✅     |
| Rust     | [`rust/`](rust/) | ✅     |

## What is x402?

x402 is a protocol for HTTP 402 Payment Required responses with cryptocurrency micropayments. When a client requests a paid resource, the server responds with `402 Payment Required` along with payment details. The client signs a payment authorization, retries the request, and receives the resource after verification and settlement.

## Supported Networks

- X Layer (`eip155:196`) — USD₮0 (`0x779Ded0c9e1022225f8E0630b35a9b54bE713736`), auto-configured

## Supported Schemes

- **exact** — Transfer an exact amount per request via EIP-3009
- **aggr_deferred** — Aggregated deferred settlement via facilitator batching

## Acknowledgments

This project is built on the [x402 protocol](https://x402.org) and incorporates code from the [x402 reference implementation](https://github.com/coinbase/x402) by Coinbase, licensed under Apache 2.0. We extend our thanks to the Coinbase team for creating this open standard.

## License

Apache 2.0 — See [LICENSE](LICENSE) for details.
