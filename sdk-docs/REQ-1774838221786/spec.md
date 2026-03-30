# Spec: x402 Payments SDK — Feature Development

## 1. Summary

**What problem is solved**: x402 eliminates account-and-credential friction from internet payments by embedding payment semantics directly in HTTP using the `402 Payment Required` status code. Any web service can require payment before serving a response with minimal code changes on both sides.

**Who the users are**:
- **SDK consumers (developers)** — server-side developers adding paid API endpoints; client-side developers (including AI agent authors) consuming paid APIs
- **Facilitator operators** — infrastructure providers running verify/settle services
- **AI agents** — autonomous programs that autonomously discover, pay for, and consume paid resources

**Current version**: x402 protocol v2 (TypeScript SDK `@x402/*` packages at `2.x`)

---

## 2. Scope

**In scope:**
- TypeScript SDK packages: `@x402/core`, `@x402/evm`, `@x402/svm`, `@x402/stellar`, `@x402/aptos`, `@x402/express`, `@x402/hono`, `@x402/next`, `@x402/fetch`, `@x402/axios`, `@x402/paywall`, `@x402/extensions`
- Protocol v2 with v1 backwards-compatibility shims
- Server middleware (Express, Hono, Next.js) and client wrappers (Fetch, Axios)
- Facilitator implementation (`x402Facilitator`)
- Payment mechanisms: `exact` (EIP-3009, Permit2, SPL, Stellar, Aptos), `upto` (EVM Permit2)
- Extensions: bazaar, sign-in-with-x, offer-receipt, payment-identifier, eip2612-gas-sponsoring, erc20-approval-gas-sponsoring
- Browser paywall UI component (`@x402/paywall`)
- Python (`x402`) and Go (`github.com/coinbase/x402/go`) SDKs (implementation parity reference)
- Dual CJS + ESM build output via `tsup`
- Changeset-based versioning

**Out of scope:**
- UI/UX redesign (SDK is headless; paywall is a reference implementation only)
- Fiat payment prioritization over onchain
- Proprietary lock-in of any kind
- Removing support for any existing network unless required for security

**Not changing:**
- HTTP transport semantics (402 status code, `PAYMENT-REQUIRED` / `PAYMENT-SIGNATURE` / `PAYMENT-RESPONSE` headers)
- v1 protocol backward compatibility layer
- Apache-2.0 open-source license

---

## 3. External inputs & status

| Input | Status | Notes |
|-------|--------|-------|
| PRD (`prd.md`) | PROVIDED (local, synthesized from repo docs) | No external Lark URL was provided; PRD was synthesized from existing repo documentation |
| Figma spec | NOT PROVIDED | SDK is headless; no UI spec required |
| api-doc.md | NOT PROVIDED | Protocol API is fully defined in `specs/` and source types |
| Other-end tech design | NOT PROVIDED | No cross-spec conflicts to resolve |

---

## 4. User flows

### 4.1 Server-side — protecting a resource (happy path)

```
Developer adds middleware
        │
        ▼
app.use(paymentMiddleware(routes, server))
        │
        ▼
Middleware calls server.initialize()
  → fetches SupportedKinds from facilitator /supported
        │
        ▼
Incoming HTTP request arrives (no payment header)
        │
        ▼
Middleware calls buildPaymentRequirements()
  → parsePrice() → enhancePaymentRequirements()
        │
        ▼
Returns HTTP 402 with PAYMENT-REQUIRED header
  (base64 JSON: PaymentRequired { x402Version, resource, accepts[] })
        │
        ▼
Client retries request with PAYMENT-SIGNATURE header
        │
        ▼
Middleware calls verifyPayment()
  → beforeVerify hooks → facilitator.verify() → afterVerify hooks
        │
   ┌────┴────┐
isValid?  invalid
   │          │
   ▼          ▼
settlePayment()  return 402 with error
  → beforeSettle → facilitator.settle() → afterSettle
        │
        ▼
Resource server responds 200 with PAYMENT-RESPONSE header
  (base64 JSON: SettleResponse { success, transaction, network })
```

