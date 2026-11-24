/**
 * Admin UI + APIs (merchant-side):
 *   GET  /admin                  vanilla-JS dashboard
 *   GET  /admin/api/state        plans + subscriptions snapshot
 *   POST /admin/api/charge/:sub  manual charge
 *   POST /admin/api/cancel/:sub  merchant-initiated cancel (signed locally)
 *   POST /admin/api/sync/:sub    pull latest sub from facilitator
 *   GET  /admin/api/supported    debug: live /supported dump
 */
/* eslint-disable @typescript-eslint/no-explicit-any, no-console */
import type { Express, Request, Response } from "express";

import type {
  InMemoryStore,
  SubscriptionClient,
} from "@okxweb3/app-x402-core/subscription";
import type { OKXFacilitatorClient } from "@okxweb3/app-x402-core";
import type { PermitSubscriptionScheme } from "@okxweb3/app-x402-evm/subscription";

import { ADMIN_HTML } from "./admin-html";
import { signMerchantCancelAuth } from "./merchant-signer";
import { PLAN_ACCESS } from "./plan-accepts";
import {
  ALL_PLANS,
  MERCHANT_ADDR,
  MERCHANT_CAN_CANCEL,
  NETWORK,
  TOKEN_ADDR,
  TOKEN_DECIMALS,
} from "./shared";

export interface AdminRouteDeps {
  store: InMemoryStore;
  scheme: PermitSubscriptionScheme;
  subscriptionClient: SubscriptionClient;
  facilitator: OKXFacilitatorClient;
}

export function mountAdminRoutes(app: Express, deps: AdminRouteDeps): void {
  const { store, scheme, subscriptionClient, facilitator } = deps;

  app.get("/admin", (_req, res) => res.type("html").send(ADMIN_HTML));

  app.get("/admin/api/state", async (_req, res) => {
    const subs = await store.list();
    // EIP-712 domain populated by the resource server's first
    // enhancePaymentRequirements call (after resourceServer.initialize()
    // pulled /supported). verifyingContract = subscription contract addr.
    let contracts: { subscription: string; permit2: string } | null = null;
    try {
      const d = scheme.getDomain();
      contracts = {
        subscription: String(d.verifyingContract ?? ""),
        // permit2 lives in the cached snapshot too; demo just hides it if not yet seeded.
        permit2: "",
      };
    } catch {
      // scheme hasn't seen a request yet → leave null
    }
    res.json({
      network: NETWORK,
      merchant: MERCHANT_ADDR,
      token: { address: TOKEN_ADDR, decimals: TOKEN_DECIMALS },
      contracts,
      plans: ALL_PLANS.map(p => ({
        id: p.id,
        name: p.name,
        tier: p.tier,
        amountPerPeriod: p.amountPerPeriod,
        periodSec: p.periodSec,
        maxPeriods: p.maxPeriods,
        accessibleRoutes: PLAN_ACCESS[p.id] ?? [],
      })),
      subscriptions: subs,
    });
  });

  app.post("/admin/api/charge/:subId", async (req: Request, res: Response) => {
    try {
      const result = await subscriptionClient.charge(String(req.params.subId));
      res.json({ ok: true, result });
    } catch (err: any) {
      res.status(400).json({ ok: false, error: err?.message ?? String(err), code: err?.code });
    }
  });

  app.post("/admin/api/cancel/:subId", async (req: Request, res: Response) => {
    if (!MERCHANT_CAN_CANCEL) {
      res
        .status(403)
        .json({ ok: false, error: "MERCHANT_PRIVATE_KEY not set; merchant cancel disabled" });
      return;
    }
    try {
      const auth = await signMerchantCancelAuth(String(req.params.subId), scheme);
      await subscriptionClient.cancelBySeller(String(req.params.subId), auth, "merchant_initiated");
      res.json({ ok: true });
    } catch (err: any) {
      res.status(400).json({ ok: false, error: err?.message ?? String(err) });
    }
  });

  app.post("/admin/api/sync/:subId", async (req: Request, res: Response) => {
    try {
      const sub = await subscriptionClient.syncFromChain(String(req.params.subId));
      res.json({ ok: true, subscription: sub });
    } catch (err: any) {
      res.status(400).json({ ok: false, error: err?.message ?? String(err) });
    }
  });

  // Operator debug: hit GET /supported live and dump the raw response.
  app.get("/admin/api/supported", async (_req, res) => {
    try {
      const supported = await facilitator.getSupported();
      console.log("[demo seller] facilitator GET /supported response:");
      console.log(JSON.stringify(supported, null, 2));
      res.json({ ok: true, supported });
    } catch (err: any) {
      res.status(502).json({ ok: false, error: err?.message ?? String(err) });
    }
  });
}
