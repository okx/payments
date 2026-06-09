# @okxweb3/app-x402-hono

Hono middleware for the x402 Payment Protocol. Adds x402 payment requirements to Hono applications.

## Installation

```bash
npm install @okxweb3/app-x402-hono
```

## Quick Start

```typescript
import { Hono } from "hono";
import { serve } from "@hono/node-server";
import { paymentMiddleware, x402ResourceServer } from "@okxweb3/app-x402-hono";
import { ExactEvmScheme } from "@okxweb3/app-x402-evm/exact/server";
import { OKXFacilitatorClient } from "@okxweb3/app-x402-core";

const app = new Hono();

const facilitatorClient = new OKXFacilitatorClient();
const resourceServer = new x402ResourceServer(facilitatorClient)
  .register("eip155:196", new ExactEvmScheme());

app.use(
  paymentMiddleware(
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
  ),
);

app.get("/protected-route", (c) => {
  return c.json({ message: "Premium content" });
});

serve({ fetch: app.fetch, port: 3000 });
```

## API Reference

### paymentMiddleware

```typescript
function paymentMiddleware(
  routes: RoutesConfig,
  server: x402ResourceServer,
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart?: boolean,
): MiddlewareHandler;
```

Creates Hono middleware that:

1. Checks if the incoming request matches a protected route
2. Validates payment headers if required
3. Returns payment instructions (402 status) if payment is missing or invalid
4. Processes the request if payment is valid
5. Handles settlement after successful response

#### Parameters

| Parameter | Required | Description |
|-----------|----------|-------------|
| `routes` | Yes | Route configurations for protected endpoints |
| `server` | Yes | Pre-configured `x402ResourceServer` instance |
| `paywallConfig` | No | Configuration for the built-in paywall UI shown to browser visitors (`Accept: text/html` + `Mozilla` UA). Ignored for API/SDK clients. |
| `paywall` | No | Custom `PaywallProvider` that overrides the default HTML generator. Only used for browser visitors. |
| `syncFacilitatorOnStart` | No | Whether to sync with facilitator on startup (default: `true`) |

### Paywall

When an unpaid request comes from a web browser (`Accept` header contains `text/html` **and** `User-Agent` contains `Mozilla`), the middleware returns an HTML paywall page instead of a JSON 402. API/SDK clients are unaffected — they continue to receive JSON 402 with the `PAYMENT-REQUIRED` header.

```typescript
import { paymentMiddleware, PaywallConfig, PaywallProvider } from "@okxweb3/app-x402-hono";

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

app.use(paymentMiddleware(routes, resourceServer, paywallConfig, customPaywall));
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

app.use(paymentMiddleware(routes, resourceServer));
```

### Multiple Protected Routes

```typescript
app.use(
  paymentMiddleware(
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
  ),
);
```