**Bullet steps (happy path):**
1. Developer configures routes with `accepts` (scheme, network, price, payTo) and registers `paymentMiddleware`.
2. On startup, `x402ResourceServer.initialize()` calls the facilitator's `/supported` endpoint to cache `SupportedKind[]`.
3. Client makes an unauth'd request — middleware calls `buildPaymentRequirements()`, encodes `PaymentRequired` as base64 JSON into `PAYMENT-REQUIRED` header, returns HTTP 402.
4. Client decodes `PaymentRequired`, selects a `PaymentRequirements` entry it can satisfy, and calls `x402Client.createPaymentPayload()`.
5. Client re-sends request with `PAYMENT-SIGNATURE` header (base64 JSON `PaymentPayload`).
6. Middleware decodes `PaymentPayload`, runs `beforeVerify` hooks, calls `facilitatorClient.verify()`, runs `afterVerify` hooks.
7. If valid, middleware runs `beforeSettle` hooks, calls `facilitatorClient.settle()`, runs `afterSettle` hooks.
8. Middleware attaches `PAYMENT-RESPONSE` header with base64-encoded `SettleResponse`, passes request to the actual handler, which returns 200.

### 4.2 Client-side — consuming a paid resource (happy path)

```
Client calls wrapFetchWithPayment(fetch, x402Client)
        │
        ▼
Makes initial HTTP GET to paid endpoint
        │
response.status === 402?
   YES  │
        ▼
Decode PAYMENT-REQUIRED header
  → parse PaymentRequired
        │
        ▼
Run onPaymentRequired hooks (e.g., try session token first)
        │
        ▼
x402Client.selectPaymentRequirements()
  → filter by registered schemes
  → apply PaymentPolicy[] filters
  → selector(reqs) → chosen PaymentRequirements
        │
        ▼
x402Client.createPaymentPayload()
  → beforePaymentCreation hooks
  → schemeClient.createPaymentPayload()
  → enrich with client extensions
  → afterPaymentCreation hooks
        │
        ▼
Encode as PAYMENT-SIGNATURE header
Retry request
        │
        ▼
Receive 200 + PAYMENT-RESPONSE header
```

### 4.3 Facilitator — verify and settle (happy path)

```
POST /verify { paymentPayload, paymentRequirements }
        │
        ▼
x402Facilitator.verify()
  → find registered SchemeNetworkFacilitator
  → mechanism.verify(payload, requirements, context)
        │
        ▼
Return VerifyResponse { isValid, payer? }

POST /settle { paymentPayload, paymentRequirements }
        │
        ▼
x402Facilitator.settle()
  → mechanism.settle(payload, requirements, context)
  → submit tx to blockchain
  → wait for confirmation
        │
        ▼
Return SettleResponse { success, transaction, network, amount? }
```

### 4.4 Non-happy paths

- **No registered scheme on client**: `x402Client.selectPaymentRequirements()` throws `Error("No network/scheme registered …")` — wrapping library (fetch/axios) surfaces as thrown error to caller.
- **Policy filters all requirements**: throws `Error("All payment requirements were filtered out by policies …")`.
- **beforeVerify hook aborts**: returns `{ isValid: false, invalidReason, invalidMessage }` — middleware returns 402.
- **Verification failure**: `VerifyError` thrown with `statusCode`, `invalidReason`, `invalidMessage`; middleware returns 402 with error field.
- **Settlement failure**: `SettleError` thrown; `onSettleFailure` hooks run first; if no recovery, middleware returns 500.
- **Facilitator response malformed**: `FacilitatorResponseError` thrown; `getFacilitatorResponseError()` helper walks cause chain; Express middleware returns 502.
- **Facilitator unavailable on initialize()**: error thrown with cause — startup fails fast.
- **Multiple facilitators**: earlier entries get precedence; fallback iterates all if no specific support found.
- **PaymentPayload version mismatch**: `findMatchingRequirements()` throws `Error("Unsupported x402 version: …")`.
- **Partial settlement (upto scheme)**: `SettlementOverrides.amount` passed via `settlement-overrides` header or directly; `resolveSettlementOverrideAmount()` handles percent (`"50%"`), dollar (`"$0.05"`), and raw atomic-unit formats.
- **Paywall HTML response**: when client `Accept` header indicates browser, middleware returns 402 with HTML paywall body instead of JSON.

