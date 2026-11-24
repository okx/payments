/* eslint-disable @typescript-eslint/no-explicit-any, no-console */
/**
 * Self-contained runtime checks for the subscription subsystem (core
 * package only). Run with:
 *   npx tsx src/subscription/__verify__.ts
 *
 * Covers:
 *   - codec: payload / accessProof / typed-data / period-math
 *   - store: InMemoryStore round-trip
 *   - error: ChargeError construction
 *   - type-guard: hasSubscriptionCapability narrows correctly
 *
 * The evm scheme runtime (ecrecover + facilitator HTTP) is covered by the
 * evm package's own verify script.
 */
import { keccak256, stringToHex, type TypedDataDomain } from "viem";

import type { PaymentRequirements } from "../types/payments";
import type { AccessProof, Subscription } from "./types";
import {
  asSubscriptionPaymentInner,
  buildAccessProofMessage,
  buildCancelAuthTypedData,
  buildPermit2TypedData,
  buildSubscriptionTermsTypedData,
  ChargeError,
  ChargeErrorCode,
  decodeAccessProof,
  decodePaymentPayload,
  encodeAccessProof,
  encodePaymentPayload,
  hasSubscriptionCapability,
  InMemoryStore,
  parseChainIdFromNetwork,
  parsePaymentRequired,
  ZERO_BYTES32,
  type SubscriptionRequirementsExtra,
} from "./index";

type Test = { name: string; fn: () => void | Promise<void> };
const tests: Test[] = [];
function test(name: string, fn: () => void | Promise<void>): void {
  tests.push({ name, fn });
}
function assert(cond: unknown, msg: string): void {
  if (!cond) throw new Error(msg);
}
function eq<T>(actual: T, expected: T, msg: string): void {
  if (actual !== expected) {
    throw new Error(`${msg}\n  actual:   ${String(actual)}\n  expected: ${String(expected)}`);
  }
}
function deepEq(actual: unknown, expected: unknown, msg: string): void {
  const a = JSON.stringify(actual);
  const b = JSON.stringify(expected);
  if (a !== b) throw new Error(`${msg}\n  actual:   ${a}\n  expected: ${b}`);
}
function expectThrows(fn: () => unknown | Promise<unknown>, needle: string, msg: string): void {
  try {
    const out = fn();
    if (out instanceof Promise) {
      throw new Error(`${msg}: async fn did not throw synchronously`);
    }
  } catch (e) {
    if (!String((e as Error).message).includes(needle)) {
      throw new Error(`${msg}: error did not include "${needle}"; got ${(e as Error).message}`);
    }
    return;
  }
  throw new Error(`${msg}: did not throw`);
}

const DOMAIN: TypedDataDomain = {
  name: "A2APaySubscriptionV3",
  version: "1",
  chainId: 196,
  verifyingContract: "0x0000000000000000000000000000000000000099",
};
const PAYER = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MERCHANT = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TOKEN = "0xcccccccccccccccccccccccccccccccccccccccc";
const SUBSCRIPTION_CONTRACT = "0xdddddddddddddddddddddddddddddddddddddddd";
const PERMIT2_CONTRACT = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const FACILITATOR = "0xffffffffffffffffffffffffffffffffffffffff";
const SUB_ID = `0x${"a".repeat(64)}` as const;

function makeExtra(
  over: Partial<SubscriptionRequirementsExtra> = {},
): SubscriptionRequirementsExtra {
  return {
    contracts: { subscription: SUBSCRIPTION_CONTRACT, permit2: PERMIT2_CONTRACT },
    facilitator: FACILITATOR,
    amountPerPeriod: "5000000",
    periodSec: 2_592_000,
    maxPeriods: 12,
    plan: { id: "pro_monthly", tier: 2 },
    domain: DOMAIN,
    ...over,
  };
}
function makeRequirements(over: Partial<PaymentRequirements> = {}): PaymentRequirements {
  return {
    scheme: "period",
    network: "eip155:196",
    asset: TOKEN,
    amount: "5000000",
    payTo: MERCHANT,
    maxTimeoutSeconds: 60,
    extra: makeExtra(),
    ...over,
  };
}

