# @okxweb3/app-payment-router

> Unified payment router — lets the same URL support both [**MPP**](https://mpp.dev) and [**x402**](https://x402.org) 402 payment protocols simultaneously.

API shape aligned with the Rust [`payment-router-axum`](https://github.com/okx/payments): built on Web Standards `Request` / `Response`, not bound to any of Express / Hono / Next; as long as you can convert a request into a `Request` and write a `Response` back to the response stream, it works.

## Installation

```bash
npm install @okxweb3/app-payment-router @okxweb3/app-mpp @okxweb3/app-x402-core @okxweb3/app-x402-evm
```

`@okxweb3/app-mpp` and `@okxweb3/app-x402-core` are optional peerDependencies — if you only use one protocol, install only that one.

## Quick Start

```typescript
import * as http from "node:http";
import { Mppx } from "@okxweb3/app-mpp";
import { charge as mppCharge } from "@okxweb3/app-mpp/evm/server";
import { SaApiClient } from "@okxweb3/app-mpp/evm";
import { OKXFacilitatorClient } from "@okxweb3/app-x402-core";
import {
  x402HTTPResourceServer,
  x402ResourceServer,
} from "@okxweb3/app-x402-core/server";
import { ExactEvmScheme } from "@okxweb3/app-x402-evm/exact/server";
import {
  MppAdapter,
  X402Adapter,
  paymentRouter,
} from "@okxweb3/app-payment-router";

// 1. MPP
const saClient = new SaApiClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
});
const mppx = Mppx.create({
  methods: [mppCharge({ saClient })],
  realm: "demo.merchant.com",
  secretKey: process.env.MPPX_SECRET_KEY!,
});

// 2. x402 (register facilitator + scheme only, without routes)
const x402Server = new x402ResourceServer(
  new OKXFacilitatorClient({
    apiKey: process.env.OKX_API_KEY!,
    secretKey: process.env.OKX_SECRET_KEY!,
    passphrase: process.env.OKX_PASSPHRASE!,
  }),
).register("eip155:196", new ExactEvmScheme());

// 3. Unified router — all per-route billing params declared in adapterConfigs.<name>
const protect = paymentRouter({
  adapters: [
    new MppAdapter({ mppx }),
    new X402Adapter({
      resourceServer: x402Server,
      httpResourceServerCtor: x402HTTPResourceServer,
    }),
  ],
  routes: {
    "GET /generateImg": {
      adapterConfigs: {
        mpp: {
          intent: "charge",
          amount: "10000",
          currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
          recipient: process.env.PAY_TO!,
          methodDetails: { chainId: 196, feePayer: true },
        },
        x402: {
          scheme: "exact",
          network: "eip155:196",
          payTo: process.env.PAY_TO!,
          price: "$0.01",
        },
      },
    },
  },
});

// 4. Business handler is protocol-agnostic
const handler = protect(async (_req) =>
  Response.json({ imageUrl: "https://example.com/img.png" }),
);

// 5. node:http bridge
http.createServer(async (req, res) => {
  const host = req.headers.host ?? "localhost:4000";
  const webReq = new Request(`http://${host}${req.url}`, {
    method: req.method,
    headers: new Headers(req.headers as Record<string, string>),
  });
  const webRes = await handler(webReq);
  res.statusCode = webRes.status;
  webRes.headers.forEach((v, k) => res.setHeader(k, v));
  res.end(await webRes.text());
}).listen(4000);
```

## How It Works

```
HTTP request → toWebRequest → paymentRouter(handler) → business handler → Response → write back
                                │
                                ├─ route not in routes  → pass through to inner
                                ├─ protocol detected (mpp / x402) → adapter.handle()
                                │     └─ verify → inner → settle → attach receipt
                                └─ none detected        → merge challenges → 402
```

- **MPP** detect: `Authorization: Payment ...` (priority=10)
- **x402** detect: `payment-signature` / `x-payment` header (priority=20)
- When both are present, sorted by ascending priority; MPP wins

## Custom Protocols

Implement `ProtocolAdapter` to plug in a new protocol:

```typescript
import type { ProtocolAdapter, Handler } from "@okxweb3/app-payment-router";

class MyProtocolAdapter implements ProtocolAdapter {
  readonly name = "my-proto";
  readonly priority = 100;

  detect(req: Request): boolean { return req.headers.has("x-my-proto"); }
  async buildChallenge(_req: Request, _cfg: unknown) {
    return { "WWW-Authenticate": `my-proto realm="..."` };
  }
  async handle(req: Request, _cfg: unknown, inner: Handler) {
    // verify → call inner → settle → attach receipt
    return inner(req);
  }
}
```

Simply add a `my-proto: { ... }` field in `routes[*].adapterConfigs` and add the adapter to `adapters: [...]` — no changes to the middleware core are needed.

## License

Apache-2.0 © OKX
