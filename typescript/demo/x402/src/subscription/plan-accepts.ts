/**
 * Maps the static PlanCatalogEntry shape (`shared.ts`) into the x402 route
 * `accepts` entry shape consumed by `paymentMiddleware`. Encodes the
 * per-route plan allowlist for progressive access:
 *
 *   /basic       → all 4 plans
 *   /pro         → pro / enterprise / ultimate
 *   /enterprise  → enterprise / ultimate
 *   /ultimate    → ultimate only
 */
import {
  ALL_PLANS,
  BASIC_PLAN,
  ENTERPRISE_PLAN,
  NETWORK,
  PRO_PLAN,
  TOKEN_ADDR,
  ULTIMATE_PLAN,
} from "./shared";

export function buildAccepts(plan: typeof BASIC_PLAN) {
  return {
    scheme: "period",
    payTo: plan.payTo,
    price: { asset: plan.asset ?? TOKEN_ADDR, amount: plan.amountPerPeriod },
    network: NETWORK,
    extra: {
      amountPerPeriod: plan.amountPerPeriod,
      periodMode: plan.periodMode ?? 0,
      periodSec: plan.periodSec,
      maxPeriods: plan.maxPeriods,
      plan: { id: plan.id, tier: plan.tier, name: plan.name },
    },
  };
}

export function buildRoutes() {
  const allPlans = ALL_PLANS.map(buildAccepts);
  const proAndUp = [PRO_PLAN, ENTERPRISE_PLAN, ULTIMATE_PLAN].map(buildAccepts);
  const enterpriseAndUp = [ENTERPRISE_PLAN, ULTIMATE_PLAN].map(buildAccepts);
  const ultimateOnly = [ULTIMATE_PLAN].map(buildAccepts);
  return {
    "GET /api/protected/basic": { accepts: allPlans },
    "GET /api/protected/pro": { accepts: proAndUp },
    "GET /api/protected/enterprise": { accepts: enterpriseAndUp },
    "GET /api/protected/ultimate": { accepts: ultimateOnly },
    "GET /api/change-plan": { accepts: allPlans, operation: "change" as const },
    "POST /api/cancel-subscription": {
      accepts: allPlans,
      operation: "cancel" as const,
    },
    "POST /api/cancel-pending-change": {
      accepts: allPlans,
      operation: "cancel-pending-change" as const,
    },
  };
}

/**
 * Plan → accessible route paths, used by the admin dashboard to show which
 * routes each plan tier unlocks.
 */
export const PLAN_ACCESS: Record<string, string[]> = {
  [BASIC_PLAN.id]: ["/api/protected/basic"],
  [PRO_PLAN.id]: ["/api/protected/basic", "/api/protected/pro"],
  [ENTERPRISE_PLAN.id]: [
    "/api/protected/basic",
    "/api/protected/pro",
    "/api/protected/enterprise",
  ],
  [ULTIMATE_PLAN.id]: [
    "/api/protected/basic",
    "/api/protected/pro",
    "/api/protected/enterprise",
    "/api/protected/ultimate",
  ],
};
