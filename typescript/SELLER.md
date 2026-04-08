# x402 Seller SDK — TypeScript Agent Integration Reference

> **Audience:** AI coding agents (Cursor, Claude Code, Copilot, etc.).
> **Scope:** Seller (server) only. No buyer/client code. X Layer network only.
> **Goal:** Generate complete, working x402 payment-protected server code from this document alone.

---

## CONSTRAINTS

- Network: `eip155:196` (X Layer mainnet) — no other networks
- Facilitator client: always use `OKXFacilitatorClient` — never use `HTTPFacilitatorClient`
- Default payment token: USDT0 at `0x779ded0c9e1022225f8e0630b35a9b54be713736` (6 decimals)
- Prices expressed as USD strings (e.g. `"$0.01"`) are auto-converted to USDT0 atomic units
- `resourceServer.initialize()` MUST be called after server starts and before handling requests
- Supported schemes: `exact`, `deferred`

---

## PACKAGES

```
@okxweb3/x402-core       — OKXFacilitatorClient
@okxweb3/x402-evm        — ExactEvmScheme, DeferredEvmScheme (server-side)
@okxweb3/x402-express    — Express middleware
@okxweb3/x402-hono       — Hono middleware
@okxweb3/x402-fastify    — Fastify middleware (NOTE: different call signature)
@okxweb3/x402-next       — Next.js proxy + route handler wrapper
```

Install core + framework:
```bash
npm install @okxweb3/x402-core @okxweb3/x402-evm @okxweb3/x402-{express|hono|fastify|next}
```

---

## IMPORTS

```typescript
// Core — always needed
import { OKXFacilitatorClient } from "@okxweb3/x402-core";

// Schemes — register on resource server
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";
import { DeferredEvmScheme } from "@okxweb3/x402-evm/deferred/server";  // optional

// Framework middleware — pick ONE
// Express:
import { x402ResourceServer, x402HTTPResourceServer, paymentMiddlewareFromHTTPServer, paymentMiddleware, paymentMiddlewareFromConfig } from "@okxweb3/x402-express";
// Hono:
import { x402ResourceServer, x402HTTPResourceServer, paymentMiddlewareFromHTTPServer, paymentMiddleware, paymentMiddlewareFromConfig } from "@okxweb3/x402-hono";
// Fastify:
import { x402ResourceServer, x402HTTPResourceServer, paymentMiddlewareFromHTTPServer, paymentMiddleware, paymentMiddlewareFromConfig } from "@okxweb3/x402-fastify";
// Next.js:
import { x402ResourceServer, x402HTTPResourceServer, paymentProxyFromHTTPServer, paymentProxyFromConfig, withX402FromHTTPServer, withX402 } from "@okxweb3/x402-next";
```

---

## SETUP PATTERN (all frameworks share this)

```typescript
// Step 1: Create facilitator client
const facilitatorClient = new OKXFacilitatorClient({
  apiKey: process.env.OKX_API_KEY!,        // required
  secretKey: process.env.OKX_SECRET_KEY!,  // required — HMAC-SHA256 signing
  passphrase: process.env.OKX_PASSPHRASE!, // required
  baseUrl: "https://web3.okx.com",         // optional, this is the default
  syncSettle: true,                        // optional — see SETTLE MODES below
});

// Step 2: Create resource server + register scheme(s)
const resourceServer = new x402ResourceServer(facilitatorClient);
resourceServer.register("eip155:196", new ExactEvmScheme());
// Optional: resourceServer.register("eip155:196", new DeferredEvmScheme());

// Step 3: Create HTTP server with route config
const httpServer = new x402HTTPResourceServer(resourceServer, {
  "GET /api/resource": {                   // format: "METHOD /path"
    accepts: {                             // single PaymentOption or PaymentOption[]
      scheme: "exact",
      network: "eip155:196",
      payTo: "0xYourWalletAddress",
      price: "$0.01",
      maxTimeoutSeconds: 300,
    },
    description: "Optional description",
  },
});

// Step 4: Register middleware (framework-specific — see below)
// Step 5: Initialize after server starts
await resourceServer.initialize();
```

---

## ROUTE CONFIG REFERENCE

### RoutesConfig

```typescript
type RoutesConfig = Record<string, RouteConfig>;
// Key format: "METHOD /path" e.g. "GET /api/data", "POST /api/generate"
```

### RouteConfig