---

## 5. Architecture

### 5.1 Package dependency graph

```
@x402/core
  ├── client/x402Client          ← scheme registry, policy engine, payload creation
  ├── server/x402ResourceServer  ← resource protection, lifecycle hooks, settlement
  ├── facilitator/x402Facilitator← verify/settle coordination
  ├── http/x402HTTPClient        ← HTTP header encode/decode (client)
  ├── http/x402HTTPResourceServer← HTTP adapter, route config, paywall
  ├── types/                     ← Protocol types (PaymentRequired, PaymentPayload, …)
  └── schemas/                   ← Zod validators (parse/validate/isPayment*)

@x402/evm     → implements SchemeNetworkClient + SchemeNetworkServer + SchemeNetworkFacilitator (EIP-3009, Permit2)
@x402/svm     → implements for Solana SPL
@x402/stellar → implements for Stellar/Soroban
@x402/aptos   → implements for Aptos Fungible Assets

@x402/express → Express middleware wrapping @x402/core/server
@x402/hono    → Hono middleware wrapping @x402/core/server
@x402/next    → Next.js integration wrapping @x402/core/server
@x402/fetch   → wrapFetchWithPayment() wrapping @x402/core/client
@x402/axios   → Axios interceptor wrapping @x402/core/client
@x402/paywall → Browser paywall HTML/React component
@x402/extensions → bazaar, sign-in-with-x, offer-receipt, payment-identifier, gas sponsoring
```

### 5.2 Core design patterns

- **Builder pattern + method chaining**: `x402Client`, `x402ResourceServer`, `x402Facilitator` all return `this` from `register()`, `registerPolicy()`, `onBeforeVerify()` etc.
- **Strategy pattern**: `SchemeNetworkClient`, `SchemeNetworkServer`, `SchemeNetworkFacilitator` interfaces — concrete mechanism implementations plugged in at construction time.
- **Chain-of-responsibility**: lifecycle hooks (`beforeVerify`, `afterVerify`, `onVerifyFailure`, etc.) run sequentially; any hook can abort or recover.
- **Adapter pattern**: `HTTPAdapter` interface abstracts framework-specific request handling; each framework package provides a concrete adapter (e.g., `ExpressAdapter`).
- **Registry via nested Map**: `Map<version, Map<network, Map<scheme, impl>>>` used for O(1) scheme lookup; `findByNetworkAndScheme` utility handles wildcard network pattern matching (CAIP family patterns like `"eip155:*"`).
- **Zod schemas as the validation layer**: all wire formats validated via `src/schemas/index.ts`; `safeParse` / `parse` / type-guards exported for external consumers.
- **Dual build (CJS + ESM)**: `tsup` outputs `dist/cjs/` and `dist/esm/` with `.d.ts` and `.d.mts` types; `package.json` `exports` map routes entry points (`"."`, `"./client"`, `"./server"`, `"./types"`, etc.).

### 5.3 State management

SDK is stateless per-request. The only persistent in-process state:
- `x402ResourceServer.supportedResponsesMap` — cached `SupportedResponse` from facilitator, populated by `initialize()`.
- `x402ResourceServer.facilitatorClientsMap` — per-(version, network, scheme) facilitator client reference.
- `x402ResourceServer.registeredServerSchemes` / `x402Client.registeredClientSchemes` — scheme registries set up at construction time.
- `x402Facilitator.registeredFacilitatorSchemes` — facilitator mechanism registry.

No Redux, no Zustand, no React state — this is a pure SDK library.

### 5.4 Routing and transport

All transport is HTTP-header-based:
- `PAYMENT-REQUIRED` header → base64 JSON `PaymentRequired`
- `PAYMENT-SIGNATURE` header → base64 JSON `PaymentPayload`
- `PAYMENT-RESPONSE` header → base64 JSON `SettleResponse`

