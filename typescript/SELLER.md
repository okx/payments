# Seller SDK — TypeScript Agent Integration Reference

> **Audience:** AI coding agents (Cursor, Claude Code, Copilot, etc.).
> **Scope:** Seller (server) only. No buyer/client code. X Layer (`eip155:196`) only.
> **Goal:** Generate complete, working payment-protected server code from this document alone.
>
> Pick a section by the **billing capability you need**. The capability map sits right below — find your row, jump to the section, copy the snippet.

---

## CAPABILITY MAP — start here

| You need to … | Go to | Package(s) |
|---|---|---|
| Charge a **fixed price per call** on USDT0 (gasless EIP-3009) | [Fixed price per call](#fixed-price-per-call) | `@okxweb3/x402-{core,evm,<framework>}` |
| Charge a fixed price on **any ERC-20** (token has no EIP-3009 support) | [Any ERC-20 via Permit2](#any-erc-20-via-permit2) | same as above |
| Bill **by actual usage** (tokens / bytes / requests), buyer signs an upper cap | [Pay-by-usage (metered cap)](#pay-by-usage-metered-cap) | same as above |
| Split one payment across **multiple recipients** (revenue share) | [Multi-recipient splits](#multi-recipient-splits) | `@okxweb3/mpp` |
| Open a deposit **channel** — many off-chain vouchers, one settle at close | [Pay-as-you-go channel](#pay-as-you-go-channel) | `@okxweb3/mpp` |
| **Batch** many high-frequency low-value calls into fewer on-chain txs | [High-frequency batched](#high-frequency-batched) | `@okxweb3/x402-{core,evm,<framework>}` |
| Let buyers on **different protocols** hit the same URL | [One URL, multiple protocols](#one-url-multiple-protocols) | `@okxweb3/payment-router` |
| Express / Hono / Fastify / Next.js integration | [Framework integration](#framework-integration) | framework-specific adapter |
| Lifecycle hooks, on-chain timeout recovery, paywall HTML | [Operational extras](#operational-extras) | — |

---

## CONSTRAINTS (apply to every section)

- Network: `eip155:196` (X Layer mainnet) — no other networks
- Default payment token: USDT0 `0x779ded0c9e1022225f8e0630b35a9b54be713736` (6 decimals)
- All on-chain settlement is brokered by OKX SA API (HMAC-SHA256 signed) — env vars below
- Two SDK families:
  - **`@okxweb3/x402-*`** — 402-protected HTTP routes, framework-adapted (Express/Hono/Fastify/Next.js)
  - **`@okxweb3/mpp`** — Web Standards `Request`/`Response`, framework-agnostic (bridge with `node:http`, Express, anything)
- Price encoding **differs** between the two families:
  - `@okxweb3/x402-*` accepts USD strings (`"$0.01"`), numbers, or `{ asset, amount }`
  - `@okxweb3/mpp` requires **base units** (`"10000"` = 0.01 of a 6-decimal token); no `"$..."` syntax
- For the `@okxweb3/x402-*` family: `await resourceServer.initialize()` MUST run after the server starts, before any request is handled

## ENV VARS (apply to every section)

```bash
# OKX SA API — used everywhere
OKX_API_KEY=your-api-key
OKX_SECRET_KEY=your-secret-key
OKX_PASSPHRASE=your-passphrase

# Channel capability only
MPP_SECRET_KEY=hex-string-for-challenge-signing
MPP_MERCHANT_PRIVATE_KEY=0x...                 # signer for vouchers
MPP_ESCROW=0x...                               # deployed escrow address

# Seller wallet (payee)
PAY_TO=0x...
```

---

## TYPE SIGNATURES (verbatim from the SDK)

Copy these names exactly. Do NOT invent field names; if a field isn't here, it doesn't exist.

### `@okxweb3/x402-core`

```typescript
// new OKXFacilitatorClient(config)
interface OKXConfig {
  apiKey: string;                                    // required
  secretKey: string;                                 // required — HMAC-SHA256 signing
  passphrase: string;                                // required
  baseUrl?: string;                                  // default: "https://web3.okx.com"
  syncSettle?: boolean;                              // default false — see Settle modes
}

// new x402ResourceServer(facilitatorClients?)
class x402ResourceServer {
  constructor(facilitatorClients?: FacilitatorClient | FacilitatorClient[]);
  register(network: Network, server: SchemeNetworkServer): this;     // chainable
  initialize(): Promise<void>;                                       // MUST run after server starts
  registerExtension(extension: ResourceServerExtension): this;
  onBeforeVerify(hook): this;
  onAfterVerify(hook): this;
  onVerifyFailure(hook): this;                       // hook may return { recovered, result }
  onBeforeSettle(hook): this;
  onAfterSettle(hook): this;
  onSettleFailure(hook): this;
}

// new x402HTTPResourceServer(resourceServer, routes)
class x402HTTPResourceServer {
  constructor(resourceServer: x402ResourceServer, routes: RoutesConfig);
  onProtectedRequest(hook): void;                    // grantAccess / abort / continue
  onSettlementTimeout(hook): void;                   // { confirmed: boolean }
  setPollDeadline(ms: number): void;                 // default 5000
}

type RoutesConfig = Record<string, RouteConfig>;     // key: "METHOD /path"

interface RouteConfig {
  accepts: PaymentOption | PaymentOption[];          // REQUIRED
  description?: string;
  resource?: string;
  mimeType?: string;
  customPaywallHtml?: string;
  unpaidResponseBody?: (ctx) => { contentType: string; body: unknown };
  settlementFailedResponseBody?: (ctx, result) => { contentType: string; body: unknown };
}

interface PaymentOption {
  scheme: "exact" | "upto" | "deferred";
  network: "eip155:196";                             // X Layer only
  payTo: string | DynamicPayTo;                      // EVM address or fn(ctx) => address
  price: Price | DynamicPrice;                       // "$0.01" | 0.01 | { asset, amount } | fn
  maxTimeoutSeconds?: number;
  extra?: Record<string, unknown>;                   // scheme-specific (see below)
}

// extra fields by scheme:
//   exact   (EIP-3009 default)  → none
//   exact   (Permit2 sub-mode)  → { assetTransferMethod: "permit2" }
//   upto                        → { decimals?: number }  (auto-injects assetTransferMethod)
//   deferred                    → none
```

### Settlement override (`@okxweb3/x402-{express,...}` + `@okxweb3/x402-core/server`)

```typescript
// Express helper
function setSettlementOverrides(res: express.Response, overrides: SettlementOverrides): void;

// Or set the header directly (any framework)
const SETTLEMENT_OVERRIDES_HEADER = "settlement-overrides";    // JSON.stringify(overrides)

interface SettlementOverrides {
  amount?: string;
  // Formats:
  //   "1234000"  raw atomic units
  //   "50%"      percent of cap (2-decimal precision)
  //   "$0.034"   dollar string (uses extra.decimals; default 6)
  //   "0"        short-circuit — no on-chain tx
  // Resolved amount MUST be ≤ the cap declared in accepts.price.
}
```

### Framework middleware factory signatures

```typescript
// @okxweb3/x402-express  (and identical in -hono)
paymentMiddlewareFromHTTPServer(
  httpServer,
  paywallConfig?,
  paywall?,
  syncFacilitatorOnStart?,
): RequestHandler;
paymentMiddleware(routes, server, paywallConfig?, paywall?, syncFacilitatorOnStart?): RequestHandler;
paymentMiddlewareFromConfig(
  routes,
  facilitatorClients?,
  schemes?: { network: Network; server: SchemeNetworkServer }[],
  paywallConfig?, paywall?, syncFacilitatorOnStart?,
): RequestHandler;

// @okxweb3/x402-fastify  — DIFFERENT: app is FIRST arg, registers hooks directly
paymentMiddlewareFromHTTPServer(app, httpServer, paywallConfig?, paywall?, syncFacilitatorOnStart?): void;
paymentMiddleware(app, routes, server, paywallConfig?, paywall?, syncFacilitatorOnStart?): void;
paymentMiddlewareFromConfig(app, routes, facilitatorClients?, schemes?, paywallConfig?, paywall?, syncFacilitatorOnStart?): void;

// @okxweb3/x402-next
paymentProxyFromHTTPServer(httpServer, paywallConfig?): (req: NextRequest) => Promise<Response>;
paymentProxyFromConfig(routes, facilitatorClients?, schemes?, paywallConfig?): (req: NextRequest) => Promise<Response>;
withX402FromHTTPServer(handler, httpServer): NextRouteHandler;
withX402(handler, routeConfig, server): NextRouteHandler;

interface PaywallConfig {
  appName?: string;
  appLogo?: string;
  sessionTokenEndpoint?: string;
  currentUrl?: string;
  testnet?: boolean;
}
```

### `@okxweb3/mpp` + `@okxweb3/mpp/evm`

```typescript
// new SaApiClient(config)
interface SaApiClientConfig {
  apiKey: string;
  secretKey: string;
  passphrase: string;
  baseUrl?: string;                                  // default "https://web3.okx.com"
  onError?: (info: SaApiErrorInfo) => void;          // never affects main flow
}

// Mppx.create(config) — from "mppx/server" (re-exported by @okxweb3/mpp)
namespace Mppx {
  function create<methods>(config: {
    methods: methods;                                // [charge({...})], [session({...})], or both
    realm?: string;                                  // default: auto-detect (MPP_REALM, VERCEL_URL, …)
    secretKey?: string;                              // default: process.env.MPP_SECRET_KEY (REQUIRED)
    transport?: Transport.AnyTransport;              // default: Transport.http()
  }): Mppx<methods>;
}

// charge({ saClient }) — from "@okxweb3/mpp/evm/server"
namespace charge {
  type Parameters = { saClient: SaApiClient };
}

// charge() options passed to mppx.charge(options)(request)
type ChargeOptions = {
  amount: string;                                    // base units, e.g. "10000"  NOT "$0.01"
  currency: string;                                  // ERC-20 token address (40-hex)
  recipient: string;                                 // primary payee (40-hex EIP-55)
  description?: string;
  externalId?: string;                               // idempotency key
  methodDetails: {
    chainId: number;                                 // default 196 (X Layer)
    feePayer?: boolean;                              // true → seller broadcasts (transaction mode)
    splits?: Array<{
      amount: string;                                // base units; sum(splits) < amount
      recipient: string;
      memo?: string;
    }>;                                              // splits.length ≤ 10
  };
};

// session({ saClient, signer, ... }) — from "@okxweb3/mpp/evm/server"
namespace session {
  type Parameters = {
    saClient: SaApiClient;
    signer: SessionSigner;                           // viem LocalAccount or WalletClient.account
                                                     // signer.address MUST equal opts.recipient
    chainId?: number;                                // default 196
    escrowContract?: Hex;                            // can also live in route options
    domainName?: string;                             // default "EVM Payment Channel"
    domainVersion?: string;                          // default "1"
    store?: SessionStore;                            // default: in-memory
    minVoucherDelta?: string;                        // default "0"
  };
}

// session() options passed to mppx.session(options)(request)
type SessionOptions = {
  amount: string;                                    // unit price (base units)
  currency: string;
  recipient: string;                                 // MUST equal signer.address
  description?: string;
  unitType: string;                                  // "request" | "token" | "byte" | ...
  suggestedDeposit: string;                          // base units
  methodDetails: {
    chainId: number;                                 // 196
    escrowContract: string;                          // REQUIRED (40-hex) for open/topUp
    feePayer?: boolean;
    minVoucherDelta?: string;                        // base units (default "0")
  };
};

// Return shape — both charge() and session() invocations
type MethodResponse =
  | { status: 402; challenge: Response }              // no/invalid credential → return as-is
  | { status: 200; withReceipt: (res: Response) => Response };  // wrap business response
```

### `@okxweb3/payment-router`

```typescript
function paymentRouter(config: PaymentRouterConfig): (handler: Handler) => Handler;

interface PaymentRouterConfig {
  adapters: ProtocolAdapter[];
  routes: UnifiedRoutesConfig;
  onError?: (error: unknown, protocol: string) => void;
}

type Handler = (request: Request) => Promise<Response> | Response;
type UnifiedRoutesConfig = Record<string, UnifiedRouteConfig>;  // key: "METHOD /path"

interface UnifiedRouteConfig {
  description?: string;
  adapterConfigs: Record<string, unknown>;            // key = adapter.name → per-adapter route opts
}

interface ProtocolAdapter<Cfg = unknown> {
  readonly name: string;                              // unique key in adapterConfigs
  readonly priority: number;                          // detect order: MPP=10, x402=20, custom≥100
  initialize?(): Promise<void>;
  prepare?(routes: ReadonlyMap<string, Cfg>): void | Promise<void>;
  detect(request: Request): boolean;
  buildChallenge(request: Request, config: Cfg): Promise<Record<string, string>>;
  handle(request: Request, config: Cfg, inner: Handler): Promise<Response>;
}

// Built-in adapters
new MppAdapter({
  mppx: MppxInstance;                                 // from Mppx.create({...})
  priority?: number;                                  // default 10
  defaultIntent?: string;                             // default "charge"
});

new X402Adapter({
  resourceServer: x402ResourceServer;                 // facilitator + schemes registered (NO routes)
  httpResourceServerCtor: typeof x402HTTPResourceServer;  // injected to avoid hard import
  priority?: number;                                  // default 20
  paywallConfig?: PaywallConfig;
});

// Per-route adapter configs
interface MppAdapterRouteConfig {
  intent?: string;                                    // default "charge"; "session" also valid
  [key: string]: unknown;                             // forwarded to mppx.<intent>(opts) as-is
}
type X402AdapterRouteConfig =
  | (PaymentOption & { description?: string; mimeType?: string; paywallConfig?: PaywallConfig })
  | { accepts: PaymentOption | PaymentOption[]; description?: string; mimeType?: string; paywallConfig?: PaywallConfig };
```

---

## Fixed price per call

USDT0, gasless `transferWithAuthorization` (EIP-3009). Simplest case — buyer signs a single authorization per call.

```typescript
import express from "express";
import { OKXFacilitatorClient } from "@okxweb3/x402-core";
import {
  x402ResourceServer,
  x402HTTPResourceServer,
  paymentMiddlewareFromHTTPServer,
} from "@okxweb3/x402-express";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";

const app = express();

const facilitatorClient = new OKXFacilitatorClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
  syncSettle: true,                       // wait for on-chain confirm before delivery
});

const resourceServer = new x402ResourceServer(facilitatorClient)
  .register("eip155:196", new ExactEvmScheme());

const httpServer = new x402HTTPResourceServer(resourceServer, {
  "GET /api/premium": {
    accepts: {
      scheme: "exact",
      network: "eip155:196",
      payTo: process.env.PAY_TO!,
      price: "$0.01",                     // USD string → USDT0 atomic units
      maxTimeoutSeconds: 300,
    },
  },
});

app.use(paymentMiddlewareFromHTTPServer(httpServer));
app.get("/api/premium", (_req, res) => res.json({ data: "premium content" }));

app.listen(4000, async () => {
  await resourceServer.initialize();
});
```

`price` accepts: `"$0.01"` · `0.01` · `{ asset: "0x...", amount: "10000" }`.

---

## Any ERC-20 via Permit2

Same shape as fixed-price, but the buyer signs a Permit2 `PermitWitnessTransferFrom` instead of EIP-3009. Use this when the target token doesn't support EIP-3009. One-time on the buyer side: `IERC20.approve(PERMIT2, MAX_UINT256)`.

```typescript
import express from "express";
import { OKXFacilitatorClient } from "@okxweb3/x402-core";
import {
  x402ResourceServer,
  x402HTTPResourceServer,
  paymentMiddlewareFromHTTPServer,
} from "@okxweb3/x402-express";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";

const app = express();

const facilitatorClient = new OKXFacilitatorClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
  syncSettle: true,
});

const resourceServer = new x402ResourceServer(facilitatorClient)
  .register("eip155:196", new ExactEvmScheme());

const httpServer = new x402HTTPResourceServer(resourceServer, {
  "GET /api/premium": {
    accepts: {
      scheme: "exact",
      network: "eip155:196",
      payTo: process.env.PAY_TO!,
      price: "$0.05",
      extra: { assetTransferMethod: "permit2" },   // the one diff vs default exact
    },
  },
});

app.use(paymentMiddlewareFromHTTPServer(httpServer));
app.get("/api/premium", (_req, res) => res.json({ data: "premium content" }));

app.listen(4000, async () => {
  await resourceServer.initialize();
});
```

---

## Pay-by-usage (metered cap)

Buyer signs an **upper bound** (Permit2 cap); the handler decides the actual charge per request (`0 ≤ actual ≤ cap`) via `setSettlementOverrides`. Good for token-billed AI, bandwidth, per-row queries.

```typescript
import express from "express";
import { OKXFacilitatorClient } from "@okxweb3/x402-core";
import {
  x402ResourceServer,
  paymentMiddleware,
  setSettlementOverrides,
} from "@okxweb3/x402-express";
import { UptoEvmScheme } from "@okxweb3/x402-evm/upto/server";

const facilitatorClient = new OKXFacilitatorClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
});
const resourceServer = new x402ResourceServer(facilitatorClient)
  .register("eip155:196", new UptoEvmScheme());

const routes = {
  "GET /api/usage": {
    accepts: {
      scheme: "upto",
      network: "eip155:196",
      payTo: process.env.PAY_TO!,
      price: "$0.10",                       // CAP — the upper bound buyer signs
      maxTimeoutSeconds: 300,
      // UptoEvmScheme auto-injects assetTransferMethod=permit2; do NOT set it yourself
    },
    description: "Pay by usage",
    mimeType: "application/json",
  },
};

const app = express();
app.use(paymentMiddleware(routes, resourceServer));

app.get("/api/usage", (_req, res) => {
  // Compute the real cost for this request, must be ≤ cap.
  setSettlementOverrides(res, { amount: "$0.034" });
  res.json({ tokens_used: 1342, billed: "$0.034" });
});

app.listen(4000, async () => { await resourceServer.initialize(); });
```

`setSettlementOverrides({ amount })` accepts:

| Format | Meaning |
|---|---|
| `"1234000"` | Raw atomic units |
| `"50%"` | Percent of cap (supports 2 decimals: `"33.33%"`) |
| `"$0.034"` | Dollar string (uses `extra.decimals`, default 6) |
| `"0"` | Short-circuit — no on-chain tx |

Resolved amount must be ≤ cap, else facilitator rejects. **Non-Express frameworks**: set the response header `settlement-overrides` (JSON-encoded) directly — middleware reads it from `responseHeaders` and strips it before sending to the buyer.

---

## Multi-recipient splits

Total `amount` is split: primary recipient receives `amount − sum(splits)`, each entry in `splits` receives its share. Buyer signs one EIP-3009 per recipient (primary + each split). Use for revenue share, partner payouts, affiliate fees.

```typescript
import * as http from "node:http";
import { Mppx } from "@okxweb3/mpp";
import { charge } from "@okxweb3/mpp/evm/server";
import { SaApiClient } from "@okxweb3/mpp/evm";

const saClient = new SaApiClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
});

const mppx = Mppx.create({
  methods: [charge({ saClient })],
  realm: "demo.merchant.com",
  secretKey: process.env.MPP_SECRET_KEY!,
});

// Constraints: sum(splits) < amount;  splits.length ≤ 10;  recipient is 40-hex EIP-55.
const splits = [
  { amount: "30", recipient: "0x...", memo: "partner-a" },
  { amount: "20", recipient: "0x...", memo: "partner-b" },
];

const CHARGE = {
  amount: "100",                            // base units — NOT a USD string
  currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
  recipient: process.env.PAY_TO!,           // primary recipient (gets 50)
  description: "Premium API (split)",
  methodDetails: { chainId: 196, feePayer: true, splits },
} as const;

async function premium(request: Request): Promise<Response> {
  const result = await mppx.charge(CHARGE)(request);
  if (result.status === 402) return result.challenge;
  return result.withReceipt(Response.json({ data: "premium content" }));
}

// node:http ↔ Web Standards bridge (no framework needed)
http.createServer(async (req, res) => {
  const url = `http://${req.headers.host ?? "localhost:4000"}${req.url}`;
  const webReq = new Request(url, {
    method: req.method,
    headers: new Headers(req.headers as Record<string, string>),
  });
  const webRes = new URL(url).pathname === "/api/premium"
    ? await premium(webReq)
    : new Response("not found", { status: 404 });
  res.statusCode = webRes.status;
  webRes.headers.forEach((v, k) => res.setHeader(k, v));
  res.end(await webRes.text());
}).listen(4000);
```

Drop `splits` to get a plain single-recipient charge — same call, same imports. `payload.type` dispatches on the credential: `"transaction"` → SA API `/charge/settle` (seller broadcasts if `feePayer: true`); `"hash"` → `/charge/verifyHash` (buyer-broadcast tx).

---

## Pay-as-you-go channel

Open a deposit channel; buyer sends one **off-chain voucher** per call (signature-only, no on-chain tx); seller settles the highest voucher on close. Best for high-frequency low-latency APIs (pay-per-token chat, per-request inference).

```typescript
import * as http from "node:http";
import { Mppx } from "@okxweb3/mpp";
import { session } from "@okxweb3/mpp/evm/server";
import { SaApiClient } from "@okxweb3/mpp/evm";
import { privateKeyToAccount } from "viem/accounts";

const saClient = new SaApiClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
});

// signer.address MUST equal the expected payee (`recipient` below).
// The session method fast-fails at startup if they don't match.
const sellerSigner = privateKeyToAccount(
  process.env.MPP_MERCHANT_PRIVATE_KEY! as `0x${string}`,
);

const mppx = Mppx.create({
  methods: [session({ saClient, signer: sellerSigner })],
  // Optional: `store: ...` for SQLite / Redis / Postgres. Defaults to in-memory.
  realm: "demo.merchant.com",
  secretKey: process.env.MPP_SECRET_KEY!,
});

const SESSION = {
  amount: "100",                           // unit price (base units)
  currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
  recipient: process.env.PAY_TO!,          // must match sellerSigner.address
  description: "Pay-per-use API",
  unitType: "request",                     // request | token | byte | ...
  suggestedDeposit: "10000",               // 100× unit price
  methodDetails: {
    chainId: 196,
    escrowContract: process.env.MPP_ESCROW!,  // REQUIRED — 40-hex
    feePayer: true,
    minVoucherDelta: "0",                  // anti-griefing minimum increment
  },
} as const;

// One endpoint serves all 4 actions (`payload.action`):
//   open      → on-chain deposit, returns channelId
//   voucher   → off-chain signature; runs inner business handler
//   topUp     → on-chain deposit into existing channel
//   close     → settle highest voucher on-chain
async function manage(request: Request): Promise<Response> {
  const result = await mppx.session(SESSION)(request);
  if (result.status === 402) return result.challenge;
  return result.withReceipt(Response.json({ status: "ok" }));
}

// node:http ↔ Web Standards bridge
http.createServer(async (req, res) => {
  const url = `http://${req.headers.host ?? "localhost:4000"}${req.url}`;
  const webReq = new Request(url, {
    method: req.method,
    headers: new Headers(req.headers as Record<string, string>),
  });
  const webRes = new URL(url).pathname === "/session/manage"
    ? await manage(webReq)
    : new Response("not found", { status: 404 });
  res.statusCode = webRes.status;
  webRes.headers.forEach((v, k) => res.setHeader(k, v));
  res.end(await webRes.text());
}).listen(4000);
```

---

## High-frequency batched

Many small calls amortized into fewer on-chain settles by the facilitator. Buyer uses a session-key delegation; settlement is asynchronous.

```typescript
import express from "express";
import { OKXFacilitatorClient } from "@okxweb3/x402-core";
import {
  x402ResourceServer,
  x402HTTPResourceServer,
  paymentMiddlewareFromHTTPServer,
} from "@okxweb3/x402-express";
import { DeferredEvmScheme } from "@okxweb3/x402-evm/deferred/server";

const app = express();

const facilitatorClient = new OKXFacilitatorClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
});

const resourceServer = new x402ResourceServer(facilitatorClient)
  .register("eip155:196", new DeferredEvmScheme());

const httpServer = new x402HTTPResourceServer(resourceServer, {
  "GET /api/feed": {
    accepts: {
      scheme: "deferred",
      network: "eip155:196",
      payTo: process.env.PAY_TO!,
      price: "$0.001",
      maxTimeoutSeconds: 300,
    },
  },
});

app.use(paymentMiddlewareFromHTTPServer(httpServer));
app.get("/api/feed", (_req, res) => res.json({ data: "feed item" }));

app.listen(4000, async () => {
  await resourceServer.initialize();
});
```

You can also list **multiple schemes** in one route's `accepts` so the buyer picks. Register every scheme you list:

```typescript
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";
import { DeferredEvmScheme } from "@okxweb3/x402-evm/deferred/server";
import { UptoEvmScheme } from "@okxweb3/x402-evm/upto/server";

const resourceServer = new x402ResourceServer(facilitatorClient)
  .register("eip155:196", new ExactEvmScheme())
  .register("eip155:196", new DeferredEvmScheme())
  .register("eip155:196", new UptoEvmScheme());

const httpServer = new x402HTTPResourceServer(resourceServer, {
  "GET /api/feed": {
    accepts: [
      { scheme: "deferred", network: "eip155:196", payTo: process.env.PAY_TO!, price: "$0.001" },
      { scheme: "exact",    network: "eip155:196", payTo: process.env.PAY_TO!, price: "$0.001" },
    ],
  },
});
```

---

## One URL, multiple protocols

Expose the same path to buyers that speak different 402 dialects. Detection is automatic via request headers; if no protocol matches, all challenges are merged into a single 402 response.

```typescript
import * as http from "node:http";

import { Mppx } from "@okxweb3/mpp";
import { charge as mppCharge } from "@okxweb3/mpp/evm/server";
import { SaApiClient } from "@okxweb3/mpp/evm";

import { OKXFacilitatorClient } from "@okxweb3/x402-core";
import { x402HTTPResourceServer, x402ResourceServer } from "@okxweb3/x402-core/server";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";

import { MppAdapter, X402Adapter, paymentRouter } from "@okxweb3/payment-router";

const saClient = new SaApiClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
});
const mppx = Mppx.create({
  methods: [mppCharge({ saClient })],
  realm: "demo.merchant.com",
  secretKey: process.env.MPP_SECRET_KEY!,
});

const x402Server = new x402ResourceServer(
  new OKXFacilitatorClient({
    apiKey: process.env.OKX_API_KEY!,
    secretKey: process.env.OKX_SECRET_KEY!,
    passphrase: process.env.OKX_PASSPHRASE!,
  }),
).register("eip155:196", new ExactEvmScheme());
// Don't pass routes to x402ResourceServer — declare them on the router below.

const protect = paymentRouter({
  adapters: [
    new MppAdapter({ mppx }),                          // priority 10 (runs first)
    new X402Adapter({                                  // priority 20
      resourceServer: x402Server,
      httpResourceServerCtor: x402HTTPResourceServer,
    }),
  ],
  routes: {
    "GET /generateImg": {
      description: "AI Image Generation",
      adapterConfigs: {
        mpp: {
          intent: "charge",                            // defaults to "charge"
          amount: "10",
          currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
          recipient: process.env.PAY_TO!,
          methodDetails: { chainId: 196, feePayer: true },
        },
        x402: {
          scheme: "exact",
          network: "eip155:196",
          payTo: process.env.PAY_TO!,
          price: "$0.00001",
          mimeType: "application/json",
        },
      },
    },
  },
});

const handler = protect(async () =>
  Response.json({ imageUrl: "https://placehold.co/512x512/png" })
);

http.createServer(async (req, res) => {
  const url = `http://${req.headers.host ?? "localhost:4000"}${req.url}`;
  const webReq = new Request(url, {
    method: req.method,
    headers: new Headers(req.headers as Record<string, string>),
  });
  const webRes = new URL(url).pathname === "/generateImg" && req.method === "GET"
    ? await handler(webReq)
    : new Response("not found", { status: 404 });
  res.statusCode = webRes.status;
  webRes.headers.forEach((v, k) => res.setHeader(k, v));
  res.end(await webRes.text());
}).listen(4000);
```

**Detection rules:**

- `Authorization: Payment ...` header → MPP adapter (priority 10 — wins)
- `payment-signature` or `x-payment` header → x402 adapter (priority 20)
- Neither → both adapters' `buildChallenge` run concurrently; headers merge into one 402
- Both → priority wins

**Adding a custom protocol:**

```typescript
import type { ProtocolAdapter, Handler } from "@okxweb3/payment-router";

class MyAdapter implements ProtocolAdapter {
  readonly name = "my-proto";
  readonly priority = 100;                            // custom adapters: ≥ 100
  detect(req: Request) { return req.headers.has("x-my-proto"); }
  async buildChallenge() { return { "WWW-Authenticate": `my-proto realm="..."` }; }
  async handle(req: Request, _cfg: unknown, inner: Handler) {
    // verify → call inner(req) → settle → attach receipt
    return inner(req);
  }
}
```

Add `my-proto: { ... }` to each route's `adapterConfigs`, plus the adapter to `adapters: [...]`. Router core is unchanged.

---

## Framework integration

The fixed-price / Permit2 / metered-cap / batched sections all use the same wiring; switch only the imported middleware.

### Express

```typescript
import { paymentMiddlewareFromHTTPServer, paymentMiddleware, paymentMiddlewareFromConfig } from "@okxweb3/x402-express";

app.use(paymentMiddlewareFromHTTPServer(httpServer));
// or:
app.use(paymentMiddleware(routes, resourceServer));
// or (SDK creates resource server for you):
app.use(paymentMiddlewareFromConfig(routes, facilitatorClient, [
  { network: "eip155:196", server: new ExactEvmScheme() },
]));
```

### Hono

```typescript
import { Hono } from "hono";
import { paymentMiddlewareFromHTTPServer } from "@okxweb3/x402-hono";

const app = new Hono();
app.use("*", paymentMiddlewareFromHTTPServer(httpServer));
export default { port: 4000, fetch: app.fetch };
await resourceServer.initialize();
```

### Fastify

> **DIFFERENT SIGNATURE:** `app` is the FIRST argument. The function registers hooks directly — do NOT wrap with `app.use(...)`.

```typescript
import Fastify from "fastify";
import { paymentMiddlewareFromHTTPServer } from "@okxweb3/x402-fastify";

const app = Fastify();
paymentMiddlewareFromHTTPServer(app, httpServer);
//   paymentMiddleware(app, routes, server, paywallConfig?, paywall?, syncFacilitatorOnStart?)
//   paymentMiddlewareFromConfig(app, routes, facilitatorClients?, schemes?, ...)
app.listen({ port: 4000 }, async () => { await resourceServer.initialize(); });
```

### Next.js

```typescript
// middleware.ts — proxy pattern (matches one or more routes)
import { paymentProxyFromHTTPServer } from "@okxweb3/x402-next";
import type { NextRequest } from "next/server";

const paymentHandler = paymentProxyFromHTTPServer(httpServer);
export async function middleware(request: NextRequest) { return paymentHandler(request); }
export const config = { matcher: ["/api/premium"] };
```

```typescript
// app/api/premium/route.ts — per-route wrapper
import { withX402FromHTTPServer } from "@okxweb3/x402-next";
import { NextResponse } from "next/server";

async function handler() { return NextResponse.json({ data: "premium content" }); }
export const GET = withX402FromHTTPServer(handler, httpServer);
// Also: withX402(handler, routeConfig, server)
```

### Framework-free (`@okxweb3/mpp` / `@okxweb3/payment-router`)

Both speak Web Standards `Request` / `Response`. Bridge from any HTTP server in ~10 lines — see the `node:http` snippet in [Multi-recipient splits](#multi-recipient-splits).

---

## Operational extras

### Settle modes (`syncSettle`)

| Value | Behavior | Use when |
|---|---|---|
| `true` | Facilitator waits for on-chain confirmation, returns `status="success"` | High-value resources, need payment proof before delivery |
| `false` / omitted | Facilitator returns `status="pending"` immediately | Low-value, high-throughput |

### Hooks (`@okxweb3/x402-*`)

```typescript
// HTTP-level — grant / deny / continue, on-chain fallback, poll deadline
httpServer.onProtectedRequest(async (ctx, routeConfig) => {
  if (isWhitelisted(ctx)) return { grantAccess: true };
  if (isBlocked(ctx))     return { abort: true, reason: "Blocked" };
});
httpServer.onSettlementTimeout(async (txHash, network) => {
  return { confirmed: await verifyOnChain(txHash) };
});
httpServer.setPollDeadline(10000);                 // default 5000ms

// Resource-server lifecycle
resourceServer
  .onBeforeVerify(async (ctx) => { /* { abort, reason } to reject */ })
  .onAfterVerify(async (ctx) => { /* ctx.result */ })
  .onVerifyFailure(async (ctx) => { /* { recovered, result } to recover */ })
  .onBeforeSettle(async (ctx) => { /* same shapes */ })
  .onAfterSettle(async (ctx) => { /* ctx.result */ })
  .onSettleFailure(async (ctx) => { /* same shapes */ });
```

### On-chain timeout recovery (viem)

```typescript
import { createPublicClient, http, defineChain } from "viem";

const xlayer = defineChain({
  id: 196, name: "X Layer",
  nativeCurrency: { name: "OKB", symbol: "OKB", decimals: 18 },
  rpcUrls: { default: { http: ["https://rpc.xlayer.tech"] } },
});
const viemClient = createPublicClient({ chain: xlayer, transport: http() });

httpServer.onSettlementTimeout(async (txHash) => {
  try {
    const receipt = await viemClient.getTransactionReceipt({ hash: txHash as `0x${string}` });
    return { confirmed: receipt?.status === "success" };
  } catch { return { confirmed: false }; }
});
```

### Paywall (`@okxweb3/x402-*`)

```typescript
const paywallConfig = {
  appName?: string;
  appLogo?: string;
  sessionTokenEndpoint?: string;
  currentUrl?: string;
  testnet?: boolean;
};
// Pass as 2nd arg to paymentMiddlewareFromHTTPServer / paymentProxyFromHTTPServer
```

Per-route HTML override: set `customPaywallHtml` on the route config.

---

## COMMON MISTAKES

| Mistake | Fix |
|---|---|
| Forgot `await resourceServer.initialize()` | Call it after the server starts, before any request is handled |
| Used `HTTPFacilitatorClient` | Always use `OKXFacilitatorClient` |
| Fastify: `app.use(paymentMiddlewareFromHTTPServer(...))` | Fastify: `paymentMiddlewareFromHTTPServer(app, httpServer)` — app is first arg |
| Hardcoded token amount ignoring decimals on `@okxweb3/x402-*` | Use USD string `"$0.01"` — SDK converts automatically |
| Passed `"$0.01"` to `@okxweb3/mpp` | MPP `amount` is base units (e.g. `"10000"`); USD strings are not parsed |
| Network other than `eip155:196` | Only X Layer is supported |
| Metered cap: handler omitted `setSettlementOverrides` | Set it on every response, or facilitator settles the FULL cap |
| Metered cap: resolved override `>` cap | Facilitator rejects — clamp on your side first |
| Permit2 without buyer-side approve | Buyer must do one-time `IERC20.approve(PERMIT2, MAX_UINT256)` before first payment |
| Channel: `signer.address !== recipient` | `session({ signer })` fast-fails at startup — make them match |
| Channel: missing `methodDetails.escrowContract` | Required for `open`/`topUp`; provide a deployed escrow address |
| Multi-protocol: routes declared on the x402 resource server | Don't — declare routes once under `paymentRouter({ routes })` |