```typescript
interface RouteConfig {
  accepts: PaymentOption | PaymentOption[];   // REQUIRED
  description?: string;
  resource?: string;
  mimeType?: string;
  customPaywallHtml?: string;                 // custom HTML for browser 402 response
  unpaidResponseBody?: (ctx) => { contentType: string; body: unknown };
  settlementFailedResponseBody?: (ctx, result) => { contentType: string; body: unknown };
}
```

### PaymentOption

```typescript
interface PaymentOption {
  scheme: string;              // "exact" | "deferred"
  network: string;             // "eip155:196"
  payTo: string;               // EVM wallet address
  price: Price;                // "$0.01" | 0.01 | { asset: "0x...", amount: "10000" }
  maxTimeoutSeconds?: number;  // payment signature validity (seconds)
  extra?: Record<string, unknown>;
}
```

### Price formats

| Format | Example | Behavior |
|--------|---------|----------|
| USD string | `"$0.01"` | Converted to USDT0 atomic units (10000) |
| Number | `0.01` | Same as USD string |
| AssetAmount | `{ asset: "0x779d...", amount: "10000" }` | Direct token + atomic units |

---

## FRAMEWORK: EXPRESS

Three middleware functions available. All return `express.RequestHandler`.

### paymentMiddlewareFromHTTPServer (recommended)

```typescript
import express from "express";
import { OKXFacilitatorClient } from "@okxweb3/x402-core";
import { x402ResourceServer, x402HTTPResourceServer, paymentMiddlewareFromHTTPServer } from "@okxweb3/x402-express";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";

const app = express();

const facilitatorClient = new OKXFacilitatorClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
  syncSettle: true,
});

const resourceServer = new x402ResourceServer(facilitatorClient);
resourceServer.register("eip155:196", new ExactEvmScheme());

const httpServer = new x402HTTPResourceServer(resourceServer, {
  "GET /api/premium": {
    accepts: { scheme: "exact", network: "eip155:196", payTo: "0xYourAddress", price: "$0.01", maxTimeoutSeconds: 300 },
  },
});

app.use(paymentMiddlewareFromHTTPServer(httpServer));
// Signature: paymentMiddlewareFromHTTPServer(httpServer, paywallConfig?, paywall?, syncFacilitatorOnStart?)

app.get("/api/premium", (req, res) => {
  res.json({ data: "premium content" });
});

app.listen(4000, async () => {
  await resourceServer.initialize();
});
```

### paymentMiddleware

```typescript
// Passes resource server + route config directly
app.use(paymentMiddleware(routes, resourceServer));
// Signature: paymentMiddleware(routes: RoutesConfig, server: x402ResourceServer, paywallConfig?, paywall?, syncFacilitatorOnStart?)
```

### paymentMiddlewareFromConfig

```typescript
// Minimal — SDK creates resource server internally
app.use(paymentMiddlewareFromConfig(
  routes,                                                    // RoutesConfig
  facilitatorClient,                                         // FacilitatorClient
  [{ network: "eip155:196", scheme: new ExactEvmScheme() }], // SchemeRegistration[]
));
// Signature: paymentMiddlewareFromConfig(routes, facilitatorClients?, schemes?, paywallConfig?, paywall?, syncFacilitatorOnStart?)
```

---

## FRAMEWORK: HONO

Same three functions, same signatures as Express. Returns Hono `MiddlewareHandler`.

```typescript
import { Hono } from "hono";
import { OKXFacilitatorClient } from "@okxweb3/x402-core";
import { x402ResourceServer, x402HTTPResourceServer, paymentMiddlewareFromHTTPServer } from "@okxweb3/x402-hono";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";

const app = new Hono();

const facilitatorClient = new OKXFacilitatorClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
  syncSettle: true,
});

const resourceServer = new x402ResourceServer(facilitatorClient);
resourceServer.register("eip155:196", new ExactEvmScheme());

const httpServer = new x402HTTPResourceServer(resourceServer, {
  "GET /api/premium": {
    accepts: { scheme: "exact", network: "eip155:196", payTo: "0xYourAddress", price: "$0.01", maxTimeoutSeconds: 300 },
  },
});

app.use("*", paymentMiddlewareFromHTTPServer(httpServer));

app.get("/api/premium", (c) => c.json({ data: "premium content" }));

export default { port: 4000, fetch: app.fetch };
await resourceServer.initialize();
```

---

## FRAMEWORK: FASTIFY

> **CRITICAL DIFFERENCE:** Fastify functions take `app` (FastifyInstance) as the FIRST argument.
> They register hooks directly on the instance — they do NOT return middleware.

