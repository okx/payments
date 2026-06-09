/**
 * x402 Seller Demo — X Layer testnet (eip155:1952).
 *
 * Two protected endpoints:
 *   GET /weather  — $0.01 USDT0 per call (exact scheme)
 *   GET /report   — $0.05 USDT0 per call (exact scheme)
 *
 * The demo uses USD-denominated prices; the SDK auto-converts to USDT0
 * atomic units (6 decimals) based on the default asset registered for
 * the `eip155:1952` network.
 */
import express from "express";
import { OKXFacilitatorClient } from "@okxweb3/app-x402-core";
import {
  paymentMiddleware,
  x402ResourceServer,
} from "@okxweb3/app-x402-express";
import { ExactEvmScheme } from "@okxweb3/app-x402-evm/exact/server";

const NETWORK = "eip155:1952"; // X Layer testnet
const PORT = Number.parseInt(process.env.PORT ?? "3000", 10);

function requireEnv(key: string): string {
  const v = process.env[key];
  if (!v) {
    throw new Error(
      `Missing required env var ${key}. Copy .env.example to .env and fill it in.`,
    );
  }
  return v;
}

const SELLER_ADDRESS = requireEnv("SELLER_ADDRESS");

const facilitatorClient = new OKXFacilitatorClient({
  apiKey: requireEnv("OKX_API_KEY"),
  secretKey: requireEnv("OKX_SECRET_KEY"),
  passphrase: requireEnv("OKX_PASSPHRASE"),
  baseUrl: process.env.OKX_BASE_URL ?? "https://web3.okx.com",
});

const resourceServer = new x402ResourceServer(facilitatorClient).register(
  NETWORK,
  new ExactEvmScheme(),
);

const routes = {
  "GET /weather": {
    accepts: {
      scheme: "exact",
      network: NETWORK,
      payTo: SELLER_ADDRESS,
      price: "$0.01",
    },
    description: "Weather data lookup",
    mimeType: "application/json",
  },
  "GET /report": {
    accepts: {
      scheme: "exact",
      network: NETWORK,
      payTo: SELLER_ADDRESS,
      price: "$0.05",
    },
    description: "Detailed weather report",
    mimeType: "application/json",
  },
} as const;

const app = express();
app.disable("x-powered-by");

app.use(paymentMiddleware(routes, resourceServer));

app.get("/", (_req, res) => {
  res.json({
    status: "ok",
    network: NETWORK,
    protectedRoutes: Object.keys(routes),
  });
});

app.get("/weather", (_req, res) => {
  res.json({
    city: "X Layer City",
    temperature: "22°C",
    condition: "Sunny",
  });
});

app.get("/report", (_req, res) => {
  res.json({
    city: "X Layer City",
    forecast: [
      { day: "Mon", high: "24°C", low: "15°C", condition: "Sunny" },
      { day: "Tue", high: "22°C", low: "14°C", condition: "Cloudy" },
      { day: "Wed", high: "20°C", low: "13°C", condition: "Rain" },
    ],
  });
});

app.listen(PORT, () => {
  console.log(`[okx-x402-demo] listening on http://localhost:${PORT}`);
  console.log(`[okx-x402-demo] network=${NETWORK} payTo=${SELLER_ADDRESS}`);
});
