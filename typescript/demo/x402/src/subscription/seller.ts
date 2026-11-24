/* eslint-disable no-console */
import express from "express";

import { OKXFacilitatorClient } from "@okxweb3/app-x402-core";
import { x402ResourceServer } from "@okxweb3/app-x402-core/server";
import { InMemoryStore, SubscriptionClient } from "@okxweb3/app-x402-core/subscription";
import { PermitSubscriptionScheme } from "@okxweb3/app-x402-evm/subscription";
import { paymentMiddleware } from "@okxweb3/app-x402-express";

import { wrapFacilitator } from "./logging-facilitator";
import { buildRoutes } from "./plan-accepts";
import { mountAdminRoutes } from "./routes-admin";
import { mountBuyerRoutes } from "./routes-buyer";
import { NETWORK } from "./shared";

function requireEnv(key: string): string {
  const v = process.env[key];
  if (!v) throw new Error(`Missing required env var: ${key}`);
  return v;
}

export interface DemoSellerHandle {
  url: string;
  close: () => Promise<void>;
  client: SubscriptionClient;
  store: InMemoryStore;
}

export async function startSeller(port = 4242): Promise<DemoSellerHandle> {
  const realFacilitator = new OKXFacilitatorClient({
    apiKey: requireEnv("OKX_API_KEY"),
    secretKey: requireEnv("OKX_SECRET_KEY"),
    passphrase: requireEnv("OKX_PASSPHRASE"),
    baseUrl: process.env.OKX_BASE_URL,
  });
  // Wrap with a logging proxy so every facilitator call prints its
  // request + response envelope.
  const facilitator = wrapFacilitator(realFacilitator);

  try {
    const supported = await facilitator.getSupported();
    console.log("[demo seller] /supported (full):");
    console.log(JSON.stringify(supported, null, 2));
  } catch (err) {
    console.warn("[demo seller] /supported probe failed:", (err as Error).message);
  }

  const store = new InMemoryStore();
  const scheme = new PermitSubscriptionScheme({
    facilitator,
    network: NETWORK,
    store,
  });
  const subscriptionClient = new SubscriptionClient({ scheme, store });

  // Register the scheme so getDomain() returns the correct EIP-712 domain
  // for merchant-initiated CancelAuth signing.
  const resourceServer = new x402ResourceServer(facilitator).register(NETWORK, scheme);
  await resourceServer.initialize();

  const app = express();
  app.use(express.json());

  app.use(paymentMiddleware(buildRoutes(), resourceServer));
  mountBuyerRoutes(app);
  mountAdminRoutes(app, { store, scheme, subscriptionClient, facilitator });

  return new Promise(resolve => {
    const server = app.listen(port, "127.0.0.1", () => {
      const url = `http://127.0.0.1:${port}`;
      console.log(`[demo seller] listening at ${url}`);
      console.log(`[demo seller] admin UI: ${url}/admin`);
      resolve({
        url,
        client: subscriptionClient,
        store,
        close: () => new Promise<void>(r2 => server.close(() => r2())),
      });
    });
  });
}