```typescript
import Fastify from "fastify";
import { OKXFacilitatorClient } from "@okxweb3/x402-core";
import { x402ResourceServer, x402HTTPResourceServer, paymentMiddlewareFromHTTPServer } from "@okxweb3/x402-fastify";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";

const app = Fastify();

const facilitatorClient = new OKXFacilitatorClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
  syncSettle: true,
});

const resourceServer = new x402ResourceServer(facilitatorClient);
resourceServer.register("eip155:196", new ExactEvmScheme());

const httpServer = new x402HTTPResourceServer(resourceServer, {
  "GET /api/premium": {
    accepts: { scheme: "exact", network: "eip155:196", payTo: "0xYourAddress", price: "$0.01", maxTimeoutSeconds: 300 },
  },
});

// NOTE: first arg is app instance — NOT app.use(...)
paymentMiddlewareFromHTTPServer(app, httpServer);
// Signatures:
//   paymentMiddlewareFromHTTPServer(app, httpServer, paywallConfig?, paywall?, syncFacilitatorOnStart?)
//   paymentMiddleware(app, routes, server, paywallConfig?, paywall?, syncFacilitatorOnStart?)
//   paymentMiddlewareFromConfig(app, routes, facilitatorClients?, schemes?, paywallConfig?, paywall?, syncFacilitatorOnStart?)

app.get("/api/premium", async () => ({ data: "premium content" }));

app.listen({ port: 4000 }, async () => {
  await resourceServer.initialize();
});
```

---

## FRAMEWORK: NEXT.JS

Two patterns: **Proxy** (middleware.ts) and **Route Handler wrapper**.

### Proxy pattern — middleware.ts

```typescript
// middleware.ts
import { OKXFacilitatorClient } from "@okxweb3/x402-core";
import { x402ResourceServer, x402HTTPResourceServer, paymentProxyFromHTTPServer } from "@okxweb3/x402-next";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";
import { NextRequest } from "next/server";

const facilitatorClient = new OKXFacilitatorClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
  syncSettle: true,
});

const resourceServer = new x402ResourceServer(facilitatorClient);
resourceServer.register("eip155:196", new ExactEvmScheme());

const httpServer = new x402HTTPResourceServer(resourceServer, {
  "GET /api/premium": {
    accepts: { scheme: "exact", network: "eip155:196", payTo: "0xYourAddress", price: "$0.01", maxTimeoutSeconds: 300 },
  },
});

const paymentHandler = paymentProxyFromHTTPServer(httpServer);
// Also available: paymentProxyFromConfig(routes, facilitatorClients?, schemes?)

export async function middleware(request: NextRequest) {
  return paymentHandler(request);
}

export const config = { matcher: ["/api/premium"] };
```

### Route Handler wrapper — per-route

```typescript
// app/api/premium/route.ts
import { OKXFacilitatorClient } from "@okxweb3/x402-core";
import { x402ResourceServer, x402HTTPResourceServer, withX402FromHTTPServer } from "@okxweb3/x402-next";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";
import { NextResponse } from "next/server";

const facilitatorClient = new OKXFacilitatorClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
  syncSettle: true,
});

const resourceServer = new x402ResourceServer(facilitatorClient);
resourceServer.register("eip155:196", new ExactEvmScheme());

const httpServer = new x402HTTPResourceServer(resourceServer, {
  "GET /api/premium": {
    accepts: { scheme: "exact", network: "eip155:196", payTo: "0xYourAddress", price: "$0.01", maxTimeoutSeconds: 300 },
  },
});

async function handler() {
  return NextResponse.json({ data: "premium content" });
}

export const GET = withX402FromHTTPServer(handler, httpServer);
// Also available: withX402(handler, routeConfig, server)
```

---

## SETTLE MODES

| `syncSettle` | Behavior | Use when |
|---|---|---|
| `true` | Facilitator waits for on-chain confirmation, returns `status="success"` | High-value resources, need payment proof before delivery |
| `false` / omitted | Facilitator returns `status="pending"` immediately | Low-value, high-throughput, acceptable delivery-before-confirm |

---

## HOOKS

### x402HTTPResourceServer hooks

```typescript
// Grant access without payment / deny / continue to payment
httpServer.onProtectedRequest(async (context, routeConfig) => {
  if (isWhitelisted(context)) return { grantAccess: true };
  if (isBlocked(context)) return { abort: true, reason: "Blocked" };
  // return void → continue to payment flow
});

// On-chain fallback when facilitator returns status="timeout"
httpServer.onSettlementTimeout(async (txHash, network) => {
  const confirmed = await verifyOnChain(txHash);
  return { confirmed };
});

// Adjust poll deadline (default 5000ms)
httpServer.setPollDeadline(10000);
```

