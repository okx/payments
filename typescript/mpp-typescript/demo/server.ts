/**
 * Unified seller server — node:http + mppx/server (Web Standards handler).
 *
 *   GET /charge/weather  → one-time payment
 *   GET /session/weather → session-based pay-per-request
 *
 * Port: 3000 (configurable via `SERVER_PORT`).
 */
import * as http from "node:http";
import { Mppx } from "@okxweb3/app-mpp";
import { charge, session } from "@okxweb3/app-mpp/evm/server";
import { SaApiClient } from "@okxweb3/app-mpp/evm";
import { privateKeyToAccount } from "viem/accounts";

const PORT = Number.parseInt(process.env.SERVER_PORT ?? "3000", 10);

/**
 * Sensitive configuration is loaded from `.env` (see `.env.example`).
 * Run `cp .env.example .env` and edit before starting; npm scripts pass
 * `node --env-file=.env` so values are injected into `process.env`.
 */
function requireEnv(key: string): string {
  const v = process.env[key];
  if (!v) {
    throw new Error(
      `Missing required env var ${key}. Did you copy .env.example to .env?`,
    );
  }
  return v;
}

const OKX_API_KEY = requireEnv("OKX_API_KEY");
const OKX_SECRET_KEY = requireEnv("OKX_SECRET_KEY");
const OKX_PASSPHRASE = requireEnv("OKX_PASSPHRASE");
const MPPX_SECRET_KEY = requireEnv("MPPX_SECRET_KEY");
const SELLER_PRIVATE_KEY = requireEnv("SELLER_PRIVATE_KEY") as `0x${string}`;
const SELLER_ADDRESS = requireEnv("SELLER_ADDRESS");

function log(tag: string, msg: string) {
  console.log(`  [${tag}] ${msg}`);
}

// `SA_BASE_URL` can override the default production endpoint.
const SA_BASE_URL = process.env.SA_BASE_URL ?? "https://web3.okx.com";

const saClient = new SaApiClient({
  apiKey: OKX_API_KEY,
  secretKey: OKX_SECRET_KEY,
  passphrase: OKX_PASSPHRASE,
  baseUrl: SA_BASE_URL,
  // Surface SA API errors (non-2xx HTTP or business code != 0) for debugging.
  onError: (info) => {
    log(
      "SaApiError",
      `${info.method} ${info.path} -> http=${info.httpStatus}` +
        (info.code !== undefined ? ` code=${info.code}` : "") +
        (info.msg ? ` msg=${info.msg}` : "") +
        `\n    request: ${info.requestBody ?? "<none>"}` +
        `\n    response: ${info.responseBody ?? "<none>"}`,
    );
  },
});

// Seller signing account for EIP-712 settle / close authorizations.
// Replace with KMS / HSM or viem `WalletClient` in production.
const sellerSigner = privateKeyToAccount(SELLER_PRIVATE_KEY);

const mppx = Mppx.create({
  methods: [charge({ saClient }), session({ saClient, signer: sellerSigner })],
  realm: "demo.merchant.com",
  secretKey: MPPX_SECRET_KEY,
});

const CHARGE_CONFIG = {
  amount: "10",
  currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
  recipient: SELLER_ADDRESS,
  description: "Weather API call",
  externalId: "weather-001",
  methodDetails: { chainId: 196, feePayer: true },
} as const;

const SESSION_CONFIG = {
  amount: "10",
  currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
  recipient: SELLER_ADDRESS,
  description: "Weather API session",
  unitType: "request",
  suggestedDeposit: "500",
} as const;

const WEATHER = {
  xlayer: {
    city: "X Layer City",
    temperature: "22°C",
    condition: "Sunny",
    forecast: "Clear skies for 3 days",
  },
  ethereum: {
    city: "Ethereum Town",
    temperature: "18°C",
    condition: "Cloudy",
    forecast: "Rain expected tomorrow",
  },
  default: {
    city: "Crypto Valley",
    temperature: "20°C",
    condition: "Partly Cloudy",
    forecast: "Mild weather ahead",
  },
} as Record<string, object>;

async function handler(request: Request): Promise<Response> {
  const url = new URL(request.url);

  if (url.pathname === "/" && request.method === "GET") {
    return Response.json({ status: "ok" });
  }

  if (url.pathname === "/charge/weather" && request.method === "GET") {
    const result = await mppx.charge(CHARGE_CONFIG)(request);

    if (result.status === 402) return result.challenge;

    log("Charge", "paid -> 200");
    return result.withReceipt(Response.json(WEATHER["xlayer"]));
  }

  if (url.pathname === "/session/weather" && request.method === "GET") {
    const result = await mppx.session(SESSION_CONFIG)(request);

    if (result.status === 402) return result.challenge;

    const city = url.searchParams.get("city") ?? "default";
    log("Session", `verified -> serve city=${city} (or 204 for management action)`);
    return result.withReceipt(
      Response.json(WEATHER[city] ?? WEATHER["default"]),
    );
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
    const webRes = await handler(webReq);
    await sendWebResponse(res, webRes);
  } catch (err) {
    log("Error", err instanceof Error ? err.stack ?? err.message : String(err));
    res.statusCode = 500;
    res.end("Internal Server Error");
  }
});

server.listen(PORT, () => {
  console.log(`[Server] http://localhost:${PORT}`);
  console.log(`[Server] GET /charge/weather  (charge)`);
  console.log(`[Server] GET /session/weather (session, Web Standards mode)`);
});

export { server, PORT };
