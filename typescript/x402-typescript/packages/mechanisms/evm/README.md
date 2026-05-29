# @okxweb3/x402-evm

EVM implementation of the x402 payment protocol for X Layer. Supports EIP-3009 `transferWithAuthorization`, Permit2-based metered billing (`upto`), and aggregated deferred payment schemes.

## Installation

```bash
npm install @okxweb3/x402-evm
```

## Supported Chain

| Chain | CAIP-2 ID | Default Stablecoin | Transfer Method |
| --- | --- | --- | --- |
| **X Layer** | `eip155:196` | USDT0 | EIP-3009 |

## Supported Schemes

- **exact** — Standard payment. The exact amount is transferred on-chain via EIP-3009 `transferWithAuthorization`.
- **upto** — Cap-style metered billing. The buyer signs a Permit2 `PermitWitnessTransferFrom` for a **maximum** amount; at settlement time the route handler decides the actual charge (≤ cap). The witness binds the facilitator address, so only the authorized facilitator can settle. Settled via the on-chain `x402UptoPermit2Proxy` (`0x4020e7393B728A3939659E5732F87fdd8e680002`).
- **aggr_deferred** — Aggregated deferred payment using session keys. The facilitator batches multiple payments on-chain for high-throughput scenarios.

## Exports

### Main entry (`@okxweb3/x402-evm`)

- `ExactEvmScheme` — Client-side Exact scheme (EIP-3009 or Permit2, picked from the seller's `accepts[].extra.assetTransferMethod`)
- `UptoEvmScheme` — Client-side Upto scheme (Permit2 cap-style billing)
- `AggrDeferredEvmClientScheme` / `AggrDeferredEvmServerScheme` — Aggregated deferred payment scheme
- `toClientEvmSigner(account)` / `toFacilitatorEvmSigner(wallet)` — Convert viem objects to x402 signers
- Type guards: `isEIP3009Payload`, `isPermit2Payload`, `isUptoPermit2Payload`

### Subpath exports

| Path | Description |
| --- | --- |
| `@okxweb3/x402-evm/exact/client` | Client-side Exact scheme (signs EIP-3009 or Permit2) |
| `@okxweb3/x402-evm/exact/server` | Server-side Exact scheme (builds payment requirements) |
| `@okxweb3/x402-evm/exact/facilitator` | Facilitator Exact scheme (verifies & settles payments) |
| `@okxweb3/x402-evm/upto/client` | Client-side Upto scheme (signs the Permit2 cap) |
| `@okxweb3/x402-evm/upto/server` | Server-side Upto scheme (builds metered payment requirements) |
| `@okxweb3/x402-evm/upto/facilitator` | Facilitator Upto scheme (verifies the cap & settles the actual amount) |
| `@okxweb3/x402-evm/deferred/client` | Deferred payment client scheme |
| `@okxweb3/x402-evm/deferred/server` | Deferred payment server scheme |

## Usage

### Client-side (signing payments)

```typescript
import { x402Client } from "@okxweb3/x402-core/client";
import { ExactEvmScheme, toClientEvmSigner } from "@okxweb3/x402-evm";
import { privateKeyToAccount } from "viem/accounts";

const account = privateKeyToAccount("0x...");
const signer = toClientEvmSigner(account);

const client = new x402Client()
  .register("eip155:196", new ExactEvmScheme(signer));
```

### Server-side (building payment requirements)

```typescript
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";

const evmServer = new ExactEvmScheme();
```

### Facilitator (verifying & settling payments)

```typescript
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/facilitator";
import { toFacilitatorEvmSigner } from "@okxweb3/x402-evm";

const facilitator = new ExactEvmScheme(toFacilitatorEvmSigner(walletClient));
```

### Upto scheme (metered billing)

Server registers the upto scheme and decides the actual charge per request via `setSettlementOverrides`:

```typescript
import { UptoEvmScheme } from "@okxweb3/x402-evm/upto/server";
import { paymentMiddleware, setSettlementOverrides, x402ResourceServer } from "@okxweb3/x402-express";

const resourceServer = new x402ResourceServer(facilitatorClient)
  .register("eip155:196", new UptoEvmScheme());

app.use(paymentMiddleware({
  "POST /summarize": {
    accepts: {
      scheme: "upto",
      network: "eip155:196",
      payTo: SELLER_ADDRESS,
      price: "$1.00",          // signed cap
    },
    description: "Summarizer — billed by input size, capped at $1.00",
    mimeType: "application/json",
  },
}, resourceServer));

app.post("/summarize", (req, res) => {
  // Decide the actual charge based on the request; must be ≤ the signed cap.
  setSettlementOverrides(res, { amount: actualAtomicUnits });
  res.json({ /* ... */ });
});
```

Client (and facilitator) wire up the same way as Exact, using `UptoEvmScheme` from `@okxweb3/x402-evm/upto/client` and `/upto/facilitator`.

## Asset Transfer Method

Tokens on X Layer use **EIP-3009** (`transferWithAuthorization`) by default, which provides gasless, single-signature token transfers. The default stablecoin is USDT0.

For the **upto** scheme — and for the `exact` sub-mode that opts into `extra.assetTransferMethod: "permit2"` — settlement runs through the canonical **Permit2** contract (`0x000000000022D473030F116dDEE9F6B43aC78BA3`, same address on every EVM chain) plus an x402 settle proxy. The buyer MUST perform a one-time `IERC20.approve(PERMIT2_ADDRESS, MAX_UINT256)` for the token before signing; after that, every subsequent payment is off-chain signatures only.

| Path | On-chain proxy (settle target) | Witness fields |
| --- | --- | --- |
| `exact` (EIP-3009)                   | n/a — direct EIP-3009 call on the token | n/a |
| `exact` + `assetTransferMethod=permit2` | `0x402085c248EeA27D92E8b30b2C58ed07f9E20001` | `(to, validAfter)` |
| `upto`                                | `0x4020e7393B728A3939659E5732F87fdd8e680002` | `(to, facilitator, validAfter)` |

The upto witness binds the facilitator address: a leaked cap-signature can only be settled by the exact facilitator the buyer named.

## Related Packages

- `@okxweb3/x402-core` — Core protocol types and client