// ── payload round-trip ─────────────────────────────────────────────────
test("payload: encode → decode round-trip preserves all fields", () => {
  const req = makeRequirements();
  const encoded = encodePaymentPayload({
    selected: req,
    permitSingle: {
      details: { token: TOKEN, amount: "60000000", expiration: 1_900_000_000, nonce: 5 },
      spender: SUBSCRIPTION_CONTRACT,
      sigDeadline: "1900000600",
    },
    permitSingleSignature: "0xpermit",
    terms: {
      payer: PAYER,
      merchant: MERCHANT,
      facilitator: FACILITATOR,
      token: TOKEN,
      amountPerPeriod: "5000000",
      periodSec: 2_592_000,
      maxPeriods: 12,
      startAt: 0,
      initialChargePeriods: 0,
      initialChargeAmount: "0",
      termsDeadline: 1_900_000_000,
      permitHash: ZERO_BYTES32,
      salt: ZERO_BYTES32,
      planTier: 2,
      changeFromSubId: ZERO_BYTES32,
      changeEffectiveAt: 0,
    },
    termsSignature: "0xterms",
  });
  const decoded = decodePaymentPayload(encoded);
  eq(decoded.x402Version, 2, "x402Version");
  deepEq(decoded.accepted, req, "selected requirements");
  const inner = asSubscriptionPaymentInner(decoded);
  eq(inner.permitSingleSignature, "0xpermit", "permitSig");
  eq(inner.termsSignature, "0xterms", "termsSig");
});

test("parsePaymentRequired: rejects malformed base64 / missing accepts", () => {
  expectThrows(() => parsePaymentRequired("not-base64-zzz"), "", "malformed");
  const bad = Buffer.from(JSON.stringify({ x402Version: 2 }), "utf8").toString("base64");
  expectThrows(() => parsePaymentRequired(bad), "accepts", "missing accepts");
});

// ── accessProof ─────────────────────────────────────────────────────────
test("buildAccessProofMessage: 32-byte hex; timestamp-sensitive", () => {
  const a = buildAccessProofMessage({
    subId: SUB_ID,
    payer: PAYER as `0x${string}`,
    timestamp: 1_700_000_000,
  });
  const b = buildAccessProofMessage({
    subId: SUB_ID,
    payer: PAYER as `0x${string}`,
    timestamp: 1_700_000_001,
  });
  assert(/^0x[0-9a-f]{64}$/.test(a), "hex");
  assert(a !== b, "timestamp differs");
});
test("encode/decodeAccessProof: round-trip + wrong kind rejected", () => {
  const p: AccessProof = {
    kind: "subscription-id",
    subId: SUB_ID,
    payer: PAYER,
    timestamp: 1_700_000_000,
    signature: "0xsig",
  };
  deepEq(decodeAccessProof(encodeAccessProof(p)), p, "round-trip");
  const bad = Buffer.from(JSON.stringify({ kind: "other" }), "utf8").toString("base64");
  expectThrows(() => decodeAccessProof(bad), "subscription-id", "wrong kind");
});

// ── typed-data ─────────────────────────────────────────────────────────
test("buildPermit2TypedData: PermitSingle shape, spender = subscription contract", () => {
  const req = makeRequirements();
  const td = buildPermit2TypedData({
    selected: req,
    nonce: 7,
    expiration: 1_900_000_000,
    sigDeadline: "1900000600",
  });
  eq(td.primaryType, "PermitSingle", "primaryType");
  eq(td.domain.name, "Permit2", "domain.name");
  eq(td.domain.chainId, 196, "chainId");
  eq(td.domain.version, undefined, "no version (Permit2 spec)");
  eq((td.message as any).spender, SUBSCRIPTION_CONTRACT, "spender = sub contract");
  // default amount = initialChargeAmount(0) + (maxPeriods - icp) * amountPerPeriod
  eq(
    (td.message as any).details.amount,
    (BigInt(5_000_000) * BigInt(12)).toString(),
    "default amount = remaining commitment",
  );
  eq((td.message as any).details.nonce, 7, "permit nonce (uint48)");
});

