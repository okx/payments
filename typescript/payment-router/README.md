# @okxweb3/payment-router

> 统一支付路由 — 让同一个 URL 同时支持 [**MPP**](https://mpp.dev) 与 [**x402**](https://x402.org) 两种 402 支付协议。

API 形态对齐 Rust [`payment-router-axum`](https://github.com/okx/payments)：基于 Web Standards `Request` / `Response`，不绑定 Express / Hono / Next 任一框架；只要能把请求转成 `Request`、把 `Response` 写回响应流就能用。

## 安装

```bash
npm install @okxweb3/payment-router @okxweb3/mpp @okxweb3/x402-core @okxweb3/x402-evm
```

`@okxweb3/mpp` 与 `@okxweb3/x402-core` 是可选的 peerDependency — 只用其中一个协议时只装一个即可。

## 快速开始

```typescript
import * as http from "node:http";
import { Mppx } from "@okxweb3/mpp";
import { charge as mppCharge } from "@okxweb3/mpp/evm/server";
import { SaApiClient } from "@okxweb3/mpp/evm";
import { OKXFacilitatorClient } from "@okxweb3/x402-core";
import {
  x402HTTPResourceServer,
  x402ResourceServer,
} from "@okxweb3/x402-core/server";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";
import {
  MppAdapter,
  X402Adapter,
  paymentRouter,
} from "@okxweb3/payment-router";

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

// 2. x402（仅注册 facilitator + scheme，不带 routes）
const x402Server = new x402ResourceServer(
  new OKXFacilitatorClient({
    apiKey: process.env.OKX_API_KEY!,
    secretKey: process.env.OKX_SECRET_KEY!,
    passphrase: process.env.OKX_PASSPHRASE!,
  }),
).register("eip155:196", new ExactEvmScheme());

// 3. 统一路由 — per-route 收费参数全部声明在 adapterConfigs.<name>
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

// 4. 业务 handler 协议无关
const handler = protect(async (_req) =>
  Response.json({ imageUrl: "https://example.com/img.png" }),
);

// 5. node:http 桥接
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

## 工作原理

```
HTTP 请求 → toWebRequest → paymentRouter(handler) → 业务 handler → Response → 写回
                                │
                                ├─ 路由不在 routes 里  → 透传 inner
                                ├─ 检出某协议 (mpp / x402) → adapter.handle()
                                │     └─ verify → inner → settle → 加 receipt
                                └─ 都未检出           → 合并 challenge → 402
```

- **MPP** detect: `Authorization: Payment ...`（priority=10）
- **x402** detect: `payment-signature` / `x-payment` header（priority=20）
- 两条同时携带时按 priority 升序，MPP 胜出

## 自定义协议

实现 `ProtocolAdapter` 即可接入新协议：

```typescript
import type { ProtocolAdapter, Handler } from "@okxweb3/payment-router";

class MyProtocolAdapter implements ProtocolAdapter {
  readonly name = "my-proto";
  readonly priority = 100;

  detect(req: Request): boolean { return req.headers.has("x-my-proto"); }
  async buildChallenge(_req: Request, _cfg: unknown) {
    return { "WWW-Authenticate": `my-proto realm="..."` };
  }
  async handle(req: Request, _cfg: unknown, inner: Handler) {
    // verify → 调 inner → settle → 加 receipt
    return inner(req);
  }
}
```

只需在 `routes[*].adapterConfigs` 里加 `my-proto: { ... }` 字段、把 adapter 加进 `adapters: [...]`，中间件内核不需要改动。

## License

Apache-2.0 © OKX
