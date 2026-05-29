// Run: npx tsx --env-file=.env server.ts
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

// —— MPP setup ——
const saClient = new SaApiClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
});
const mppx = Mppx.create({
  methods: [mppCharge({ saClient })],
  realm: "test realm",
  secretKey: process.env.MPP_SECRET_KEY!,
});

// —— x402 setup (facilitator + scheme; routes are declared on the router) ——
const NETWORK = "eip155:196"; // X Layer Mainnet
const x402Server = new x402ResourceServer(
  new OKXFacilitatorClient({
    apiKey: process.env.OKX_API_KEY!,
    secretKey: process.env.OKX_SECRET_KEY!,
    passphrase: process.env.OKX_PASSPHRASE!,
  }),
).register(NETWORK, new ExactEvmScheme());

// Built-in priorities: MPP=10, x402=20 (MPP wins when both headers present).
// Custom adapters should start at priority ≥ 100.
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
      description: "AI Image Generation Service",
      adapterConfigs: {
        mpp: {
          intent: "charge",
          amount: "10",
          currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736", // currency
          recipient: "0x238193be9e80e68eace3588b45d8cf4a7eae0fa3", // receipt
          description: "AI Image Generation Service",
          methodDetails: { chainId: 196, feePayer: true },
        },
        x402: {
          scheme: "exact",
          network: NETWORK,
          payTo: "0x238193be9e80e68eace3588b45d8cf4a7eae0fa3", // receipt
          price: "$0.00001",
          description: "AI Image Generation Service",
          mimeType: "application/json",
        },
      },
    },
  },
});

// Protocol-agnostic. Runs only after one of the adapters has verified payment.
const handler = protect(async () =>
  Response.json({
    imageUrl: "https://placehold.co/512x512/png?text=AI+Generated",
    prompt: "a sunset over mountains",
  }),
);

http
  .createServer(async (req, res) => {
    const url = `http://${req.headers.host ?? "localhost:4000"}${req.url}`;
    const webReq = new Request(url, {
      method: req.method,
      headers: new Headers(req.headers as Record<string, string>),
    });
    const webRes =
      new URL(url).pathname === "/generateImg" && req.method === "GET"
        ? await handler(webReq)
        : new Response("not found", { status: 404 });
    res.statusCode = webRes.status;
    webRes.headers.forEach((v, k) => res.setHeader(k, v));
    res.end(await webRes.text());
  })
  .listen(4000);