test("buildSubscriptionTermsTypedData: 16 fields (v2.0), changeEffectiveAt default", () => {
  const req = makeRequirements();
  const td = buildSubscriptionTermsTypedData({
    selected: req,
    payer: PAYER,
    startAt: 0,
    termsDeadline: 1_900_000_000,
    salt: ZERO_BYTES32,
    permitHash: ZERO_BYTES32,
  });
  eq(td.primaryType, "SubscriptionTerms", "primaryType");
  eq(td.message.planTier, 2, "planTier");
  eq(td.message.changeFromSubId, ZERO_BYTES32, "changeFromSubId default");
  eq(td.message.changeEffectiveAt, 0, "changeEffectiveAt default (none)");
  eq(td.message.termsDeadline, 1_900_000_000, "termsDeadline");
});

test("buildSubscriptionTermsTypedData: downgrade → changeEffectiveAt=2", () => {
  const req = makeRequirements({
    extra: makeExtra({
      initialCharge: { periodCount: 12, totalAmount: "192000000" },
    }),
  });
  const td = buildSubscriptionTermsTypedData({
    selected: req,
    payer: PAYER,
    startAt: 100,
    termsDeadline: 1_900_000_000,
    salt: ZERO_BYTES32,
    permitHash: ZERO_BYTES32,
    changeFrom: {
      fromSubId: SUB_ID,
      fromPlanId: ZERO_BYTES32,
      fromPlanTier: 1,
      direction: "downgrade",
      effectiveAt: "period_end",
    },
  });
  eq(td.message.initialChargePeriods, 12, "initialChargePeriods");
  eq(td.message.initialChargeAmount, "192000000", "initialChargeAmount");
  eq(td.message.changeFromSubId, SUB_ID, "changeFromSubId");
  eq(td.message.changeEffectiveAt, 2, "period_end → 2");
});

test("buildSubscriptionTermsTypedData: missing extra throws clearly", () => {
  const broken = makeRequirements();
  (broken as any).extra = {};
  expectThrows(
    () =>
      buildSubscriptionTermsTypedData({
        selected: broken,
        payer: PAYER,
        startAt: 0,
        termsDeadline: 1_900_000_000,
        salt: ZERO_BYTES32,
        permitHash: ZERO_BYTES32,
      }),
    "contracts/plan/domain",
    "missing extra",
  );
});

test("buildCancelAuthTypedData: 5 fields, action=0, payer=0 / merchant=1", () => {
  const base = {
    subId: SUB_ID,
    deadline: 1_900_000_000,
    nonce: ZERO_BYTES32,
    domain: DOMAIN,
  };
  const payerTd = buildCancelAuthTypedData({ ...base, initiator: "payer" });
  const merchantTd = buildCancelAuthTypedData({ ...base, initiator: "merchant" });
  eq(payerTd.primaryType, "CancelAuth", "primaryType");
  eq((payerTd.message as any).action, 0, "action=cancel_subscription");
  eq((payerTd.message as any).initiator, 0, "payer→0");
  eq((merchantTd.message as any).initiator, 1, "merchant→1");
});

test("parseChainIdFromNetwork: happy + invalid forms", () => {
  eq(parseChainIdFromNetwork("eip155:196"), 196, "eip155:196");
  expectThrows(() => parseChainIdFromNetwork("solana:101"), "eip155", "non-eip155");
  expectThrows(() => parseChainIdFromNetwork("eip155:abc"), "invalid", "non-numeric");
  expectThrows(() => parseChainIdFromNetwork("eip155:-1"), "invalid", "negative");
});

// ── InMemoryStore ──────────────────────────────────────────────────────
function makeSub(over: Partial<Subscription> = {}): Subscription {
  return {
    subId: SUB_ID,
    payer: PAYER,
    merchant: MERCHANT,
    facilitator: FACILITATOR,
    token: TOKEN,
    amountPerPeriod: "5000000",
    periodSec: 2_592_000,
    maxPeriods: 12,
    startAt: 0,
    initialChargePeriods: 0,
    initialChargeAmount: "0",
    state: "active",
    lastChargedPeriod: 0,
    totalPulled: "0",
    createdAt: 0,
    planId: "pro_monthly",
    planTier: 2,
    ...over,
  };
}

