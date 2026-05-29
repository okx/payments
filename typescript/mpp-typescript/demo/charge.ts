// Run: node --env-file=.env --experimental-strip-types server.ts
//      or: npx tsx --env-file=.env server.ts
import * as http from "node:http";
import { Mppx } from "@okxweb3/mpp";
import { charge } from "@okxweb3/mpp/evm/server";
import { SaApiClient } from "@okxweb3/mpp/evm";

// SA-API client (broadcasts EIP-3009 in transaction mode).
const saClient = new SaApiClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
});

const mppx = Mppx.create({
  methods: [charge({ saClient })],
  realm: "test realm",
  secretKey: process.env.MPP_SECRET_KEY!,
});

// Per-route price (base units; "100" = 0.0001 of a 6-decimal token).
// fee_payer = true → seller broadcasts the EIP-3009 (transaction mode).
const CHARGE = {
  amount: "10",
  currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
  recipient: process.env.SELLER_ADDRESS,
  description: "Weather API call",
  externalId: "weather-001",
  methodDetails: { chainId: 196, feePayer: true },
} as const;

// Runs only after verify + settle.
async function premium(request: Request): Promise<Response> {
  const result = await mppx.charge(CHARGE)(request);
  if (result.status === 402) return result.challenge;
  return result.withReceipt(Response.json({ data: "premium content" }));
}

// node:http ↔ Web Standards bridge (10 lines).
http
  .createServer(async (req, res) => {
    const url = `http://${req.headers.host ?? "localhost:4000"}${req.url}`;
    const webReq = new Request(url, {
      method: req.method,
      headers: new Headers(req.headers as Record<string, string>),
    });
    const webRes =
      new URL(url).pathname === "/api/premium"
        ? await premium(webReq)
        : new Response("not found", { status: 404 });
    res.statusCode = webRes.status;
    webRes.headers.forEach((v, k) => res.setHeader(k, v));
    res.end(await webRes.text());
  })
  .listen(4000);
