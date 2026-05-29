# @okxweb3/x402-fetch

Extends the native `fetch` API to automatically handle HTTP 402 Payment Required responses using the x402 payment protocol. When a request returns 402, the wrapper parses payment requirements, signs a payment payload via the configured EVM scheme, and retries the request with the payment header attached.

## Installation

```bash
pnpm install @okxweb3/x402-fetch @okxweb3/x402-evm @okxweb3/x402-core
```

## Quick Start

```typescript
import { wrapFetchWithPaymentFromConfig } from "@okxweb3/x402-fetch";
import { ExactEvmScheme, toClientEvmSigner } from "@okxweb3/x402-evm";
import { createWalletClient, http } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { xLayer } from "viem/chains";

// Create a viem wallet client as the signer
const account = privateKeyToAccount("0xYourPrivateKey");
const walletClient = createWalletClient({
  account,
  chain: xLayer,
  transport: http(),
});
const signer = toClientEvmSigner(walletClient);

// Wrap fetch with x402 payment handling
const fetchWithPayment = wrapFetchWithPaymentFromConfig(fetch, {
  schemes: [
    {
      network: "eip155:196", // X Layer
      client: new ExactEvmScheme(signer),
    },
  ],
});

// Requests that return 402 are automatically paid and retried
const response = await fetchWithPayment("https://api.example.com/paid-endpoint");
const data = await response.json();
```

## API Reference

### `wrapFetchWithPayment(fetch, client)`

Wraps the native fetch to handle 402 responses automatically.

| Parameter | Type | Description |
|-----------|------|-------------|
| `fetch` | `typeof globalThis.fetch` | The fetch function to wrap |
| `client` | `x402Client \| x402HTTPClient` | An x402Client instance with registered payment schemes |

Returns a wrapped fetch function with the same signature as the native `fetch`.

### `wrapFetchWithPaymentFromConfig(fetch, config)`

Convenience wrapper that creates an x402Client from a configuration object.

| Parameter | Type | Description |
|-----------|------|-------------|
| `fetch` | `typeof globalThis.fetch` | The fetch function to wrap |
| `config` | `x402ClientConfig` | Configuration object (see below) |

**`config` properties:**

- `schemes` -- Array of scheme registrations:
  - `network` -- Network identifier (e.g. `"eip155:196"` for X Layer, or `"eip155:*"` for all EVM chains)
  - `client` -- Scheme client instance (e.g. `ExactEvmScheme`)
- `paymentRequirementsSelector` -- Optional function to choose among multiple payment options

### `decodePaymentResponseHeader(header)`

Decodes the `PAYMENT-RESPONSE` header returned by the server after a successful payment.

### Re-exported types

`x402Client`, `x402HTTPClient`, `PaymentPolicy`, `SchemeRegistration`, `SelectPaymentRequirements`, `x402ClientConfig`, `Network`, `PaymentPayload`, `PaymentRequired`, `PaymentRequirements`, `SchemeNetworkClient`

## Examples

### Wildcard EVM support

```typescript
import { wrapFetchWithPaymentFromConfig, decodePaymentResponseHeader } from "@okxweb3/x402-fetch";
import { ExactEvmScheme, toClientEvmSigner } from "@okxweb3/x402-evm";
import { createWalletClient, http } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { xLayer } from "viem/chains";

const signer = toClientEvmSigner(
  createWalletClient({
    account: privateKeyToAccount(process.env.EVM_PRIVATE_KEY as `0x${string}`),
    chain: xLayer,
    transport: http(),
  }),
);

const fetchWithPayment = wrapFetchWithPaymentFromConfig(fetch, {
  schemes: [
    {
      network: "eip155:*", // match any EVM chain
      client: new ExactEvmScheme(signer),
    },
  ],
});

const response = await fetchWithPayment("https://api.example.com/paid-endpoint");
const paymentResponse = response.headers.get("PAYMENT-RESPONSE");
if (paymentResponse) {
  console.log("Payment details:", decodePaymentResponseHeader(paymentResponse));
}
```

### Builder pattern with `x402Client`

```typescript
import { wrapFetchWithPayment, x402Client } from "@okxweb3/x402-fetch";
import { ExactEvmScheme, toClientEvmSigner } from "@okxweb3/x402-evm";
import { createWalletClient, http } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { xLayer } from "viem/chains";

const signer = toClientEvmSigner(
  createWalletClient({
    account: privateKeyToAccount("0xYourPrivateKey"),
    chain: xLayer,
    transport: http(),
  }),
);

const client = new x402Client()
  .register("eip155:196", new ExactEvmScheme(signer));

const fetchWithPayment = wrapFetchWithPayment(fetch, client);
```