test("InMemoryStore: put → get round-trip", async () => {
  const s = new InMemoryStore();
  const sub = makeSub();
  await s.put(sub);
  const fetched = await s.get(sub.subId);
  deepEq(fetched, sub, "round-trip");
});

test("InMemoryStore: get returns null when missing", async () => {
  const s = new InMemoryStore();
  eq(await s.get("missing"), null, "null");
});

test("InMemoryStore: put is defensive (external mutate does not bleed)", async () => {
  const s = new InMemoryStore();
  const sub = makeSub();
  await s.put(sub);
  sub.state = "canceled";
  const fetched = await s.get(sub.subId);
  eq(fetched?.state, "active", "store not mutated by caller's reference");
});

test("InMemoryStore: delete removes entries", async () => {
  const s = new InMemoryStore();
  await s.put(makeSub({ subId: "0x1", payer: PAYER }));
  await s.put(makeSub({ subId: "0x2", payer: MERCHANT }));
  await s.delete("0x1");
  eq(await s.get("0x1"), null, "deleted");
  assert((await s.get("0x2")) !== null, "sibling still there");
});

// ── ChargeError + type-guard ───────────────────────────────────────────
test("ChargeError: code/subId/txHash preserved + message", () => {
  const e = new ChargeError(ChargeErrorCode.SubscriptionNotActive, "sub1", "0xtx");
  eq(e.code, ChargeErrorCode.SubscriptionNotActive, "code");
  eq(e.subId, "sub1", "subId");
  eq(e.txHash, "0xtx", "txHash");
  eq(e.name, "ChargeError", "name");
  assert(e.message.includes("sub1"), "message contains subId");
});

test("hasSubscriptionCapability: narrows to true only when both fields present", () => {
  assert(!hasSubscriptionCapability(null), "null");
  assert(!hasSubscriptionCapability({}), "empty");
  assert(!hasSubscriptionCapability({ verifyAccess: () => null }), "missing settlementMode");
  assert(
    !hasSubscriptionCapability({ verifyAccess: () => null, settlementMode: "post" }),
    "wrong settlementMode",
  );
  assert(
    hasSubscriptionCapability({ verifyAccess: () => null, settlementMode: "pre" }),
    "valid stub",
  );
});

// ── SubscriptionClient ─────────────────────────────────────────────────
import { SubscriptionClient } from "./client";
import type { SubscriptionCapability } from "./types";

/** Minimal SubscriptionCapability stub for client-level tests. */
function makeSchemeStub(over: Partial<SubscriptionCapability> = {}): SubscriptionCapability {
  const notImpl = async () => {
    throw new Error("scheme stub: not_implemented");
  };
  return {
    settlementMode: "pre",
    verifySubscribe: notImpl as any,
    settleSubscribe: notImpl as any,
    enrichAcceptsForChange: notImpl as any,
    verifyChange: notImpl as any,
    settleChange: notImpl as any,
    verifyCancel: notImpl as any,
    settleCancel: notImpl as any,
    verifyCancelPendingChange: notImpl as any,
    settleCancelPendingChange: notImpl as any,
    verifyAccess: notImpl as any,
    verifyOwnership: notImpl as any,
    charge: notImpl as any,
    getSubscription: notImpl as any,
    ...over,
  };
}

test("SubscriptionClient.charge delegates to scheme.charge", async () => {
  let called = 0;
  const scheme = makeSchemeStub({
    charge: async _id => {
      called++;
      return { success: true, period: 7, amount: "5000000" };
    },
  });
  const client = new SubscriptionClient({ scheme, store: new InMemoryStore() });
  const r = await client.charge(SUB_ID);
  eq(called, 1, "scheme.charge called once");
  eq(r.period, 7, "result passed through");
});

