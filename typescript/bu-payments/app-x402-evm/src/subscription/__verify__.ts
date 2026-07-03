/* eslint-disable @typescript-eslint/no-explicit-any, no-console */
/**
 * Self-contained runtime checks for the evm subscription scheme.
 *
 * Run with:
 *   npx tsx src/subscription/__verify__.ts
 *
 * Covers:
 *   - AccessProofVerifier: happy + 4 reject paths (kind / window / bad sig / wrong payer)
 *   - PermitSubscriptionScheme.verifySubscribe:
 *       happy (real wallet sigs) + permit/terms sig invalid + hash mismatch
 *   - settleSubscribe (mock facilitator) — happy + facilitator error
 *   - verifyAccess (with store) — happy + payer_mismatch + insufficient_tier
 *   - getSubscription via mock GET
 */
import { privateKeyToAccount } from "viem/accounts";
import { keccak256, stringToHex, type Hex, type TypedDataDomain } from "viem";

import {
  buildAccessProofMessage,
  buildCancelAuthTypedData,
  buildPermit2TypedData,
  buildSubscriptionTermsTypedData,
  ChargeError,
  ChargeErrorCode,
  encodeAccessProof,
  encodePaymentPayload,
  InMemoryStore,
  ZERO_BYTES32,
  type AccessProof,
  type CancelAuth,
  type ChangeFromExtra,
  type SubscriptionRequirementsExtra,
  type Subscription,
} from "@okxweb3/app-x402-core/subscription";
import type { PaymentRequirements } from "@okxweb3/app-x402-core/types";
import { HTTPFacilitatorClient } from "@okxweb3/app-x402-core/server";

import { AccessProofVerifier } from "./access-verifier";
import { PermitSubscriptionScheme } from "./scheme";

type Test = { name: string; fn: () => Promise<void> | void };
const tests: Test[] = [];
function test(name: string, fn: () => Promise<void> | void): void {
  tests.push({ name, fn });
}
function eq<T>(a: T, b: T, msg: string): void {
  if (a !== b) throw new Error(`${msg}\n  actual: ${String(a)}\n  expected: ${String(b)}`);
}
function assert(c: unknown, msg: string): void {
  if (!c) throw new Error(msg);
}

// ── fixtures: deterministic test wallet ───────────────────────────────
const PAYER_KEY = "0x4af1bceebf7f3634ec3cff8a2c38e51178d5d4ce585c52d6043cfd640fcaecda" as Hex;
const account = privateKeyToAccount(PAYER_KEY);
const PAYER = account.address;
const MERCHANT = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" as Hex;
const TOKEN = "0xcccccccccccccccccccccccccccccccccccccccc" as Hex;
const SUBSCRIPTION_CONTRACT = "0xdddddddddddddddddddddddddddddddddddddddd" as Hex;
const PERMIT2_CONTRACT = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee" as Hex;
const FACILITATOR = "0xffffffffffffffffffffffffffffffffffffffff" as Hex;
const SUB_ID = `0x${"a".repeat(64)}` as Hex;
const DOMAIN: TypedDataDomain = {
  name: "A2APaySubscriptionV3",
  version: "1",
  chainId: 196,
  verifyingContract: "0x0000000000000000000000000000000000000099",
};
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

async function buildSignedPayload(
  requirements: PaymentRequirements,
): Promise<ReturnType<typeof encodePaymentPayload>> {
  const permitTd = buildPermit2TypedData({
    selected: requirements,
    nonce: 1,
    expiration: 1_900_000_000,
    sigDeadline: "1900000600",
  });
  const permitSignature = (await account.signTypedData({
    domain: permitTd.domain,
    types: permitTd.types as any,
    primaryType: permitTd.primaryType,
    message: permitTd.message as any,
  })) as Hex;

  const termsTd = buildSubscriptionTermsTypedData({
    selected: requirements,
    payer: PAYER,
    startAt: 0,
    termsDeadline: 1_900_000_000,
    salt: ZERO_BYTES32,
    permitHash: ZERO_BYTES32,
  });
  const termsSignature = (await account.signTypedData({
    domain: termsTd.domain,
    types: termsTd.types as any,
    primaryType: termsTd.primaryType,
    message: termsTd.message as any,
  })) as Hex;

  return encodePaymentPayload({
    selected: requirements,
    permitSingle: {
      details: { token: TOKEN, amount: "60000000", expiration: 1_900_000_000, nonce: 1 },
      spender: SUBSCRIPTION_CONTRACT,
      sigDeadline: "1900000600",
    },
    permitSingleSignature: permitSignature,
    terms: termsTd.message,
    termsSignature,
  });
}