### x402ResourceServer lifecycle hooks

```typescript
resourceServer.onBeforeVerify(async (ctx) => {
  // return { abort: true, reason: "..." } to reject
});
resourceServer.onAfterVerify(async (ctx) => { /* ctx.result */ });
resourceServer.onVerifyFailure(async (ctx) => {
  // return { recovered: true, result: VerifyResponse } to recover
});
resourceServer.onBeforeSettle(async (ctx) => {
  // return { abort: true, reason: "..." } to reject
});
resourceServer.onAfterSettle(async (ctx) => { /* ctx.result */ });
resourceServer.onSettleFailure(async (ctx) => {
  // return { recovered: true, result: SettleResponse } to recover
});
```

---

## MULTIPLE SCHEMES (exact + deferred)

```typescript
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";
import { DeferredEvmScheme } from "@okxweb3/x402-evm/deferred/server";

const resourceServer = new x402ResourceServer(facilitatorClient);
resourceServer.register("eip155:196", new ExactEvmScheme());
resourceServer.register("eip155:196", new DeferredEvmScheme());

const httpServer = new x402HTTPResourceServer(resourceServer, {
  "GET /api/resource": {
    accepts: [
      { scheme: "deferred", network: "eip155:196", payTo: addr, price: "$0.01", maxTimeoutSeconds: 300 },
      { scheme: "exact", network: "eip155:196", payTo: addr, price: "$0.01", maxTimeoutSeconds: 300 },
    ],
  },
});
```

---

## SETTLEMENT TIMEOUT RECOVERY (on-chain verification)

```typescript
import { createPublicClient, http, defineChain } from "viem";

const xlayer = defineChain({
  id: 196,
  name: "X Layer",
  nativeCurrency: { name: "OKB", symbol: "OKB", decimals: 18 },
  rpcUrls: { default: { http: ["https://rpc.xlayer.tech"] } },
});

const viemClient = createPublicClient({ chain: xlayer, transport: http() });

httpServer.onSettlementTimeout(async (txHash, _network) => {
  try {
    const receipt = await viemClient.getTransactionReceipt({ hash: txHash as `0x${string}` });
    return { confirmed: receipt?.status === "success" };
  } catch {
    return { confirmed: false };
  }
});
```

---

## PAYWALL CONFIG

```typescript
// Pass as second arg to paymentMiddlewareFromHTTPServer (Express/Hono)
// or paymentProxyFromHTTPServer (Next.js)
const paywallConfig = {
  appName?: string;
  appLogo?: string;           // URL to logo image
  sessionTokenEndpoint?: string;
  currentUrl?: string;
  testnet?: boolean;
};
```

---

## ENV VARS

```bash
OKX_API_KEY=your-api-key
OKX_SECRET_KEY=your-secret-key
OKX_PASSPHRASE=your-passphrase
```

---

## DECISION TREE

```
Need to protect an HTTP endpoint with payment?
│
├─ Which framework?
│  ├─ Express  → @okxweb3/x402-express
│  ├─ Hono     → @okxweb3/x402-hono
│  ├─ Fastify  → @okxweb3/x402-fastify  ⚠️ first arg is app instance
│  └─ Next.js  → @okxweb3/x402-next
│
├─ Need lifecycle hooks or onSettlementTimeout?
│  ├─ Yes → use paymentMiddlewareFromHTTPServer / paymentProxyFromHTTPServer
│  └─ No  → use paymentMiddlewareFromConfig / paymentProxyFromConfig (simpler)
│
├─ Payment confirmation before delivery?
│  ├─ Yes → syncSettle: true
│  └─ No  → omit syncSettle
│
└─ Which scheme(s)?
   ├─ Standard payment     → "exact" + ExactEvmScheme
   ├─ High-frequency small → "deferred" + DeferredEvmScheme
   └─ Both                 → register both, accepts: [deferred, exact]
```

---

## COMMON MISTAKES

| Mistake | Fix |
|---------|-----|
| Forgot `await resourceServer.initialize()` | Call it after server starts, before handling requests |
| Used `HTTPFacilitatorClient` | Always use `OKXFacilitatorClient` |
| Fastify: `app.use(paymentMiddlewareFromHTTPServer(httpServer))` | Fastify: `paymentMiddlewareFromHTTPServer(app, httpServer)` — app is first arg |
| Hardcoded token amount without understanding decimals | Use USD string `"$0.01"` — SDK converts automatically |
| Used network other than `eip155:196` | Only X Layer is supported |
