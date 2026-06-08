# @okxweb3/app-x402-core

Core implementation of the x402 payment protocol for TypeScript/JavaScript applications. Provides transport-agnostic client, server, and facilitator components for EVM-based payment flows.

## Installation

```bash
pnpm install @okxweb3/app-x402-core
```

## Quick Start

### Client Usage

```typescript
import { x402Client } from '@okxweb3/app-x402-core/client';
import { x402HTTPClient } from '@okxweb3/app-x402-core/http';
import { ExactEvmScheme } from '@okxweb3/app-x402-evm/exact/client';

// Create core client and register EVM payment scheme
const coreClient = new x402Client()
  .register('eip155:*', new ExactEvmScheme(evmSigner));

// Wrap with HTTP client for header encoding/decoding
const client = new x402HTTPClient(coreClient);

// Make a request
const response = await fetch('https://api.example.com/protected');

if (response.status === 402) {
  // Extract payment requirements from response
  const paymentRequired = client.getPaymentRequiredResponse(
    (name) => response.headers.get(name),
    await response.json()
  );

  // Create and send payment
  const paymentPayload = await client.createPaymentPayload(paymentRequired);

  const paidResponse = await fetch('https://api.example.com/protected', {
    headers: client.encodePaymentSignatureHeader(paymentPayload),
  });

  // Get settlement confirmation
  const settlement = client.getPaymentSettleResponse(
    (name) => paidResponse.headers.get(name)
  );
  console.log('Transaction:', settlement.transaction);
}
```

### Server Usage

```typescript
import { x402ResourceServer } from '@okxweb3/app-x402-core/server';
import { x402HTTPResourceServer } from '@okxweb3/app-x402-core/http';
import { OKXFacilitatorClient } from '@okxweb3/app-x402-core';
import { ExactEvmScheme } from '@okxweb3/app-x402-evm/exact/server';

// Connect to OKX facilitator
const facilitatorClient = new OKXFacilitatorClient({
  apiKey: 'YOUR_API_KEY',
  secretKey: 'YOUR_SECRET_KEY',
  passphrase: 'YOUR_PASSPHRASE',
});

// Create resource server with EVM payment scheme
const resourceServer = new x402ResourceServer(facilitatorClient)
  .register('eip155:*', new ExactEvmScheme());

// Initialize (fetches supported kinds from facilitator)
await resourceServer.initialize();

// Configure routes with payment requirements
const routes = {
  'GET /api/data': {
    accepts: {
      scheme: 'exact',
      network: 'eip155:196',
      payTo: '0xYourAddress',
      price: '$0.01',
    },
    description: 'Premium data access',
    mimeType: 'application/json',
  },
};

// Create HTTP server wrapper
const httpServer = new x402HTTPResourceServer(resourceServer, routes);
```

## OKXFacilitatorClient

The `OKXFacilitatorClient` connects to the OKX facilitator service using HMAC-SHA256 authentication.

```typescript
import { OKXFacilitatorClient } from '@okxweb3/app-x402-core';
import type { OKXConfig } from '@okxweb3/app-x402-core';

const config: OKXConfig = {
  apiKey: 'YOUR_API_KEY',
  secretKey: 'YOUR_SECRET_KEY',
  passphrase: 'YOUR_PASSPHRASE',
  baseUrl: 'https://www.okx.com',   // optional, defaults to https://www.okx.com
  syncSettle: false,                  // optional, when true the facilitator waits for on-chain confirmation
};

const client = new OKXFacilitatorClient(config);
```

## Route Configuration

Routes use the `accepts` field to define payment options:

```typescript
const routes = {
  // Single payment option
  'GET /api/data': {
    accepts: {
      scheme: 'exact',
      network: 'eip155:196',
      payTo: '0xAddress',
      price: '$0.01',
    },
    description: 'Data endpoint',
    mimeType: 'application/json',
  },

  // XLayer payment option
  'POST /api/*': {
    accepts: [
      {
        scheme: 'exact',
        network: 'eip155:196',
        payTo: '0xAddress',
        price: '$0.05',
      },
    ],
  },
};
```