Framework packages expose a single entry point (`paymentMiddleware`, `wrapFetchWithPayment`, etc.) that developers compose into their existing routing.

### 5.5 Affected modules per development area

| Development area | Files / packages touched |
|-----------------|----------------------|
| New scheme (network mechanism) | `typescript/packages/mechanisms/<name>/src/` — implement `SchemeNetworkClient`, `SchemeNetworkServer`, `SchemeNetworkFacilitator`; add to `@x402/core` type union if needed |
| New HTTP framework adapter | `typescript/packages/http/<name>/src/` — implement `HTTPAdapter`, export middleware/wrapper |
| New extension | `typescript/packages/extensions/src/<name>/` — implement `ResourceServerExtension` and/or `ClientExtension`; register via `registerExtension()` |
| Core protocol change | `typescript/packages/core/src/types/`, `src/schemas/index.ts`, `src/server/x402ResourceServer.ts`, `src/client/x402Client.ts` |
| New facilitator hook | `typescript/packages/core/src/facilitator/x402Facilitator.ts` |

---

## 6. Data model

### 6.1 Core protocol types (`@x402/core/types`)

```typescript
// Primitives
type Network = `${string}:${string}`;   // CAIP-2 format, e.g. "eip155:8453"
type Money = string | number;
type AssetAmount = { asset: string; amount: string; extra?: Record<string, unknown> };
type Price = Money | AssetAmount;

// Wire types (v2)
type ResourceInfo = { url: string; description?: string; mimeType?: string };

type PaymentRequirements = {
  scheme: string;              // e.g. "exact"
  network: Network;
  asset: string;               // token contract address or identifier
  amount: string;              // atomic units (bigint-safe string)
  payTo: string;               // recipient address
  maxTimeoutSeconds: number;   // default 300
  extra: Record<string, unknown>;  // scheme-specific, e.g. { decimals: 6 }
};

type PaymentRequired = {       // returned in 402 response
  x402Version: number;
  error?: string;
  resource: ResourceInfo;
  accepts: PaymentRequirements[];
  extensions?: Record<string, unknown>;
};

type PaymentPayload = {        // sent in payment request
  x402Version: number;
  resource?: ResourceInfo;
  accepted: PaymentRequirements;
  payload: Record<string, unknown>;  // scheme-specific signed data
  extensions?: Record<string, unknown>;
};

// Facilitator wire types
type VerifyRequest  = { x402Version: number; paymentPayload: PaymentPayload; paymentRequirements: PaymentRequirements };
type VerifyResponse = { isValid: boolean; invalidReason?: string; invalidMessage?: string; payer?: string; extensions?: Record<string, unknown> };
type SettleRequest  = { x402Version: number; paymentPayload: PaymentPayload; paymentRequirements: PaymentRequirements };
type SettleResponse = { success: boolean; errorReason?: string; errorMessage?: string; payer?: string; transaction: string; network: Network; amount?: string; extensions?: Record<string, unknown> };
type SupportedKind  = { x402Version: number; scheme: string; network: Network; extra?: Record<string, unknown> };
type SupportedResponse = { kinds: SupportedKind[]; extensions: string[]; signers: Record<string, string[]> };
```

### 6.2 EVM mechanism types (`@x402/evm`)

```typescript
type AssetTransferMethod = "eip3009" | "permit2";

// exact/evm EIP-3009 payload
type ExactEIP3009Payload = {
  signature?: `0x${string}`;
  authorization: { from: `0x${string}`; to: `0x${string}`; value: string; validAfter: string; validBefore: string; nonce: `0x${string}` };
};

// exact/evm Permit2 payload
type ExactPermit2Payload = {
  signature: `0x${string}`;
  permit2Authorization: { from: `0x${string}`; permitted: { token: `0x${string}`; amount: string }; spender: `0x${string}`; nonce: string; deadline: string; witness: { to: `0x${string}`; validAfter: string } };
};

type ExactEvmPayloadV2 = ExactEIP3009Payload | ExactPermit2Payload;

// upto/evm Permit2 (adds facilitator field to witness)
type UptoPermit2Payload = {
  signature: `0x${string}`;
  permit2Authorization: { from: `0x${string}`; permitted: { token: `0x${string}`; amount: string }; spender: `0x${string}`; nonce: string; deadline: string; witness: { to: `0x${string}`; facilitator: `0x${string}`; validAfter: string } };
};
```