test("SubscriptionClient.getSubscription reads from store (not chain)", async () => {
  const store = new InMemoryStore();
  const sub = makeSub({ payer: PAYER });
  await store.put(sub);
  let chainCalled = 0;
  const scheme = makeSchemeStub({
    getSubscription: async _id => {
      chainCalled++;
      return null;
    },
  });
  const client = new SubscriptionClient({ scheme, store });
  const got = await client.getSubscription(sub.subId);
  eq(got?.subId, sub.subId, "store hit");
  eq(chainCalled, 0, "scheme.getSubscription NOT called (we read store)");
});

test("SubscriptionClient.syncFromChain: happy → store updated", async () => {
  const store = new InMemoryStore();
  const fresh = makeSub({ state: "active", lastChargedPeriod: 3 });
  const scheme = makeSchemeStub({
    getSubscription: async id => (id === fresh.subId ? fresh : null),
  });
  const client = new SubscriptionClient({ scheme, store });
  const r = await client.syncFromChain(fresh.subId);
  eq(r?.lastChargedPeriod, 3, "returned fresh");
  const stored = await store.get(fresh.subId);
  eq(stored?.lastChargedPeriod, 3, "store updated");
});

test("SubscriptionClient.syncFromChain: state=changed → fetches changedToSubId too", async () => {
  const store = new InMemoryStore();
  const NEW_ID = "0x" + "b".repeat(64);
  const oldSub = makeSub({ state: "changed", changedToSubId: NEW_ID });
  const newSub = makeSub({ subId: NEW_ID, planTier: 1, state: "active" });
  const scheme = makeSchemeStub({
    getSubscription: async id => (id === oldSub.subId ? oldSub : id === NEW_ID ? newSub : null),
  });
  const client = new SubscriptionClient({ scheme, store });
  await client.syncFromChain(oldSub.subId);
  const storedNew = await store.get(NEW_ID);
  eq(storedNew?.planTier, 1, "new sub fetched + stored");
});

test("SubscriptionClient.syncFromChain: scheme returns null → null + no store write", async () => {
  const store = new InMemoryStore();
  const scheme = makeSchemeStub({ getSubscription: async () => null });
  const client = new SubscriptionClient({ scheme, store });
  const r = await client.syncFromChain("0xunknown");
  eq(r, null, "null passthrough");
  eq(await store.get("0xunknown"), null, "store untouched");
});

test("SubscriptionClient.cancelBySeller: happy → verify+settle invoked", async () => {
  let verifyCalled = 0;
  let settleCalled = 0;
  const scheme = makeSchemeStub({
    verifyCancel: async () => {
      verifyCalled++;
      return { ok: true };
    },
    settleCancel: async () => {
      settleCalled++;
      return { success: true, subId: "0x1", headers: {} };
    },
  });
  const client = new SubscriptionClient({ scheme, store: new InMemoryStore() });
  const auth = {
    action: 0 as const,
    subId: "0x1",
    initiator: 1 as const,
    nonce: ZERO_BYTES32,
    deadline: 99_999_999_999,
    signature: "0xsig",
  };
  await client.cancelBySeller("0x1", auth, "tos_violation");
  eq(verifyCalled, 1, "verifyCancel called");
  eq(settleCalled, 1, "settleCancel called");
});

test("SubscriptionClient.cancelBySeller: verify fail → throws", async () => {
  const scheme = makeSchemeStub({
    verifyCancel: async () => ({ ok: false, error: "subscription_not_found" }),
  });
  const client = new SubscriptionClient({ scheme, store: new InMemoryStore() });
  const auth = {
    action: 0 as const,
    subId: "0x1",
    initiator: 1 as const,
    nonce: ZERO_BYTES32,
    deadline: 99_999_999_999,
    signature: "0xsig",
  };
  try {
    await client.cancelBySeller("0x1", auth);
    throw new Error("should have thrown");
  } catch (e) {
    assert((e as Error).message.includes("subscription_not_found"), "wraps verify error");
  }
});

async function main(): Promise<void> {
  let failed = 0;
  for (const t of tests) {
    try {
      await t.fn();
      console.log(`  ✓ ${t.name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${t.name}\n      ${(e as Error).message}`);
    }
  }
  console.log(`\n${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) process.exit(1);
}
void main();
