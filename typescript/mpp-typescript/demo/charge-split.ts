// Run: npx tsx --env-file=.env server.ts
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
  realm: "test realm",
  secretKey: process.env.MPP_SECRET_KEY!,
});

// Total 100 base units; primary keeps 50, splits take 30 + 20.
// Constraints: sum(splits) < amount; splits.length <= 10;
//              recipient must be 40-hex EIP-55.
// Buyer signs one EIP-3009 per split (one to primary + one per entry).
const splits = [
  { amount: "30", recipient: "0x....321a1308", memo: "partner-a" },
  { amount: "20", recipient: "0x....d31a6608", memo: "partner-b" },
];

// Only difference vs single charge: methodDetails.splits.
const CHARGE = {
  amount: "100",
  currency: "0x...adb21711",                 // currency
  recipient: "0x...378211",                  // primary receipt
  description: "One premium API call (split)",
  methodDetails: { chainId: 196, feePayer: true, splits },
} as const;

async function premium(request: Request): Promise<Response> {
  const result = await mppx.charge(CHARGE)(request);
  if (result.status === 402) return result.challenge;
  return result.withReceipt(Response.json({ data: "premium content" }));
}

http.createServer(async (req, res) => {
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
}).listen(4000);