### 6.3 Configuration types

```typescript
// Server-side resource config
interface ResourceConfig {
  scheme: string;
  payTo: string | DynamicPayTo;
  price: Price | DynamicPrice;
  network: Network;
  maxTimeoutSeconds?: number;    // default: 300
  extra?: Record<string, unknown>;
}

// Client-side scheme registration
interface SchemeRegistration { network: Network; client: SchemeNetworkClient; x402Version?: number }
interface x402ClientConfig { schemes: SchemeRegistration[]; policies?: PaymentPolicy[]; paymentRequirementsSelector?: SelectPaymentRequirements }

// Settlement overrides (upto scheme)
interface SettlementOverrides {
  amount?: string;  // "1000" (atomic) | "50%" (percent of requirement) | "$0.05" (USD)
}
```

### 6.4 Zod schemas (validation layer, `@x402/core/schemas`)

All wire types have corresponding Zod schemas:
- `PaymentRequiredSchema` — discriminated union on `x402Version` (v1 | v2)
- `PaymentPayloadSchema` — discriminated union on `x402Version` (v1 | v2)
- `PaymentRequirementsSchema` — union of v1 and v2 shapes
- Helper functions: `parsePaymentRequired`, `validatePaymentRequired`, `isPaymentRequired`, and equivalents for each type and version

---

## 7. API / Service

### 7.1 Facilitator HTTP API (consumed by SDK)

| Endpoint | Method | Request body | Response body |
|----------|--------|-------------|---------------|
| `/supported` | GET | — | `SupportedResponse` |
| `/verify` | POST | `VerifyRequest` | `VerifyResponse` |
| `/settle` | POST | `SettleRequest` | `SettleResponse` |

TBC[1]: assumption=Coinbase CDP facilitator endpoint used as default in `HTTPFacilitatorClient` | impact=SDK may fail if endpoint changes | needs=read `typescript/packages/core/src/http/httpFacilitatorClient.ts` default URL constant.

### 7.2 HTTP header protocol

| Header | Direction | Encoding | Content |
|--------|-----------|----------|---------|
| `PAYMENT-REQUIRED` | Server → Client | base64 JSON | `PaymentRequired` |
| `PAYMENT-SIGNATURE` | Client → Server | base64 JSON | `PaymentPayload` |
| `PAYMENT-RESPONSE` | Server → Client | base64 JSON | `SettleResponse` |
| `settlement-overrides` | Internal (handler → middleware) | JSON string | `SettlementOverrides` |

### 7.3 Mechanism interfaces (implemented per chain)

```typescript
interface SchemeNetworkClient {
  readonly scheme: string;
  createPaymentPayload(x402Version: number, requirements: PaymentRequirements, context?: PaymentPayloadContext): Promise<PaymentPayloadResult>;
}

interface SchemeNetworkServer {
  readonly scheme: string;
  parsePrice(price: Price, network: Network): Promise<AssetAmount>;
  enhancePaymentRequirements(base: PaymentRequirements, supportedKind: SupportedKind & { x402Version: number }, facilitatorExtensions: string[]): Promise<PaymentRequirements>;
}

interface SchemeNetworkFacilitator {
  readonly scheme: string;
  readonly caipFamily: string;  // e.g. "eip155:*", "solana:*"
  getExtra(network: Network): Record<string, unknown> | undefined;
  getSigners(network: string): string[];
  verify(payload: PaymentPayload, requirements: PaymentRequirements, context?: FacilitatorContext): Promise<VerifyResponse>;
  settle(payload: PaymentPayload, requirements: PaymentRequirements, context?: FacilitatorContext): Promise<SettleResponse>;
}
```

### 7.4 Error types

