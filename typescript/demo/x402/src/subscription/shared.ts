/**
 * Demo configuration. Secrets come from `.env`.
 *
 * MERCHANT key vs address:
 *   - `MERCHANT_ADDRESS` (required) — receiving address, used as `payTo` in
 *     every plan. Pure on-chain identifier; no signing capability needed.
 *   - `MERCHANT_PRIVATE_KEY` (optional) — only required if the seller wants
 *     to invoke merchant-initiated cancel (signs an EIP-712 CancelAuth with
 *     `initiator=merchant`). Subscribe / charge / change / cancel-by-buyer /
 *     getSubscription all use OKX API-key auth — no merchant key needed.
 */
import { privateKeyToAccount, type PrivateKeyAccount } from "viem/accounts";
import { isAddress, type Hex, type TypedDataDomain } from "viem";

import type { PlanCatalogEntry } from "@okxweb3/app-x402-core/subscription";
import {
  getDefaultAsset,
  SUBSCRIPTION_CONTRACT_ADDRESS,
  SUBSCRIPTION_DOMAIN_NAME,
  SUBSCRIPTION_DOMAIN_VERSION,
} from "@okxweb3/app-x402-evm";

// Optional merchant signing key. Only needed for merchant-initiated cancel.
export const merchantAccount: PrivateKeyAccount | null = process.env.MERCHANT_PRIVATE_KEY
  ? privateKeyToAccount(process.env.MERCHANT_PRIVATE_KEY as Hex)
  : null;

// Merchant on-chain address. If MERCHANT_PRIVATE_KEY is set, derive from it;
// otherwise require MERCHANT_ADDRESS explicitly. The two must agree if both
// are present — protects against accidental key/address mismatch.
function resolveMerchantAddr(): string {
  const fromKey = merchantAccount?.address;
  const fromEnv = process.env.MERCHANT_ADDRESS;
  if (!fromKey && !fromEnv) {
    throw new Error("Set MERCHANT_ADDRESS or MERCHANT_PRIVATE_KEY in .env");
  }
  if (fromKey && fromEnv && fromKey.toLowerCase() !== fromEnv.toLowerCase()) {
    throw new Error(
      `MERCHANT_ADDRESS (${fromEnv}) does not match address derived from MERCHANT_PRIVATE_KEY (${fromKey})`,
    );
  }
  const addr = fromKey ?? fromEnv!;
  if (!isAddress(addr)) throw new Error(`Invalid merchant address: ${addr}`);
  return addr;
}
export const MERCHANT_ADDR = resolveMerchantAddr();

/** True when the demo is configured to sign merchant-initiated CancelAuth. */
export const MERCHANT_CAN_CANCEL = merchantAccount !== null;

// CAIP-2 network identifier (default X Layer mainnet).
export const NETWORK = (process.env.CHAIN_NETWORK ?? "eip155:196") as `eip155:${string}`;

// ERC-20 token. Defaults to the SDK's per-network stablecoin (X Layer
// mainnet → USDT0 `0x779ded…`; testnet → USDT0 `0x9e29b3…`) — same map
// exact / upto / aggr_deferred use. Override `TOKEN_ADDRESS` in .env to
// point at a different ERC-20.
const defaultAsset = getDefaultAsset(NETWORK);
export const TOKEN_ADDR = process.env.TOKEN_ADDRESS ?? defaultAsset.address;

// EIP-712 domain. `verifyingContract` is the A2APaySubscription contract,
// shared by terms / CancelAuth / pending change.
export const SELLER_DOMAIN: TypedDataDomain = {
  name: SUBSCRIPTION_DOMAIN_NAME,
  version: SUBSCRIPTION_DOMAIN_VERSION,
  chainId: Number(NETWORK.split(":")[1]),
  verifyingContract: SUBSCRIPTION_CONTRACT_ADDRESS,
};

// Four plans for upgrade / downgrade / charge testing. periodSec=60 (1 min)
// so charge() can be triggered repeatedly without waiting a real month.
// amountPerPeriod in token base units (6 decimals): 0.000001 / 0.000005 / 0.00001 / 0.00002.
const TEST_PERIOD_SEC = 300; // 5 minutes per period
const TEST_MAX_PERIODS = 6;  // 30 minutes total (6 periods × 5 min)

export const BASIC_PLAN: PlanCatalogEntry = {
  id: "basic_monthly",
  tier: 1,
  amountPerPeriod: "1",
  periodSec: TEST_PERIOD_SEC,
  maxPeriods: TEST_MAX_PERIODS,
  payTo: MERCHANT_ADDR,
  name: "Basic",
};

export const PRO_PLAN: PlanCatalogEntry = {
  id: "pro_monthly",
  tier: 2,
  amountPerPeriod: "5",
  periodSec: TEST_PERIOD_SEC,
  maxPeriods: TEST_MAX_PERIODS,
  payTo: MERCHANT_ADDR,
  name: "Pro",
};

export const ENTERPRISE_PLAN: PlanCatalogEntry = {
  id: "enterprise_monthly",
  tier: 3,
  amountPerPeriod: "10",
  periodSec: TEST_PERIOD_SEC,
  maxPeriods: TEST_MAX_PERIODS,
  payTo: MERCHANT_ADDR,
  name: "Enterprise",
};

export const ULTIMATE_PLAN: PlanCatalogEntry = {
  id: "ultimate_monthly",
  tier: 4,
  amountPerPeriod: "20",
  periodSec: TEST_PERIOD_SEC,
  maxPeriods: TEST_MAX_PERIODS,
  payTo: MERCHANT_ADDR,
  name: "Ultimate",
};

export const ALL_PLANS = [BASIC_PLAN, PRO_PLAN, ENTERPRISE_PLAN, ULTIMATE_PLAN] as const;

/** Token-unit decimals — default from the SDK's per-network stablecoin info. */
export const TOKEN_DECIMALS = Number(process.env.TOKEN_DECIMALS ?? defaultAsset.decimals);
