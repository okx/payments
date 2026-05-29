// Run: npx tsx --env-file=.env server.ts
import * as http from "node:http";
import { privateKeyToAccount } from "viem/accounts";
import { Mppx } from "@okxweb3/mpp";
import { session } from "@okxweb3/mpp/evm/server";
import { SaApiClient } from "@okxweb3/mpp/evm";

const UNIT_PRICE_BASE_UNITS = "100";   // 0.0001 of a 6-decimal token
const UNIT_TYPE = "request";
const SUGGESTED_DEPOSIT = "10000";     // 100× unit price

const saClient = new SaApiClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
});

// viem LocalAccount — replace with WalletClient / KMS / HSM signer in production.
// The session method fast-fails on startup if signer.address !== expected payee.
const sellerSigner = privateKeyToAccount(
  process.env.MPP_MERCHANT_PRIVATE_KEY! as `0x${string}`,
);

// Default in-memory store. Pass `store: ...` for SQLite / Redis / Postgres.
const mppx = Mppx.create({
  methods: [session({ saClient, signer: sellerSigner })],
  realm: "test realm",
  secretKey: process.env.MPP_SECRET_KEY!,
});

// Per-route session config. Charged per call; voucher accumulates;
// settle batches on /session/manage close action.
const SESSION = {
  amount: UNIT_PRICE_BASE_UNITS,
  currency: "0x...adb21711",                 // currency
  recipient: "0x...378211",                  // receipt
  description: "Pay-per-use API",
  unitType: UNIT_TYPE,
  suggestedDeposit: SUGGESTED_DEPOSIT,
  methodDetails: {
    chainId: 196,                            // X Layer
    escrowContract: process.env.MPP_ESCROW!, // 40-hex escrow address
    feePayer: true,
    minVoucherDelta: "0",
  },
} as const;

// Routes by `payload.action`: open / voucher / topUp / close.
// mppx.session(...)(request) handles all four uniformly:
//   - 402 → challenge response
//   - 200 → action-specific result; withReceipt() attaches Payment-Receipt
async function manage(request: Request): Promise<Response> {
  const result = await mppx.session(SESSION)(request);
  if (result.status === 402) return result.challenge;
  // open / topUp / close → empty 204; voucher → resource body.
  return result.withReceipt(Response.json({ status: "ok" }));
}

http.createServer(async (req, res) => {
  const url = `http://${req.headers.host ?? "localhost:4023"}${req.url}`;
  const webReq = new Request(url, {
    method: req.method,
    headers: new Headers(req.headers as Record<string, string>),
  });
  const path = new URL(url).pathname;
  const webRes =
    path === "/session/manage"
      ? await manage(webReq)
      : new Response("not found", { status: 404 });
  res.statusCode = webRes.status;
  webRes.headers.forEach((v, k) => res.setHeader(k, v));
  res.end(await webRes.text());
}).listen(4023);