| Error class | When thrown | Key fields |
|-------------|-------------|-----------|
| `VerifyError` | Verification rejected by facilitator | `statusCode`, `invalidReason`, `invalidMessage`, `payer` |
| `SettleError` | Settlement rejected by facilitator | `statusCode`, `errorReason`, `errorMessage`, `transaction`, `network` |
| `FacilitatorResponseError` | Malformed facilitator success payload | `message` |
| `RouteConfigurationError` | Bad route config detected on startup | TBC[2] |

### 7.5 Error handling strategy

- **Parsing errors**: throw exceptions immediately (fail-fast)
- **Validation errors**: return structured objects (`VerifyResponse.isValid=false`, `SettleResponse.success=false`)
- **Hook abort**: hooks return `{ abort: true, reason }` — never throw
- **Hook recovery**: failure hooks return `{ recovered: true, result }` — swallows original error
- **Retries**: no automatic retry in core SDK; framework adapters and AI clients implement retry policy
- **Multi-facilitator fallback**: `x402ResourceServer` iterates all registered facilitators on unknown scheme
- **Extension errors**: caught internally; `console.error` logged; extension skipped; request processing continues

---

## 8. UI / UX

This is a headless SDK. The only UI component is `@x402/paywall`.

### 8.1 Paywall component (`@x402/paywall`)

- Rendered as HTML string via `PaywallProvider.generateHtml(paymentRequired, config?)`
- Served by the resource server middleware when the incoming `Accept` header signals a browser
- Config options: `appName?: string`, `appLogo?: string`, `sessionTokenEndpoint?: string`, `currentUrl?: string`, `testnet?: boolean`
- Implements EVM and SVM wallet connection flows internally (React component `PaywallApp.tsx`)
- Source: `typescript/packages/http/paywall/src/`

### 8.2 Copy / i18n

TBC[3]: assumption=no i18n support in paywall (English only) | impact=non-English markets see English UI | needs=product decision on i18n requirement.

### 8.3 UI states

| State | Client type | Response |
|-------|-------------|----------|
| Payment required | API client | HTTP 402 + `PAYMENT-REQUIRED` header, no HTML body |
| Payment required | Browser | HTTP 402 + `PAYMENT-REQUIRED` header + HTML paywall body |
| Verification failure | Any | HTTP 402 + `PaymentRequired.error` set |
| Settlement failure | Any | HTTP 500 (or hook recovery) |
| Success | Any | HTTP 200 + `PAYMENT-RESPONSE` header |
| Facilitator boundary error | Any | HTTP 502 with `{ error: message }` JSON body |

---

## 9. E2E hooks

Integration and E2E tests live in `e2e/` (repo root) and `typescript/packages/*/test/integrations/`.

| Test type | Framework | Command | Location |
|-----------|-----------|---------|----------|
| Unit | vitest | `pnpm test` (per-package) | `typescript/packages/<pkg>/test/unit/` |
| Integration | vitest | `pnpm test:integration` (per-package) | `typescript/packages/<pkg>/test/integrations/` |
| E2E (cross-package) | vitest / shell scripts | `e2e/` commands | `e2e/` root |

**Test helper factories** (imported from `test/mocks`):
```typescript
buildPaymentRequired(overrides?)       // constructs PaymentRequired
buildPaymentRequirements(overrides?)   // constructs PaymentRequirements
buildPaymentPayload(overrides?)        // constructs PaymentPayload
buildVerifyResponse(overrides?)        // constructs VerifyResponse
buildSettleResponse(overrides?)        // constructs SettleResponse
buildSupportedResponse(overrides?)     // constructs SupportedResponse
```

**Mock implementations**:
- `MockSchemeNetworkClient` — unit test stub for client-side schemes
- `MockSchemeNetworkServer` — unit test stub for server-side schemes
- `MockFacilitatorClient` — unit test stub for facilitator HTTP client

**Real cash implementations** (for integration tests):
- `CashSchemeNetworkClient`, `CashSchemeNetworkServer`, `CashSchemeNetworkFacilitator` — actual in-process settlement without blockchain