function makeMockFetch(behavior: {
  subscribe?: (body: any) => Promise<{ status?: number; json: any }>;
  change?: (body: any) => Promise<{ status?: number; json: any }>;
  cancel?: (subId: string, body: any) => Promise<{ status?: number; json: any }>;
  charge?: (subId: string, body: any) => Promise<{ status?: number; json: any }>;
  getSubscription?: (subId: string) => Promise<{ status?: number; json: any }>;
}): typeof fetch {
  return (async (url: string | URL, init?: RequestInit) => {
    const u = url.toString();
    const method = init?.method ?? "GET";
    let result: { status?: number; json: any };
    // subId is in body (POST) or query (GET).
    const detailGet = u.match(/\/api\/v6\/pay\/x402\/subscriptions\/detail\?subId=([^&]+)/);
    if (u.endsWith("/supported") && method === "GET") {
      // `kind.extra.facilitatorAddress` carries the per-request EOA
      // (load-balanced); `signers[network]` is the full pool. Scheme reads
      // kind.extra.facilitatorAddress.
      return new Response(
        JSON.stringify({
          kinds: [
            {
              x402Version: 2,
              scheme: "period",
              network: "eip155:196",
              extra: {
                facilitatorAddress: FACILITATOR,
                subscriptionContract: SUBSCRIPTION_CONTRACT,
                permit2Contract: PERMIT2_CONTRACT,
              },
            },
          ],
          extensions: [],
          signers: { "eip155:196": [FACILITATOR] },
        }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      );
    } else if (u.endsWith("/api/v6/pay/x402/subscriptions") && method === "POST") {
      const body = init?.body ? JSON.parse(init.body as string) : {};
      result = await (behavior.subscribe?.(body) ?? Promise.resolve({ json: { code: 0 } }));
    } else if (u.endsWith("/api/v6/pay/x402/subscriptions/change") && method === "POST") {
      const body = init?.body ? JSON.parse(init.body as string) : {};
      result = await (behavior.change?.(body) ?? Promise.resolve({ json: { code: 0 } }));
    } else if (u.endsWith("/api/v6/pay/x402/subscriptions/cancel") && method === "POST") {
      const body = init?.body ? JSON.parse(init.body as string) : {};
      result = await (behavior.cancel?.(body.subId, body) ??
        Promise.resolve({ json: { code: 0 } }));
    } else if (u.endsWith("/api/v6/pay/x402/subscriptions/charge") && method === "POST") {
      const body = init?.body ? JSON.parse(init.body as string) : {};
      result = await (behavior.charge?.(body.subId, body) ??
        Promise.resolve({ json: { code: 0 } }));
    } else if (detailGet && method === "GET") {
      const subId = decodeURIComponent(detailGet[1]);
      // Default "settled" snapshot — pollUntilSettled checks
      // `lastChargedPeriod >= elapsedPeriods`. Tests that need a specific
      // shape (e.g. charge bumping period) supply their own mock.
      const defaultDetail = {
        code: 0,
        data: {
          subId,
          state: 1,
          payer: PAYER,
          merchant: MERCHANT,
          token: TOKEN,
          amountPerPeriod: "5000000",
          periodMode: 0,
          periodSec: 2_592_000,
          maxPeriods: 12,
          startAt: 0,
          lastChargedPeriod: 1,
          totalPulled: "5000000",
          elapsedPeriods: 1,
          currentPeriod: 1,
          planId: "pro_monthly",
          planTier: 2,
        },
      };
      result = await (behavior.getSubscription?.(subId) ??
        Promise.resolve({ json: defaultDetail }));
    } else {
      result = { status: 404, json: { code: "404", msg: `mock fetch: no handler for ${u}` } };
    }
    return new Response(JSON.stringify(result.json), {
      status: result.status ?? 200,
      headers: { "Content-Type": "application/json" },
    });
  }) as unknown as typeof fetch;
}

function makeScheme(over: Partial<{ fetchFn: typeof fetch; store: InMemoryStore }> = {}) {
  const scheme = new PermitSubscriptionScheme({
    facilitator: new HTTPFacilitatorClient({
      url: "https://facilitator.test",
      fetchFn: over.fetchFn ?? makeMockFetch({}),
    }),
    network: "eip155:196",
    store: over.store ?? new InMemoryStore(),
  });
  primeSchemeSnapshot(scheme);
  return scheme;
}

/** Inject a supportedKind into the scheme so synchronous accessors work. */
function primeSchemeSnapshot(scheme: PermitSubscriptionScheme): void {
  void scheme.enhancePaymentRequirements(
    makeRequirements(),
    {
      x402Version: 2,
      scheme: "period",
      network: "eip155:196",
      extra: {
        facilitatorAddress: FACILITATOR,
        subscriptionContract: SUBSCRIPTION_CONTRACT,
        permit2Contract: PERMIT2_CONTRACT,
      },
    },
    [],
  );
}

// ── AccessProofVerifier ────────────────────────────────────────────────
async function signAccessProof(timestamp: number, subId: Hex = SUB_ID): Promise<AccessProof> {
  const msg = buildAccessProofMessage({ subId, payer: PAYER as Hex, timestamp });
  const signature = await account.signMessage({ message: { raw: msg } });
  return { kind: "subscription-id", subId, payer: PAYER, timestamp, signature };
}

test("AccessProofVerifier: real signature → ok", async () => {
  const v = new AccessProofVerifier();
  const now = Math.floor(Date.now() / 1000);
  const p = await signAccessProof(now);
  const r = await v.verify(p);
  assert(r.ok, "must verify");
});
test("AccessProofVerifier: invalid kind", async () => {
  const v = new AccessProofVerifier();
  const r = await v.verify({
    ...(await signAccessProof(Math.floor(Date.now() / 1000))),
    kind: "other" as any,
  });
  assert(!r.ok && r.error === "invalid_proof_kind", "wrong kind");
});
test("AccessProofVerifier: timestamp out of window", async () => {
  const v = new AccessProofVerifier({ windowSec: 5 });
  const p = await signAccessProof(Math.floor(Date.now() / 1000) - 60);
  const r = await v.verify(p);
  assert(!r.ok && r.error === "timestamp_out_of_window", "stale ts");
});
test("AccessProofVerifier: bad signature", async () => {
  const v = new AccessProofVerifier();
  const now = Math.floor(Date.now() / 1000);
  const p = { ...(await signAccessProof(now)), signature: ("0x" + "00".repeat(65)) as Hex };
  const r = await v.verify(p);
  assert(!r.ok, "bad sig must fail");
});
test("AccessProofVerifier: signer != claimed payer", async () => {
  const v = new AccessProofVerifier();
  const now = Math.floor(Date.now() / 1000);
  const real = await signAccessProof(now);
  const tampered = { ...real, payer: MERCHANT };
  const r = await v.verify(tampered);
  assert(!r.ok, "mismatch must fail");
});

// ── verifySubscribe ───────────────────────────────────────────────────
test("verifySubscribe: real signatures → ok", async () => {
  const scheme = makeScheme();
  const req = makeRequirements();
  const headerB64 = await buildSignedPayload(req);
  const payload = JSON.parse(Buffer.from(headerB64, "base64").toString("utf8"));
  const r = await scheme.verifySubscribe(payload, req);
  assert(r.ok, `must verify; got: ${JSON.stringify(r)}`);
});

test("verifySubscribe: bad signatures pass SDK (contract is authoritative verifier)", async () => {
  // SDK explicitly does NOT ecrecover permit/terms signatures off-chain —
  // that's the contract's job at settle time. Forged signatures still pass
  // the SDK's verifySubscribe; only at settleSubscribe → facilitator →
  // contract revert do they fail. This test documents that contract.
  const scheme = makeScheme();
  const req = makeRequirements();
  const headerB64 = await buildSignedPayload(req);
  const payload = JSON.parse(Buffer.from(headerB64, "base64").toString("utf8"));
  payload.payload.permitSingleSignature = "0x" + "00".repeat(65);
  payload.payload.termsSignature = "0x" + "00".repeat(65);
  const r = await scheme.verifySubscribe(payload, req);
  assert(r.ok, `SDK should not reject on signature contents alone; got: ${JSON.stringify(r)}`);
});

test("verifySubscribe: missing inner fields → terms_binding_invalid", async () => {
  const scheme = makeScheme();
  const req = makeRequirements();
  const headerB64 = await buildSignedPayload(req);
  const payload = JSON.parse(Buffer.from(headerB64, "base64").toString("utf8"));
  delete payload.payload.permitSingle;
  const r = await scheme.verifySubscribe(payload, req);
  assert(
    !r.ok && (r as any).error === "terms_binding_invalid",
    `expected terms_binding_invalid; got ${JSON.stringify(r)}`,
  );
});

test("verifySubscribe: unrelated terms field mutations pass SDK shape-narrow", async () => {
  // Signature validity is contract-enforced; the SDK only shape-narrows and
  // performs field-level bind checks against PaymentRequirements. Mutating
  // fields not covered by that bind must not trip SDK-side rejection.
  const scheme = makeScheme();
  const req = makeRequirements();
  const headerB64 = await buildSignedPayload(req);
  const payload = JSON.parse(Buffer.from(headerB64, "base64").toString("utf8"));
  payload.payload.terms.salt = "0x" + "ff".repeat(32);
  const r = await scheme.verifySubscribe(payload, req);
  assert(r.ok, `SDK shape-narrow only; got ${JSON.stringify(r)}`);
});

// ── settleSubscribe (mock facilitator) ────────────────────────────────
test("settleSubscribe: happy → store.put + headers", async () => {
  const store = new InMemoryStore();
  const scheme = makeScheme({
    store,
    fetchFn: makeMockFetch({
      subscribe: async () => ({
        json: {
          code: 0,
          data: {
            subId: SUB_ID,
            payer: PAYER,
            merchant: MERCHANT,
            facilitator: FACILITATOR,
            token: TOKEN,
            amountPerPeriod: "5000000",
            periodSec: 2_592_000,
            maxPeriods: 12,
            state: "active",
          },
        },
      }),
    }),
  });
  const req = makeRequirements();
  const headerB64 = await buildSignedPayload(req);
  const payload = JSON.parse(Buffer.from(headerB64, "base64").toString("utf8"));
  const r = await scheme.settleSubscribe(payload, req);
  assert(r.success, `must succeed; got ${JSON.stringify(r)}`);
  if (!r.success) return;
  eq(r.subId, SUB_ID, "subId");
  assert(r.headers["PAYMENT-RESPONSE"], "response header set");
  const stored = await store.get(SUB_ID);
  assert(stored !== null && stored.state === "active", "store has sub");
});

test("settleSubscribe: facilitator code != 0 → success:false", async () => {
  const scheme = makeScheme({
    fetchFn: makeMockFetch({
      subscribe: async () => ({ json: { code: "5", msg: "bad" } }),
    }),
  });
  const req = makeRequirements();
  const headerB64 = await buildSignedPayload(req);
  const payload = JSON.parse(Buffer.from(headerB64, "base64").toString("utf8"));
  const r = await scheme.settleSubscribe(payload, req);
  assert(!r.success, "must fail");
});

// ── verifyAccess ──────────────────────────────────────────────────────
test("verifyAccess: happy + tier ok", async () => {
  const store = new InMemoryStore();
  const sub: Subscription = {
    subId: SUB_ID,
    payer: PAYER,
    merchant: MERCHANT,
    token: TOKEN,
    amountPerPeriod: "5000000",
    periodMode: 0,
    periodSec: 2_592_000,
    maxPeriods: 12,
    // startAt set to a past timestamp so currentCalculatePeriod >= 1;
    // combined with lastChargedPeriod=1, the local math passes without
    // hitting GET.
    startAt: 1_000_000_000,
    state: "active",
    lastChargedPeriod: 100,
    totalPulled: "5000000",
    planId: "pro_monthly",
    planTier: 2,
  };
  await store.put(sub);
  const scheme = makeScheme({ store });
  const proof = await signAccessProof(Math.floor(Date.now() / 1000));
  const r = await scheme.verifyAccess(proof, { acceptedPlanIds: ["pro_monthly"] });
  assert(r.ok, `must allow; got ${JSON.stringify(r)}`);
});

test("verifyAccess: payer_mismatch when claimed payer != stored sub.payer", async () => {
  const store = new InMemoryStore();
  await store.put({
    subId: SUB_ID,
    payer: MERCHANT,
    merchant: MERCHANT,
    token: TOKEN,
    amountPerPeriod: "5000000",
    periodMode: 0,
    periodSec: 2_592_000,
    maxPeriods: 12,
    startAt: 0,
    state: "active",
    lastChargedPeriod: 0,
    totalPulled: "0",
    planId: "pro_monthly",
    planTier: 2,
  });
  const scheme = makeScheme({ store });
  const proof = await signAccessProof(Math.floor(Date.now() / 1000));
  const r = await scheme.verifyAccess(proof, {});
  assert(!r.ok && (r as any).error === "payer_mismatch", "expected payer_mismatch");
});

test("verifyAccess: planId not in route's acceptedPlanIds → subscription_not_active", async () => {
  const store = new InMemoryStore();
  await store.put({
    subId: SUB_ID,
    payer: PAYER,
    merchant: MERCHANT,
    token: TOKEN,
    amountPerPeriod: "5000000",
    periodMode: 0,
    periodSec: 2_592_000,
    maxPeriods: 12,
    startAt: 0,
    state: "active",
    lastChargedPeriod: 0,
    totalPulled: "0",
    planId: "basic_monthly",
    planTier: 1,
  });
  const scheme = makeScheme({ store });
  const proof = await signAccessProof(Math.floor(Date.now() / 1000));
  // Route only accepts pro/enterprise plans → basic_monthly is rejected.
  const r = await scheme.verifyAccess(proof, {
    acceptedPlanIds: ["pro_monthly", "enterprise_monthly"],
  });
  assert(
    !r.ok && (r as any).error === "subscription_not_active",
    "plan not in allowlist → subscription_not_active",
  );
});

test("verifyAccess: no record → subscription_not_active", async () => {
  const scheme = makeScheme();
  const proof = await signAccessProof(Math.floor(Date.now() / 1000));
  const r = await scheme.verifyAccess(proof, {});
  assert(!r.ok && (r as any).error === "subscription_not_active", "subscription_not_active");
});

test("verifyAccess: pre-start (startAt in future) → subscription_not_yet_active", async () => {
  const store = new InMemoryStore();
  await store.put({
    subId: SUB_ID,
    payer: PAYER,
    merchant: MERCHANT,
    token: TOKEN,
    amountPerPeriod: "5000000",
    periodMode: 0,
    periodSec: 2_592_000,
    maxPeriods: 12,
    // Future startAt → computeElapsedPeriods returns 0 → pre-start reject.
    startAt: Math.floor(Date.now() / 1000) + 86_400,
    state: "active",
    lastChargedPeriod: 0,
    totalPulled: "0",
    planId: "pro_monthly",
    planTier: 2,
  });
  const scheme = makeScheme({ store });
  const proof = await signAccessProof(Math.floor(Date.now() / 1000));
  const r = await scheme.verifyAccess(proof, {});
  assert(
    !r.ok && (r as any).error === "subscription_not_yet_active",
    `expected subscription_not_yet_active; got ${JSON.stringify(r)}`,
  );
});

test("verifyAccess: local behind → fetches GET, allows when remote caught up", async () => {
  const store = new InMemoryStore();
  await store.put({
    subId: SUB_ID,
    payer: PAYER,
    merchant: MERCHANT,
    token: TOKEN,
    amountPerPeriod: "5000000",
    periodMode: 0,
    periodSec: 60,
    maxPeriods: 60,
    // startAt long ago → currentCalculatePeriod is huge; lastChargedPeriod=1
    // is well behind → SDK falls through to GET, which returns settled.
    startAt: 1_000_000_000,
    state: "active",
    lastChargedPeriod: 1,
    totalPulled: "5000000",
    planId: "pro_monthly",
    planTier: 2,
  });
  const scheme = makeScheme({ store });
  const proof = await signAccessProof(Math.floor(Date.now() / 1000));
  const r = await scheme.verifyAccess(proof, {});
  assert(r.ok, `must allow after GET refresh; got ${JSON.stringify(r)}`);
});

// ── getSubscription ──────────────────────────────────────────────────
test("getSubscription: returns parsed Subscription on facilitator code=0", async () => {
  const scheme = makeScheme({
    fetchFn: makeMockFetch({
      getSubscription: async id => ({
        json: {
          code: 0,
          data: {
            subId: id,
            payer: PAYER,
            merchant: MERCHANT,
            state: "active",
            planTier: 2,
          },
        },
      }),
    }),
  });
  const sub = await scheme.getSubscription(SUB_ID);
  assert(sub !== null, "got sub");
  eq(sub!.subId, SUB_ID, "subId");
});

test("getSubscription: returns null on facilitator code!=0", async () => {
  const scheme = makeScheme({
    fetchFn: makeMockFetch({ getSubscription: async () => ({ json: { code: "1" } }) }),
  });
  eq(await scheme.getSubscription(SUB_ID), null, "null on err");
});

// ── charge ────────────────────────────────────────────────────────────
async function seedSub(store: InMemoryStore, over: Partial<Subscription> = {}): Promise<void> {
  await store.put({
    subId: SUB_ID,
    payer: PAYER,
    merchant: MERCHANT,
    token: TOKEN,
    amountPerPeriod: "5000000",
    periodMode: 0,
    periodSec: 2_592_000,
    maxPeriods: 12,
    startAt: 0,
    state: "active",
    lastChargedPeriod: 0,
    totalPulled: "0",
    planId: "pro_monthly",
    planTier: 2,
    ...over,
  });
}

test("charge: happy → store bumps lastChargedPeriod + totalPulled", async () => {
  const store = new InMemoryStore();
  await seedSub(store, { lastChargedPeriod: 0, totalPulled: "0" });
  const scheme = makeScheme({
    store,
    fetchFn: makeMockFetch({
      charge: async () => ({
        json: { code: 0, data: { period: 1, amount: "5000000", state: "active" } },
      }),
    }),
  });
  const r = await scheme.charge(SUB_ID);
  eq(r.success, true, "success");
  eq(r.period, 1, "period");
  const updated = await store.get(SUB_ID);
  eq(updated?.lastChargedPeriod, 1, "lastChargedPeriod");
  eq(updated?.totalPulled, "5000000", "totalPulled accrued");
});

test("charge: planChangeTriggered → double put (old=changed, new=fetched)", async () => {
  const store = new InMemoryStore();
  const NEW_SUB_ID = `0x${"b".repeat(64)}`;
  await seedSub(store, { lastChargedPeriod: 5 });
  const scheme = makeScheme({
    store,
    fetchFn: makeMockFetch({
      charge: async () => ({
        json: {
          code: 0,
          data: {
            period: 6,
            amount: "5000000",
            state: "active",
            planChangeTriggered: true,
            newSubId: NEW_SUB_ID,
          },
        },
      }),
      // SDK polls newSubId until settled, then pulls oldSubId once to
      // capture the CHANGED state — mock branches on id.
      getSubscription: async id => {
        const base = {
          payer: PAYER,
          merchant: MERCHANT,
          token: TOKEN,
          amountPerPeriod: "5000000",
          periodMode: 0,
          periodSec: 2_592_000,
          maxPeriods: 12,
          startAt: 0,
          totalPulled: "5000000",
        };
        if (id === NEW_SUB_ID) {
          return {
            json: {
              code: 0,
              data: {
                ...base,
                subId: id,
                state: 1,
                lastChargedPeriod: 1,
                elapsedPeriods: 1,
                currentPeriod: 1,
                planId: "",
                planTier: 1,
              },
            },
          };
        }
        return {
          json: {
            code: 0,
            data: {
              ...base,
              subId: id,
              state: 4,
              lastChargedPeriod: 5,
              elapsedPeriods: 6,
              currentPeriod: 6,
              changedToSubId: NEW_SUB_ID,
              planId: "pro_monthly",
              planTier: 2,
            },
          },
        };
      },
    }),
  });
  const r = await scheme.charge(SUB_ID);
  assert(r.planChangeTriggered, "planChangeTriggered surfaced");
  eq(r.newSubId, NEW_SUB_ID, "newSubId surfaced");
  const oldSub = await store.get(SUB_ID);
  eq(oldSub?.state, "changed", "old sub state=changed");
  eq(oldSub?.changedToSubId, NEW_SUB_ID, "changedToSubId points to new");
  const newSub = await store.get(NEW_SUB_ID);
  assert(newSub !== null, "new sub persisted");
  eq(newSub?.planTier, 1, "planTier read from facilitator");
  eq(newSub?.planId, "", "planId empty (seller maps locally for the new sub)");
});

const CHARGE_REASONS: Array<[string, string]> = [
  ["period_not_due", "PeriodNotDue"],
  ["subscription_not_active", "SubscriptionNotActive"],
  ["insufficient_balance", "InsufficientBalance"],
  ["allowance_expired", "AllowanceExpired"],
  ["unauthorized_caller", "UnauthorizedCaller"],
  ["confirmation_timeout", "ConfirmationTimeout"],
];

for (const [reason, expectedCode] of CHARGE_REASONS) {
  test(`charge: facilitator reason "${reason}" → ChargeError(${expectedCode})`, async () => {
    const scheme = makeScheme({
      fetchFn: makeMockFetch({
        charge: async () => ({ json: { code: "1", msg: reason } }),
      }),
    });
    try {
      await scheme.charge(SUB_ID);
      throw new Error("should have thrown");
    } catch (e) {
      if (!(e instanceof ChargeError)) throw new Error(`expected ChargeError; got ${e}`);
      eq(e.code, reason, `code mapped from "${reason}"`);
      eq(e.subId, SUB_ID, "subId carried");
    }
  });
}

test("charge: unknown reason → fallback ConfirmationTimeout", async () => {
  const scheme = makeScheme({
    fetchFn: makeMockFetch({
      charge: async () => ({ json: { code: "1", msg: "facilitator_lol_unknown" } }),
    }),
  });
  try {
    await scheme.charge(SUB_ID);
    throw new Error("should have thrown");
  } catch (e) {
    if (!(e instanceof ChargeError)) throw new Error(`expected ChargeError; got ${e}`);
    eq(e.code, ChargeErrorCode.ConfirmationTimeout, "fallback");
  }
});

test("charge: network error (fetch throws) → ChargeError(ConfirmationTimeout)", async () => {
  const scheme = makeScheme({
    fetchFn: (async () => {
      throw new Error("ECONNRESET");
    }) as unknown as typeof fetch,
  });
  try {
    await scheme.charge(SUB_ID);
    throw new Error("should have thrown");
  } catch (e) {
    if (!(e instanceof ChargeError)) throw new Error(`expected ChargeError; got ${e}`);
    eq(e.code, ChargeErrorCode.ConfirmationTimeout, "network → ConfirmationTimeout");
  }
});

// ── Change flow: enrichAcceptsForChange + verifyChange / settleChange ─
test("enrichAcceptsForChange: unknown subId → null", async () => {
  const scheme = makeScheme();
  const r = await scheme.enrichAcceptsForChange([makeRequirements()], "0xunknown");
  eq(r, null, "null on unknown sub");
});

test("enrichAcceptsForChange: upgrade → direction:upgrade, effectiveAt:immediate", async () => {
  const store = new InMemoryStore();
  await seedSub(store, { planTier: 1 });
  const scheme = makeScheme({ store });
  // makeRequirements default extra.plan.tier=2 → upgrade vs old tier=1
  const out = await scheme.enrichAcceptsForChange([makeRequirements()], SUB_ID);
  assert(out !== null && out.length === 1, "got 1 enriched accept");
  const extra = out![0].extra as SubscriptionRequirementsExtra;
  eq(extra.changeFrom?.direction, "upgrade", "upgrade");
  eq(extra.changeFrom?.effectiveAt, "immediate", "immediate");
});

test("enrichAcceptsForChange: same-tier accepts dropped", async () => {
  const store = new InMemoryStore();
  await seedSub(store, { planTier: 2 }); // same as default makeRequirements.plan.tier
  const scheme = makeScheme({ store });
  const out = await scheme.enrichAcceptsForChange([makeRequirements()], SUB_ID);
  eq(out?.length, 0, "0 accepts after filtering same-tier");
});

test("enrichAcceptsForChange: downgrade → effectiveAt:period_end", async () => {
  const store = new InMemoryStore();
  await seedSub(store, { planTier: 3 });
  const scheme = makeScheme({ store });
  const out = await scheme.enrichAcceptsForChange([makeRequirements()], SUB_ID);
  assert(out !== null && out.length === 1, "got accept");
  const extra = out![0].extra as SubscriptionRequirementsExtra;
  eq(extra.changeFrom?.direction, "downgrade", "downgrade");
  eq(extra.changeFrom?.effectiveAt, "period_end", "period_end");
});

async function buildSignedChangePayload(
  scheme: PermitSubscriptionScheme,
  oldSubId: string,
): Promise<{ payload: any; requirements: any }> {
  const enriched = await scheme.enrichAcceptsForChange([makeRequirements()], oldSubId);
  if (!enriched || enriched.length === 0) throw new Error("enrich produced no accepts");
  const requirements = enriched[0];
  const extra = requirements.extra as SubscriptionRequirementsExtra;
  const changeFrom: ChangeFromExtra | undefined = extra.changeFrom;

  const permitTd = buildPermit2TypedData({
    selected: requirements,
    nonce: 1,
    expiration: 1_900_000_000,
    sigDeadline: "1900000600",
  });
  const permitSignature = (await account.signTypedData({
    domain: permitTd.domain,
    types: permitTd.types as any,
    primaryType: permitTd.primaryType,
    message: permitTd.message as any,
  })) as Hex;

  const termsTd = buildSubscriptionTermsTypedData({
    selected: requirements,
    payer: PAYER,
    startAt: 0,
    termsDeadline: 1_900_000_000,
    salt: ZERO_BYTES32,
    permitHash: ZERO_BYTES32,
    changeFrom,
  });
  const termsSignature = (await account.signTypedData({
    domain: termsTd.domain,
    types: termsTd.types as any,
    primaryType: termsTd.primaryType,
    message: termsTd.message as any,
  })) as Hex;

  const headerB64 = encodePaymentPayload({
    selected: requirements,
    permitSingle: {
      details: { token: TOKEN, amount: "60000000", expiration: 1_900_000_000, nonce: 1 },
      spender: SUBSCRIPTION_CONTRACT,
      sigDeadline: "1900000600",
    },
    permitSingleSignature: permitSignature,
    terms: termsTd.message,
    termsSignature,
  });
  return {
    payload: JSON.parse(Buffer.from(headerB64, "base64").toString("utf8")),
    requirements,
  };
}

test("verifyChange: upgrade happy → ok + direction:upgrade", async () => {
  const store = new InMemoryStore();
  await seedSub(store, { planTier: 1, planId: "basic_monthly" });
  const scheme = makeScheme({ store });
  const { payload, requirements } = await buildSignedChangePayload(scheme, SUB_ID);
  const r = await scheme.verifyChange(payload, requirements);
  assert(r.ok && r.direction === "upgrade", `verify ok: ${JSON.stringify(r)}`);
});

test("verifyChange: zero changeFromSubId → terms_binding_invalid", async () => {
  const store = new InMemoryStore();
  await seedSub(store, { planTier: 1 });
  const scheme = makeScheme({ store });
  const { payload, requirements } = await buildSignedChangePayload(scheme, SUB_ID);
  payload.payload.terms.changeFromSubId = "0x" + "0".repeat(64);
  const r = await scheme.verifyChange(payload, requirements);
  assert(!r.ok && (r as any).error === "terms_binding_invalid", "terms_binding_invalid");
});

test("verifyChange: SDK shape-narrows; credit checks happen on-chain", async () => {
  // Credit is contract-enforced (refused as `credit_amount_mismatch` on
  // settle). SDK is shape-narrow only.
  const store = new InMemoryStore();
  await seedSub(store, { planTier: 1, planId: "basic_monthly" });
  const scheme = makeScheme({ store });
  const { payload, requirements } = await buildSignedChangePayload(scheme, SUB_ID);
  const r = await scheme.verifyChange(payload, requirements);
  assert(r.ok, "SDK passes; contract is authoritative");
});

test("settleChange: upgrade → oldSub state=changed + newSub put", async () => {
  const store = new InMemoryStore();
  const NEW_ID = "0x" + "b".repeat(64);
  await seedSub(store, { planTier: 1, planId: "basic_monthly" });
  const scheme = makeScheme({
    store,
    fetchFn: makeMockFetch({
      change: async () => ({
        json: {
          code: 0,
          data: {
            newSubId: NEW_ID,
            operationType: "upgrade",
            subscription: { payer: PAYER, merchant: MERCHANT, planTier: 2, planId: "pro_monthly" },
          },
        },
      }),
    }),
  });
  const { payload, requirements } = await buildSignedChangePayload(scheme, SUB_ID);
  const r = await scheme.settleChange(payload, requirements);
  assert(r.success, `succeed: ${JSON.stringify(r)}`);
  if (!r.success) return;
  eq(r.operationType, "upgrade", "upgrade");
  const oldSub = await store.get(SUB_ID);
  eq(oldSub?.state, "changed", "old state=changed");
  eq(oldSub?.changedToSubId, NEW_ID, "linked");
  const newSub = await store.get(NEW_ID);
  eq(newSub?.planTier, 2, "new tier");
});

test("settleChange: downgrade → oldSub stays active + pendingPlanChange set", async () => {
  const store = new InMemoryStore();
  const NEW_ID = "0x" + "c".repeat(64);
  // Old tier 3 vs makeRequirements default tier=2 → downgrade.
  await seedSub(store, { planTier: 3, planId: "enterprise_monthly" });
  const scheme = makeScheme({
    store,
    fetchFn: makeMockFetch({
      change: async () => ({
        json: {
          code: 0,
          data: {
            newSubId: NEW_ID,
            operationType: "downgrade",
            scheduledFromPeriod: 4,
          },
        },
      }),
      // Downgrade: SDK pulls oldSub once via getDetailOnce — return a detail
      // with the facilitator-authoritative pendingPlanChange attached.
      getSubscription: async id => ({
        json: {
          code: 0,
          data: {
            subId: id,
            state: 1,
            payer: PAYER,
            merchant: MERCHANT,
            token: TOKEN,
            amountPerPeriod: "5000000",
            periodMode: 0,
            periodSec: 2_592_000,
            maxPeriods: 12,
            startAt: 0,
            lastChargedPeriod: 1,
            totalPulled: "5000000",
            elapsedPeriods: 1,
            currentPeriod: 1,
            planId: "enterprise_monthly",
            planTier: 3,
            pendingPlanChange: { subId: id, newSubId: NEW_ID, effectiveFromPeriod: 4, state: 0 },
          },
        },
      }),
    }),
  });
  const { payload, requirements } = await buildSignedChangePayload(scheme, SUB_ID);
  const r = await scheme.settleChange(payload, requirements);
  assert(r.success, `succeed: ${JSON.stringify(r)}`);
  const oldSub = await store.get(SUB_ID);
  eq(oldSub?.state, "active", "old stays active");
  assert(oldSub?.pendingPlanChange !== undefined, "pendingPlanChange set");
  // pendingPlanChange now comes from facilitator GET /detail (authoritative).
  eq(oldSub?.pendingPlanChange?.effectiveFromPeriod, 4, "effectiveFromPeriod from facilitator");
});

// ── verifyCancel / settleCancel ────────────────────────────────────────
async function signCancelAuth(
  initiator: 0 | 1,
  _initiatorAddr: string,
  subId: string,
  deadline: number,
): Promise<CancelAuth> {
  const td = buildCancelAuthTypedData({
    subId: subId as Hex,
    initiator: initiator === 0 ? "payer" : "merchant",
    deadline,
    nonce: ZERO_BYTES32,
    domain: DOMAIN,
  });
  const signature = (await account.signTypedData({
    domain: td.domain,
    types: td.types as any,
    primaryType: td.primaryType,
    message: td.message as any,
  })) as Hex;
  return { action: 0, subId, initiator, nonce: ZERO_BYTES32, deadline, signature };
}

test("verifyCancel: cross-subId auth → cancel_signature_invalid", async () => {
  // The only SDK-owned guard for cancel: prevents using sub A's auth to
  // cancel sub B at the URL level. Everything else (signature, deadline,
  // state) is delegated to the contract via settleCancel → facilitator.
  const scheme = makeScheme();
  const otherSubId = `0x${"d".repeat(64)}`;
  const auth = await signCancelAuth(0, PAYER, otherSubId, 1_900_000_000);
  const r = await scheme.verifyCancel(auth, SUB_ID);
  assert(!r.ok && (r as any).error === "cancel_signature_invalid", "cancel_signature_invalid");
});

test("verifyCancel: SDK passes regardless of sub existence / state / signature", async () => {
  // Contract is the authoritative verifier — SDK does not pre-check
  // signature / deadline / state. This test documents that contract:
  // a tampered auth with a non-existent sub still passes verifyCancel.
  const scheme = makeScheme();
  const auth = await signCancelAuth(0, PAYER, SUB_ID, 1_900_000_000);
  auth.signature = ("0x" + "00".repeat(65)) as Hex;
  const r = await scheme.verifyCancel(auth, SUB_ID);
  assert(r.ok, `SDK should not pre-validate; got: ${JSON.stringify(r)}`);
});

test("settleCancel: happy → store state=canceled", async () => {
  const store = new InMemoryStore();
  await seedSub(store, { payer: PAYER });
  const scheme = makeScheme({
    store,
    fetchFn: makeMockFetch({
      cancel: async () => ({ json: { code: 0, data: { subId: SUB_ID } } }),
    }),
  });
  const auth = await signCancelAuth(0, PAYER, SUB_ID, 1_900_000_000);
  const r = await scheme.settleCancel(auth, SUB_ID);
  assert(r.success, `succeed: ${JSON.stringify(r)}`);
  if (!r.success) return;
  eq(r.subId, SUB_ID, "subId echoed");
  const sub = await store.get(SUB_ID);
  eq(sub?.state, "canceled", "state");
});

test("settleCancel: facilitator code != 0 → success:false", async () => {
  const scheme = makeScheme({
    fetchFn: makeMockFetch({
      cancel: async () => ({ json: { code: "9", msg: "bad" } }),
    }),
  });
  const auth = await signCancelAuth(0, PAYER, SUB_ID, 1_900_000_000);
  const r = await scheme.settleCancel(auth, SUB_ID);
  assert(!r.success, "fail");
});

// ── unused vars guard for tsconfig strict ─────────────────────────────
void keccak256;
void stringToHex;
void encodeAccessProof;

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
