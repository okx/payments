# @okxweb3/x402-fastify

Fastify middleware for the x402 Payment Protocol. Adds x402 payment requirements to Fastify applications.

## Installation

```bash
npm install @okxweb3/x402-fastify
```

## Quick Start

```typescript
import Fastify from "fastify";
import { paymentMiddleware, x402ResourceServer } from "@okxweb3/x402-fastify";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";
import { OKXFacilitatorClient } from "@okxweb3/x402-core";

const app = Fastify();

const facilitatorClient = new OKXFacilitatorClient();
const resourceServer = new x402ResourceServer(facilitatorClient)
  .register("eip155:196", new ExactEvmScheme());

paymentMiddleware(
  app,
  {
    "GET /protected-route": {
      accepts: {
        scheme: "exact",
        price: "$0.10",
        network: "eip155:196",
        payTo: "0xYourAddress",
      },
      description: "Access to premium content",
    },
  },
  resourceServer,
);

app.get("/protected-route", async () => {
  return { message: "Premium content" };
});

app.listen({ port: 3000 });
```

## API Reference

### paymentMiddleware

```typescript
function paymentMiddleware(
  app: FastifyInstance,
  routes: RoutesConfig,
  server: x402ResourceServer,
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart?: boolean,
): void;
```

Registers Fastify hooks (`onRequest` and `onSend`) that:

1. Check if the incoming request matches a protected route
2. Validate payment headers if required
3. Return payment instructions (402 status) if payment is missing or invalid
4. Process the request if payment is valid
5. Handle settlement after successful response

#### Parameters

| Parameter | Required | Description |
|-----------|----------|-------------|
| `app` | Yes | The Fastify instance to register hooks on |
| `routes` | Yes | Route configurations for protected endpoints |
| `server` | Yes | Pre-configured `x402ResourceServer` instance |
| `paywallConfig` | No | Configuration for the built-in paywall UI shown to browser visitors (`Accept: text/html` + `Mozilla` UA). Ignored for API/SDK clients. |
| `paywall` | No | Custom `PaywallProvider` that overrides the default HTML generator. Only used for browser visitors. |
| `syncFacilitatorOnStart` | No | Whether to sync with facilitator on startup (default: `true`) |

### Paywall

When an unpaid request comes from a web browser (`Accept` header contains `text/html` **and** `User-Agent` contains `Mozilla`), the middleware returns an HTML paywall page instead of a JSON 402. API/SDK clients are unaffected — they continue to receive JSON 402 with the `PAYMENT-REQUIRED` header.

```typescript
import { paymentMiddleware, PaywallConfig, PaywallProvider } from "@okxweb3/x402-fastify";

// Brand the built-in paywall
const paywallConfig: PaywallConfig = {
  appName: "My App",
  appLogo: "https://example.com/logo.png",
};

// Or replace the HTML generator entirely
const customPaywall: PaywallProvider = {
  generateHtml(paymentRequired, config) {
    return `<!DOCTYPE html><html>... your UI ...</html>`;
  },
};

paymentMiddleware(app, routes, resourceServer, paywallConfig, customPaywall);
```

Per-route override is available via `RouteConfig.customPaywallHtml`, which takes precedence over both `paywall` and the default generator. If you don't need a browser paywall (machine-to-machine APIs only), leave both `paywallConfig` and `paywall` as `undefined`.

### Route Configuration

```typescript
const routes: RoutesConfig = {
  "GET /api/protected": {
    accepts: {
      scheme: "exact",
      price: "$0.10",
      network: "eip155:196",
      payTo: "0xYourAddress",
      maxTimeoutSeconds: 60,
    },
    description: "Premium API access",
  },
};

paymentMiddleware(app, routes, resourceServer);
```

### Multiple Protected Routes

```typescript
paymentMiddleware(
  app,
  {
    "GET /api/premium/*": {
      accepts: {
        scheme: "exact",
        price: "$1.00",
        network: "eip155:196",
        payTo: "0xYourAddress",
      },
      description: "Premium API access",
    },
    "GET /api/data": {
      accepts: {
        scheme: "exact",
        price: "$0.50",
        network: "eip155:196",
        payTo: "0xYourAddress",
        maxTimeoutSeconds: 120,
      },
      description: "Data endpoint access",
    },
  },
  resourceServer,
);
```
