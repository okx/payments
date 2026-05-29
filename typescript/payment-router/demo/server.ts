/**
 * Dual-protocol unified payment demo (MPP session + x402 aggr_deferred),
 * built on `node:http` and the Web Standards request/response interfaces.
 *
 * The same route `GET /generateImg` is protected by both MPP **session** and
 * x402 **aggr_deferred**. Per-route pricing for both protocols is declared in
 * `paymentRouter.routes[*].adapterConfigs`.
 *
 * Run:
 *   1. cp .env.example .env  (fill in OKX credentials, seller key, ...)
 *   2. npm run server
 *   3. Send paid requests to :4000/generateImg with either protocol.
 */

import * as http from "node:http";
import { privateKeyToAccount } from "viem/accounts";

import { Mppx } from "@okxweb3/mpp";
import { charge as mppCharge, session as mppSession } from "@okxweb3/mpp/evm/server";
import { SaApiClient } from "@okxweb3/mpp/evm";

import { OKXFacilitatorClient } from "@okxweb3/x402-core";
import {
  x402HTTPResourceServer,
  x402ResourceServer,
} from "@okxweb3/x402-core/server";
import { ExactEvmScheme } from "@okxweb3/x402-evm/exact/server";
import { AggrDeferredEvmScheme } from "@okxweb3/x402-evm/deferred/server";

import {
  MppAdapter,
  X402Adapter,
  paymentRouter,
} from "@okxweb3/payment-router";

const PORT = Number.parseInt(process.env.SERVER_PORT ?? "4000", 10);

function requireEnv(key: string): string {
  const v = process.env[key];
  if (!v)
    throw new Error(`Missing env ${key}; did you copy .env.example to .env?`);
  return v;
}

const OKX_API_KEY = requireEnv("OKX_API_KEY");
const OKX_SECRET_KEY = requireEnv("OKX_SECRET_KEY");
const OKX_PASSPHRASE = requireEnv("OKX_PASSPHRASE");
const MPPX_SECRET_KEY = requireEnv("MPPX_SECRET_KEY");
const SELLER_PRIVATE_KEY = requireEnv("SELLER_PRIVATE_KEY") as `0x${string}`;
const PAY_TO = process.env.SELLER_ADDRESS ?? process.env.PAY_TO_ADDRESS;
if (!PAY_TO) {
  throw new Error(
    "Missing env SELLER_ADDRESS (or PAY_TO_ADDRESS); did you copy .env.example to .env?",
  );
}

// X Layer mainnet
const NETWORK = "eip155:196";
const CHAIN_ID = 196;
// USDT on X Layer.
const TOKEN_ADDRESS = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
// Default escrow contract for MPP session on X Layer (matches SDK Session default).
const ESCROW_CONTRACT = "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b";

const saClient = new SaApiClient({
  apiKey: OKX_API_KEY,
  secretKey: OKX_SECRET_KEY,
  passphrase: OKX_PASSPHRASE,
  baseUrl: process.env.SA_BASE_URL ?? "https://web3.okx.com",
});

// Seller signing account used by MPP session to sign EIP-712 authorizations
// for settle / close.
const sellerSigner = privateKeyToAccount(SELLER_PRIVATE_KEY);

const mppx = Mppx.create({
  methods: [
    mppCharge({ saClient }),
    mppSession({ saClient, signer: sellerSigner }),
  ],
  realm: "demo.merchant.com",
  secretKey: MPPX_SECRET_KEY,
});

const facilitatorClient = new OKXFacilitatorClient({
  apiKey: OKX_API_KEY,
  secretKey: OKX_SECRET_KEY,
  passphrase: OKX_PASSPHRASE,
  baseUrl: process.env.X402_FACILITATOR_URL ?? "https://web3.okx.com",
});

const x402Server = new x402ResourceServer(facilitatorClient)
  .register(NETWORK, new ExactEvmScheme())
  .register(NETWORK, new AggrDeferredEvmScheme());

const protect = paymentRouter({
  // Built-in priorities: MPP=10, x402=20. MPP wins when both headers are present.
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
          intent: "session",
          amount: "10",
          currency: TOKEN_ADDRESS,
          recipient: PAY_TO,
          description: "AI Image Generation Service",
          unitType: "request",
          suggestedDeposit: "500",
          methodDetails: {
            chainId: CHAIN_ID,
            escrowContract: ESCROW_CONTRACT,
          },
        },
        x402: {
          scheme: "aggr_deferred",
          network: NETWORK,
          payTo: PAY_TO,
          price: "$0.00001",
          description: "AI Image Generation Service",
          mimeType: "application/json",
        },
      },
    },
  },
  onError: (err, protocol) => {
    console.error(`[unified] adapter ${protocol} challenge failed:`, err);
  },
});

async function generateImg(_request: Request): Promise<Response> {
  return Response.json({
    success: true,
    imageUrl: "https://placehold.co/512x512/png?text=AI+Generated",
    prompt: "a sunset over mountains",
    timestamp: new Date().toISOString(),
  });
}

const protectedHandler = protect(generateImg);

async function appHandler(request: Request): Promise<Response> {
  const url = new URL(request.url);

  if (url.pathname === "/health") {
    return Response.json({ status: "ok" });
  }

  if (url.pathname === "/generateImg" && request.method === "GET") {
    return protectedHandler(request);
  }

  return new Response("not found", { status: 404 });
}

function toWebRequest(req: http.IncomingMessage): Request {
  const host = req.headers.host ?? `localhost:${PORT}`;
  const url = `http://${host}${req.url ?? "/"}`;
  const headers = new Headers();
  for (const [k, v] of Object.entries(req.headers)) {
    if (v == null) continue;
    if (Array.isArray(v)) for (const item of v) headers.append(k, item);
    else headers.set(k, String(v));
  }
  return new Request(url, { method: req.method ?? "GET", headers });
}

async function sendWebResponse(
  res: http.ServerResponse,
  webRes: Response,
): Promise<void> {
  res.statusCode = webRes.status;
  webRes.headers.forEach((value, key) => res.setHeader(key, value));
  const body = webRes.body ? await webRes.text() : "";
  res.end(body);
}

const server = http.createServer(async (req, res) => {
  try {
    const webReq = toWebRequest(req);
    const webRes = await appHandler(webReq);
    await sendWebResponse(res, webRes);
  } catch (err) {
    console.error("[Server] unhandled error:", err);
    res.statusCode = 500;
    res.end("Internal Server Error");
  }
});

server.listen(PORT, () => {
  console.log(`[Server] http://localhost:${PORT}`);
  console.log(`[Server] GET /health      (free)`);
  console.log(
    `[Server] GET /generateImg (mpp session + x402 aggr_deferred)`,
  );
});

export { server, PORT };