## Client Configuration

Use `fromConfig()` for declarative setup:

```typescript
const client = x402Client.fromConfig({
  schemes: [
    { network: 'eip155:196', client: new ExactEvmScheme(evmSigner) },
  ],
  policies: [
    // Filter by max price
    (version, reqs) => reqs.filter(r => BigInt(r.amount) < BigInt('1000000')),
  ],
});
```

## Lifecycle Hooks

### Client Hooks

```typescript
client
  .onBeforePaymentCreation(async (ctx) => {
    console.log('Creating payment for:', ctx.selectedRequirements.network);
    // Return { abort: true, reason: '...' } to cancel
  })
  .onAfterPaymentCreation(async (ctx) => {
    console.log('Payment created:', ctx.paymentPayload);
  })
  .onPaymentCreationFailure(async (ctx) => {
    console.error('Payment failed:', ctx.error);
    // Return { recovered: true, payload: ... } to recover
  });
```

### Server Hooks

```typescript
resourceServer
  .onBeforeVerify(async (ctx) => { /* ... */ })
  .onAfterVerify(async (ctx) => { /* ... */ })
  .onBeforeSettle(async (ctx) => { /* ... */ })
  .onAfterSettle(async (ctx) => { /* ... */ });
```

### Facilitator Hooks

```typescript
facilitator
  .onBeforeVerify(async (ctx) => { console.log('Before verify', ctx); })
  .onAfterVerify(async (ctx) => { console.log('After verify', ctx); })
  .onVerifyFailure(async (ctx) => { console.log('Verify failure', ctx); })
  .onBeforeSettle(async (ctx) => { console.log('Before settle', ctx); })
  .onAfterSettle(async (ctx) => { console.log('After settle', ctx); })
  .onSettleFailure(async (ctx) => { console.log('Settle failure', ctx); });
```

## HTTP Headers

| Header | Description |
|--------|-------------|
| `PAYMENT-SIGNATURE` | Base64-encoded payment payload |
| `PAYMENT-REQUIRED` | Base64-encoded payment requirements |
| `PAYMENT-RESPONSE` | Base64-encoded settlement response |

## Network Pattern Matching

Register handlers for EVM network families using wildcards:

```typescript
// All EVM networks
server.register('eip155:*', new ExactEvmScheme());

// Specific network takes precedence
server.register('eip155:196', new ExactEvmScheme());
```

## Types

```typescript
type Network = `${string}:${string}`; // e.g., "eip155:196"

type PaymentRequirements = {
  scheme: string;
  network: Network;
  asset: string;
  amount: string;
  payTo: string;
  maxTimeoutSeconds: number;
  extra: Record<string, unknown>;
};

type PaymentPayload = {
  x402Version: number;
  resource: ResourceInfo;
  accepted: PaymentRequirements;
  payload: Record<string, unknown>;
  extensions?: Record<string, unknown>;
};

type PaymentRequired = {
  x402Version: number;
  error?: string;
  resource: ResourceInfo;
  accepts: PaymentRequirements[];
  extensions?: Record<string, unknown>;
};
```

## Subpath Exports

| Path | Description |
|------|-------------|
| `@okxweb3/app-x402-core` | Main entry: `x402Version`, `OKXFacilitatorClient`, `OKXConfig` |
| `@okxweb3/app-x402-core/client` | Client: `x402Client`, `x402HTTPClient` |
| `@okxweb3/app-x402-core/server` | Server: `x402ResourceServer`, `x402HTTPResourceServer`, `HTTPFacilitatorClient` |
| `@okxweb3/app-x402-core/facilitator` | Facilitator: `x402Facilitator`, `OKXFacilitatorClient` |
| `@okxweb3/app-x402-core/http` | HTTP utilities: encoding/decoding helpers, `x402HTTPResourceServer`, `x402HTTPClient` |
| `@okxweb3/app-x402-core/types` | Shared type definitions |
| `@okxweb3/app-x402-core/schemas` | Zod validation schemas |
| `@okxweb3/app-x402-core/utils` | Utility functions |

## License

Apache-2.0
