# @okxweb3/x402-next

Next.js integration for the x402 Payment Protocol. Adds payment protection to Next.js applications using page-level proxy or API route wrappers.

## Installation

```bash
npm install @okxweb3/x402-next
```

## Quick Start

### Protecting Page Routes

Create a `proxy.ts` file in your Next.js project root:

```typescript
// proxy.ts
import { paymentProxy, x402ResourceServer } from "@okxweb3/x402-next";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";
import { OKXFacilitatorClient } from "@okxweb3/x402-core";

const facilitatorClient = new OKXFacilitatorClient();
const resourceServer = new x402ResourceServer(facilitatorClient)
  .register("eip155:196", new ExactEvmScheme());

export const proxy = paymentProxy(
  {
    "/protected": {
      accepts: {
        scheme: "exact",
        price: "$0.01",
        network: "eip155:196",
        payTo: "0xYourAddress",
      },
      description: "Access to protected content",
    },
  },
  resourceServer,
);

export const config = {
  matcher: ["/protected/:path*"],
};
```

### Protecting API Routes

Use the `withX402` wrapper for API routes. This approach guarantees payment settlement only after a successful response (status < 400).

```typescript
// app/api/your-endpoint/route.ts
import { NextRequest, NextResponse } from "next/server";
import { withX402, x402ResourceServer } from "@okxweb3/x402-next";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";
import { OKXFacilitatorClient } from "@okxweb3/x402-core";

const facilitatorClient = new OKXFacilitatorClient();
const server = new x402ResourceServer(facilitatorClient)
  .register("eip155:196", new ExactEvmScheme());

const handler = async (_: NextRequest) => {
  return NextResponse.json({ data: "your response" });
};

export const GET = withX402(
  handler,
  {
    accepts: {
      scheme: "exact",
      price: "$0.01",
      network: "eip155:196",
      payTo: "0xYourAddress",
    },
    description: "Access to API endpoint",
  },
  server,
);
```

## API Reference

### paymentProxy

```typescript
function paymentProxy(
  routes: RoutesConfig,
  server: x402ResourceServer,
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart?: boolean,
): NextMiddleware;
```

Protects page routes (and optionally API routes) via Next.js proxy. Note: using `paymentProxy` for API routes will charge clients even for failed responses.

#### Parameters

| Parameter | Required | Description |
|-----------|----------|-------------|
| `routes` | Yes | Route configurations for protected endpoints |
| `server` | Yes | Pre-configured `x402ResourceServer` instance |
| `paywallConfig` | No | Configuration for the built-in paywall UI shown to browser visitors (`Accept: text/html` + `Mozilla` UA). Ignored for API/SDK clients. |
| `paywall` | No | Custom `PaywallProvider` that overrides the default HTML generator. Only used for browser visitors. |
| `syncFacilitatorOnStart` | No | Whether to sync with facilitator on startup (default: `true`) |

### withX402

```typescript
function withX402(
  handler: (request: NextRequest) => Promise<NextResponse>,
  routeConfig: RouteConfig,
  server: x402ResourceServer,
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart?: boolean,
): (request: NextRequest) => Promise<NextResponse>;
```

Wraps an API route handler with payment protection. Settlement occurs only after a successful response (status < 400).

#### Parameters

| Parameter | Required | Description |
|-----------|----------|-------------|
| `handler` | Yes | Your API route handler function |
| `routeConfig` | Yes | Payment configuration for this route |
| `server` | Yes | Pre-configured `x402ResourceServer` instance |
| `paywallConfig` | No | Configuration for the built-in paywall UI shown to browser visitors (`Accept: text/html` + `Mozilla` UA). Ignored for API/SDK clients. |
| `paywall` | No | Custom `PaywallProvider` that overrides the default HTML generator. Only used for browser visitors. |
| `syncFacilitatorOnStart` | No | Whether to sync with facilitator on startup (default: `true`) |

### Paywall

When an unpaid request comes from a web browser (`Accept` header contains `text/html` **and** `User-Agent` contains `Mozilla`), the proxy/wrapper returns an HTML paywall page instead of a JSON 402. API/SDK clients are unaffected — they continue to receive JSON 402 with the `PAYMENT-REQUIRED` header.

```typescript
import { paymentProxy, PaywallConfig, PaywallProvider } from "@okxweb3/x402-next";

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

export const proxy = paymentProxy(routes, resourceServer, paywallConfig, customPaywall);
```

Per-route override is available via `RouteConfig.customPaywallHtml`, which takes precedence over both `paywall` and the default generator. If you don't need a browser paywall (machine-to-machine APIs only), leave both `paywallConfig` and `paywall` as `undefined`.

### Route Configuration

```typescript
const routes: RoutesConfig = {
  "/api/protected": {
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
```