**New feature test requirements** (per `CONTRIBUTING.md`):
1. Unit test for every new public method — mock all dependencies via mock factories
2. Integration test using `CashFacilitatorClient` for any new scheme or facilitator behavior change
3. Update `e2e/` tests when adding a new HTTP framework adapter or new protocol version
4. Run `pnpm changeset` and document all public API additions/changes

---

## 10. Risks / Rollback

### 10.1 Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| Facilitator API contract change breaks SDK | Medium | High | Pin via `/supported` capability check; version negotiation via `x402Version` field |
| Blockchain RPC unavailability | Medium | High | `onVerifyFailure` / `onSettleFailure` hooks allow custom retry/fallback |
| Permit2 nonce collision under high load | Low | Medium | Random 32-byte nonce; collision probability negligible |
| EIP-3009 token compatibility gap | Low | Medium | Permit2 fallback covers all ERC-20s; `AssetTransferMethod` selection handles this |
| Breaking change in mechanism interface | Low | High | Interface changes require major version bump and changeset; v1 shims maintained |
| Payment replay (same signature reused) | Low | High | `validBefore` / deadline enforce one-time use; facilitator must check nonce/expiry |
| `SettlementOverrides` misuse on `exact` scheme | Medium | High | See TBC[6]; `resolveSettlementOverrideAmount` does not guard against non-`upto` schemes |
| Gas sponsoring extension exploit | Low | High | `eip2612GasSponsoring` only activated when extension key declared in `PaymentRequired` |

### 10.2 Rollback strategy

- All releases managed by `@changesets/cli` — rollback = publish previous changeset version tag
- Semver strictly followed: breaking interface changes → major bump
- v1 protocol compatibility layer maintained indefinitely for rollback safety
- No server-side database or persistent state introduced by core SDK — rollback is stateless

### 10.3 Monitoring / safety

- `console.warn` on facilitator fetch failure during `initialize()` (non-fatal if other facilitators succeed)
- `console.error` on extension hook errors (non-fatal; extension skipped; processing continues)
- Express middleware returns HTTP 502 on `FacilitatorResponseError` (boundary error surfaced to API consumers)
- TBC[4]: assumption=no structured logging or metrics hooks in SDK core beyond lifecycle hooks | impact=operators must add observability in lifecycle hooks | needs=decision on whether to standardize a logging hook convention.

---

## 11. Open questions / Decision required

| ID | Section | Question | Assumption made | Impact if wrong | What is needed to confirm |
|----|---------|----------|----------------|-----------------|--------------------------|
| TBC[1] | §7.1 | What is the default facilitator base URL in `HTTPFacilitatorClient`? | Coinbase CDP production facilitator | SDK defaults to wrong endpoint; payments fail without explicit config | Read `typescript/packages/core/src/http/httpFacilitatorClient.ts` for the default URL constant |
| TBC[2] | §7.4 | What fields does `RouteConfigurationError` expose beyond `message`? | Standard `Error` with `message` only | Error-handling code may not extract expected structured fields | Read `typescript/packages/core/src/http/x402HTTPResourceServer.ts` for class definition |
| TBC[3] | §8.2 | Is i18n required for the paywall component? | English only; no i18n infrastructure | Non-English markets unsupported by paywall | Product/design decision required before building new paywall features |
| TBC[4] | §10.3 | Should core SDK expose structured logging / metrics hooks beyond lifecycle hooks? | No — operators add logging inside lifecycle hooks | Observability gaps in production deployments | Architecture decision needed before adding new hook types |
| TBC[5] | §5 | Which features are missing from Python/Go vs TypeScript SDK? | Go has EVM+SVM only; Python has EVM+SVM only (per PRD) | New TypeScript-only features may not be portable to other SDKs | Audit `python/` and `go/` directories for parity gaps before committing to TypeScript-only features |
| TBC[6] | §4.4, §10.1 | What is the expected behavior when `settlement-overrides` is applied to a non-`upto` scheme? | Settlement proceeds with overridden amount; on-chain verification will likely reject | Silent payment amount corruption | Add explicit scheme guard in `settlePayment()` that rejects overrides for non-`upto` schemes |
