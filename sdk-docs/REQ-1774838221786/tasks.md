# Tasks: x402 Payments SDK — Feature Development

## Wave Plan

```
Wave 1 (parallel): T1, T2, T3, T4, T5, T6
  T1 — TBC checkpoint: document TBC items, create typed stubs (no deps)
  T2 — Settlement override scheme guard in settlePayment() (no deps)
  T3 — x402Client policy engine unit tests (no deps)
  T4 — x402ResourceServer lifecycle hooks unit tests (no deps)
  T5 — x402Facilitator multi-facilitator fallback unit tests (no deps)
  T6 — Browser paywall path unit tests (no deps)

Wave 2 (parallel): T7, T8, T9
  T7 — x402HTTPResourceServer RouteConfigurationError typed fields tests (depends T1)
  T8 — @x402/fastify paymentMiddlewareFromConfig + malformedPathBypass tests (depends T2)
  T9 — Extension error isolation unit tests (depends T4)

Wave 3 (parallel): T10, T11
  T10 — Default facilitator URL constant guard + HTTPFacilitatorClient unit tests (depends T7)
  T11 — MCP client/server unit tests (depends T3, T5)

Wave 4 (sequential): T12
  T12 — Changeset entries for all public API changes (depends T8, T9, T10, T11)
```

---

## T1 — TBC Checkpoint: Document Open Questions and Create Typed Stubs

| Field | Content |
|-------|---------|
| **Task ID** | T1 |
| **Wave** | Wave 1 |
| **Depends On** | — |
| **Intent** | Capture all six TBC items from spec.md as inline comments in relevant source files; create typed stubs where needed so downstream tasks can proceed without being blocked. |
| **Status** | DONE |
| **Spec Context** | spec.md §7.1 TBC[1], §7.4 TBC[2], §8.2 TBC[3], §10.3 TBC[4], §5 TBC[5], §4.4/§10.1 TBC[6] |

### TBC Summary

| ID | Description | Blocked Tasks |
|----|-------------|---------------|
| TBC[1] | Default facilitator base URL in `HTTPFacilitatorClient` — assumed `https://x402.org/facilitator` | None (T10 will add a constant guard) |
| TBC[2] | `RouteConfigurationError.errors` typed field structure — confirmed `RouteValidationError[]` with `message` | None (T7 will add tests) |
| TBC[3] | i18n requirement for paywall — product decision required; assuming English-only | None |
| TBC[4] | Structured logging/metrics hooks in core — no action until architecture decision | None |
| TBC[5] | Python/Go parity gaps — audit deferred; TypeScript-only features may not be portable | None |
| TBC[6] | `settlement-overrides` applied to non-`upto` scheme — no guard exists; silent corruption | T2 implements the guard |

### File Impact List

| Path | Action | Reason |
|------|--------|--------|
| `typescript/packages/core/src/server/x402ResourceServer.ts` | MODIFY | Add `// TBC[6]: scheme guard not yet implemented — see T2` comment above `settlePayment` settlement override block |
| `typescript/packages/core/src/http/httpFacilitatorClient.ts` | MODIFY | Add `// TBC[1]: DEFAULT_FACILITATOR_URL confirmed as https://x402.org/facilitator — see T10` comment |
| `typescript/packages/core/src/http/x402HTTPResourceServer.ts` | MODIFY | Add `// TBC[2]: RouteConfigurationError.errors is RouteValidationError[] with {message: string}` comment |
| `sdk-docs/REQ-1774838221786/tbc-stubs.md` | CREATE | Record each TBC with status, assumption, and linked task |

### Test Scenarios

| Scenario | Expected |
|----------|----------|
| `tbc-stubs.md` exists and lists all 6 TBC items | File created with IDs TBC[1]–TBC[6], status, and linked task |
| Source comments exist at correct locations | `grep -n "TBC\[6\]"` returns match in `x402ResourceServer.ts` |

### Steps

1. Add `// TBC[6]: ...` comment to `x402ResourceServer.ts` above the settlement override block (line ~786)
2. Add `// TBC[1]: ...` comment to `httpFacilitatorClient.ts` at the `DEFAULT_FACILITATOR_URL` constant
3. Add `// TBC[2]: ...` comment to `x402HTTPResourceServer.ts` near `RouteConfigurationError` definition
4. Create `sdk-docs/REQ-1774838221786/tbc-stubs.md` with TBC summary table
5. Run GREEN: `cd typescript && pnpm --filter @x402/core lint:check`

### Commands

```bash
# Verify source files compile cleanly after adding comments
cd typescript && pnpm --filter @x402/core build

# GREEN
cd typescript && pnpm --filter @x402/core lint:check
```

### DoD

- All 6 TBC items annotated in source with `// TBC[n]:` comments
- `sdk-docs/REQ-1774838221786/tbc-stubs.md` created
- `pnpm --filter @x402/core build` and `lint:check` pass

---

## T2 — Settlement Override Scheme Guard in `settlePayment()`

| Field | Content |
|-------|---------|
| **Task ID** | T2 |
| **Wave** | Wave 1 |
| **Depends On** | — |
| **Intent** | Add an explicit scheme guard in `x402ResourceServer.settlePayment()` that throws a `SettleError` when `SettlementOverrides.amount` is provided for a non-`upto` scheme. Prevents silent payment corruption (TBC[6]). |
| **Status** | DONE |
| **Spec Context** | spec.md §4.4, §10.1 TBC[6]; `typescript/packages/core/src/server/x402ResourceServer.ts` lines 779–793 |

### File Impact List

| Path | Action | Reason |
|------|--------|--------|
| `typescript/packages/core/src/server/x402ResourceServer.ts` | MODIFY | Add scheme guard before override application in `settlePayment()` |
| `typescript/packages/core/test/unit/server/x402ResourceServer.test.ts` | MODIFY | Add unit tests: override with `exact` scheme → throws; override with `upto` scheme → passes |

### Test Scenarios

| Scenario | Expected |
|----------|----------|
| `settlePayment()` called with `{ amount: "500" }` override and `scheme: "exact"` | Throws `SettleError` with `errorReason: "settlement_override_not_supported"` |
| `settlePayment()` called with `{ amount: "500" }` override and `scheme: "upto"` | Proceeds; override applied to `effectiveRequirements.amount` |
| `settlePayment()` called with empty `SettlementOverrides {}` and `scheme: "exact"` | No error thrown (no amount override) |
| `settlePayment()` called without `settlementOverrides` and `scheme: "exact"` | No error thrown |

### Steps

1. **Create failing test** in `x402ResourceServer.test.ts`:
   ```typescript
   it("throws SettleError when settlement override applied to non-upto scheme", async () => {
     const requirements = buildPaymentRequirements({ scheme: "exact" });
     await expect(
       server.settlePayment(buildPaymentPayload(), requirements, undefined, undefined, { amount: "500" })
     ).rejects.toThrow(SettleError);
   });
   ```
2. **Run RED**: `cd typescript && pnpm --filter @x402/core test -- x402ResourceServer`
3. **Implement guard** in `settlePayment()` before override application:
   ```typescript
   if (settlementOverrides?.amount !== undefined && requirements.scheme !== "upto") {
     throw new SettleError(400, {
       success: false,
       errorReason: "settlement_override_not_supported",
       errorMessage: `Settlement overrides are only valid for 'upto' scheme; received '${requirements.scheme}'`,
       transaction: "",
       network: requirements.network,
     });
   }
   ```
4. **Run GREEN**: `cd typescript && pnpm --filter @x402/core test && pnpm --filter @x402/core lint:check`
5. **Refactor**: ensure jsdoc on `settlePayment` documents the guard

### Commands

```bash
# RED
cd typescript && pnpm --filter @x402/core test -- --reporter=verbose

# GREEN
cd typescript && pnpm --filter @x402/core test && pnpm --filter @x402/core lint:check && pnpm --filter @x402/core build
```

### DoD

- Guard throws `SettleError` for non-`upto` schemes when override is provided
- Unit tests cover all 4 scenarios above
- `pnpm --filter @x402/core test`, `lint:check`, and `build` pass

---

## T3 — `x402Client` Policy Engine Unit Tests

| Field | Content |
|-------|---------|
| **Task ID** | T3 |
| **Wave** | Wave 1 |
| **Depends On** | — |
| **Intent** | Add comprehensive unit tests for `x402Client.registerPolicy()`, `selectPaymentRequirements()` filter/abort paths, and multi-scheme selection. Spec §4.2 non-happy paths. |
| **Status** | DONE |
| **Spec Context** | spec.md §4.2, §7.3; `typescript/packages/core/src/client/x402Client.ts` lines 70–480 |

### File Impact List

| Path | Action | Reason |
|------|--------|--------|
| `typescript/packages/core/test/unit/client/x402Client.test.ts` | MODIFY | Add policy engine and selector test cases |

### Test Scenarios

| Scenario | Expected |
|----------|----------|
| `registerPolicy()` that filters all requirements | `selectPaymentRequirements()` throws `Error("All payment requirements were filtered out by policies")` |
| `registerPolicy()` that filters down to single requirement | Returns the one matching requirement |
| No registered scheme matching any requirement | Throws `Error("No network/scheme registered")` |
| Multiple policies applied sequentially | Each policy receives filtered output of previous policy |
| `paymentRequirementsSelector` override used when provided | Custom selector called with filtered list; its return value used |
| `createPaymentPayload()` calls `beforePaymentCreation` hooks in order | Hook sequence respected; abort hook prevents payload creation |

### Steps

1. **Write failing tests** for each scenario using `MockSchemeNetworkClient` and `buildPaymentRequired`
2. **Run RED**: `cd typescript && pnpm --filter @x402/core test -- x402Client`
3. **Verify no source changes needed** (tests reveal existing behavior)
4. **Run GREEN**: all new tests pass

### Commands

```bash
# RED
cd typescript && pnpm --filter @x402/core test -- --reporter=verbose x402Client

# GREEN
cd typescript && pnpm --filter @x402/core test && pnpm --filter @x402/core lint:check
```

### DoD

- All 6 scenarios have passing unit tests
- `pnpm --filter @x402/core test` and `lint:check` pass

---

## T4 — `x402ResourceServer` Lifecycle Hook Unit Tests

| Field | Content |
|-------|---------|
| **Task ID** | T4 |
| **Wave** | Wave 1 |
| **Depends On** | — |
| **Intent** | Add unit test coverage for all lifecycle hook paths on `x402ResourceServer`: `beforeVerify`, `afterVerify`, `onVerifyFailure`, `beforeSettle`, `afterSettle`, `onSettleFailure`. Covers abort, recovery, and error propagation. Spec §4.1, §7.5. |
| **Status** | DONE |
| **Spec Context** | spec.md §4.1, §7.5; `typescript/packages/core/src/server/x402ResourceServer.ts` |

### File Impact List

| Path | Action | Reason |
|------|--------|--------|
| `typescript/packages/core/test/unit/server/x402ResourceServer.test.ts` | MODIFY | Add hook lifecycle test cases |

### Test Scenarios

| Scenario | Expected |
|----------|----------|
| `beforeVerify` hook returns `{ abort: true, reason: "blocked" }` | `verifyPayment()` returns `{ isValid: false, invalidReason: "blocked" }` |
| `afterVerify` hook throws | Error propagates as `VerifyError` |
| `onVerifyFailure` hook returns `{ recovered: true, result }` | Original verify error swallowed; `result` returned |
| `beforeSettle` hook returns `{ abort: true }` | `settlePayment()` throws `SettleError` |
| `afterSettle` hook throws | Error propagates |
| `onSettleFailure` hook returns `{ recovered: true, result }` | Original settle error swallowed; `result` returned |
| Multiple `beforeVerify` hooks — first aborts | Second hook never executed |

### Steps

1. **Write failing tests** for each scenario using `MockSchemeNetworkFacilitator` and `buildPaymentPayload`
2. **Run RED**: `cd typescript && pnpm --filter @x402/core test -- x402ResourceServer`
3. **Verify no source changes needed**
4. **Run GREEN**: all new tests pass

### Commands

```bash
# RED
cd typescript && pnpm --filter @x402/core test -- --reporter=verbose x402ResourceServer

# GREEN
cd typescript && pnpm --filter @x402/core test && pnpm --filter @x402/core lint:check
```

### DoD

- All 7 hook scenarios have passing unit tests
- `pnpm --filter @x402/core test` and `lint:check` pass

---

## T5 — `x402Facilitator` Multi-Facilitator Fallback Unit Tests

| Field | Content |
|-------|---------|
| **Task ID** | T5 |
| **Wave** | Wave 1 |
| **Depends On** | — |
| **Intent** | Add unit tests for `x402Facilitator` multi-facilitator registration: first-registered precedence, unknown-scheme fallback iteration, and verify/settle delegation. Spec §4.3, §7.5. |
| **Status** | DONE |
| **Spec Context** | spec.md §4.3, §7.5; `typescript/packages/core/src/facilitator/x402Facilitator.ts` |

### File Impact List

| Path | Action | Reason |
|------|--------|--------|
| `typescript/packages/core/test/unit/facilitator/x402Facilitator.test.ts` | MODIFY | Add multi-facilitator and fallback test cases |

### Test Scenarios

| Scenario | Expected |
|----------|----------|
| Two facilitators both support `exact/eip155:*`; first registered | First facilitator's `verify()` called; second never called |
| First facilitator does not support scheme; second does | Fallback iterates; second facilitator's `verify()` called |
| No facilitator supports scheme | Throws `VerifyError` with unsupported scheme message |
| `settle()` fallback follows same precedence rule as `verify()` | Same precedence verified for settle path |

### Steps

1. **Write failing tests** using `MockSchemeNetworkFacilitator` stubs (from `test/mocks`)
2. **Run RED**: `cd typescript && pnpm --filter @x402/core test -- x402Facilitator`
3. **Verify no source changes needed**
4. **Run GREEN**: all new tests pass

### Commands

```bash
# RED
cd typescript && pnpm --filter @x402/core test -- --reporter=verbose x402Facilitator

# GREEN
cd typescript && pnpm --filter @x402/core test && pnpm --filter @x402/core lint:check
```

### DoD

- All 4 scenarios have passing unit tests
- `pnpm --filter @x402/core test` and `lint:check` pass

---

## T6 — Browser Paywall Path Unit Tests

| Field | Content |
|-------|---------|
| **Task ID** | T6 |
| **Wave** | Wave 1 |
| **Depends On** | — |
| **Intent** | Add unit tests for browser detection logic and HTML paywall response in `x402HTTPResourceServer`. Covers `Accept: text/html` + `Mozilla` user-agent path (spec §8.1, §8.3). |
| **Status** | DONE |
| **Spec Context** | spec.md §8.1, §8.3; `typescript/packages/core/src/http/x402HTTPResourceServer.ts` lines 859–891 |

### File Impact List

| Path | Action | Reason |
|------|--------|--------|
| `typescript/packages/core/test/unit/http/x402HTTPResourceServer.hooks.test.ts` | MODIFY | Add browser-detection and paywall HTML path tests |

### Test Scenarios

| Scenario | Expected |
|----------|----------|
| Request with `Accept: text/html` and `User-Agent: Mozilla/5.0` | 402 response has `isHtml: true` and `Content-Type: text/html` body |
| Request with `Accept: application/json` | 402 response is JSON (no HTML paywall body) |
| `PaywallProvider.generateHtml()` registered and called for browser request | HTML body equals output of `generateHtml(paymentRequired, config)` |
| `customPaywallHtml` set on route config | Custom HTML used instead of `PaywallProvider` |
| No `PaywallProvider` registered, browser request | Fallback: standard 402 response without HTML paywall body |

### Steps

1. **Write failing tests** using mock adapter implementing `getAcceptHeader()` and `getUserAgent()`
2. **Run RED**: `cd typescript && pnpm --filter @x402/core test -- x402HTTPResourceServer.hooks`
3. **Verify source already handles these paths** (lines 859–891 in `x402HTTPResourceServer.ts`)
4. **Run GREEN**: tests pass

### Commands

```bash
# RED
cd typescript && pnpm --filter @x402/core test -- --reporter=verbose x402HTTPResourceServer.hooks

# GREEN
cd typescript && pnpm --filter @x402/core test && pnpm --filter @x402/core lint:check
```

### DoD

- All 5 browser/paywall scenarios have passing unit tests
- `pnpm --filter @x402/core test` and `lint:check` pass

---

## T7 — `RouteConfigurationError` Typed Fields Unit Tests

| Field | Content |
|-------|---------|
| **Task ID** | T7 |
| **Wave** | Wave 2 |
| **Depends On** | T1 |
| **Intent** | Add unit tests asserting `RouteConfigurationError` exposes `errors: RouteValidationError[]` with `message: string` per instance. Resolves TBC[2]. |
| **Status** | DONE |
| **Spec Context** | spec.md §7.4 TBC[2]; `typescript/packages/core/src/http/x402HTTPResourceServer.ts` line 274+ |

### File Impact List

| Path | Action | Reason |
|------|--------|--------|
| `typescript/packages/core/test/unit/http/x402HTTPResourceServer.errors.test.ts` | MODIFY | Add `RouteConfigurationError.errors` typed field assertions |

### Test Scenarios

| Scenario | Expected |
|----------|----------|
| `new RouteConfigurationError([{ message: "bad route" }])` | `error.errors[0].message === "bad route"` |
| `error.errors` is `RouteValidationError[]` at runtime | Array length matches input; each element has `message` field |
| `error.name` | `"RouteConfigurationError"` |
| `error.message` | Contains each validation error message joined with newlines |

### Steps

1. **Write failing tests** importing `RouteConfigurationError` and `RouteValidationError` types
2. **Run RED**: `cd typescript && pnpm --filter @x402/core test -- x402HTTPResourceServer.errors`
3. **Verify types and class match** — no source changes expected
4. **Run GREEN**: tests pass; update `tbc-stubs.md` TBC[2] status to CONFIRMED

### Commands

```bash
# RED
cd typescript && pnpm --filter @x402/core test -- --reporter=verbose x402HTTPResourceServer.errors

# GREEN
cd typescript && pnpm --filter @x402/core test && pnpm --filter @x402/core lint:check
```

### DoD

- 4 test scenarios pass
- `sdk-docs/REQ-1774838221786/tbc-stubs.md` updated: TBC[2] status → CONFIRMED

---

## T8 — `@x402/fastify` `paymentMiddlewareFromConfig` and `malformedPathBypass` Tests

| Field | Content |
|-------|---------|
| **Task ID** | T8 |
| **Wave** | Wave 2 |
| **Depends On** | T2 |
| **Intent** | Add unit tests for `paymentMiddlewareFromConfig` in `@x402/fastify` and ensure `malformedPathBypass` behavior is fully covered. Tests verify the scheme guard added in T2 propagates correctly through the middleware stack. |
| **Status** | DONE |
| **Spec Context** | spec.md §4.1, §5.1; `typescript/packages/http/fastify/src/index.ts`, `typescript/packages/http/fastify/src/malformedPathBypass.test.ts` |

### File Impact List

| Path | Action | Reason |
|------|--------|--------|
| `typescript/packages/http/fastify/src/index.test.ts` | MODIFY | Add `paymentMiddlewareFromConfig` and scheme-guard-propagation tests |
| `typescript/packages/http/fastify/src/malformedPathBypass.test.ts` | MODIFY | Ensure malformed path bypass tests cover all edge cases |

### Test Scenarios

| Scenario | Expected |
|----------|----------|
| `paymentMiddlewareFromConfig` creates middleware with auto-constructed server + schemes | Middleware registered on Fastify; `addHook` called with `onRequest` |
| Request with malformed URL path (double-slash, encoded null byte) | Middleware bypasses payment check; no 402 returned |
| Settlement override with `exact` scheme propagates `SettleError` as HTTP 500 | Middleware returns 500 with `errorReason: "settlement_override_not_supported"` |
| `paymentMiddlewareFromConfig` with missing required config field | Throws at construction time with descriptive error |

### Steps

1. **Write failing tests** using mock Fastify instance (pattern from existing `malformedPathBypass.test.ts`)
2. **Run RED**: `cd typescript && pnpm --filter @x402/fastify test`
3. **Implement any missing middleware path handling** in `index.ts` if tests reveal gaps
4. **Run GREEN**: `cd typescript && pnpm --filter @x402/fastify test && pnpm --filter @x402/fastify lint:check`

### Commands

```bash
# RED
cd typescript && pnpm --filter @x402/fastify test -- --reporter=verbose

# GREEN
cd typescript && pnpm --filter @x402/fastify test && pnpm --filter @x402/fastify lint:check && pnpm --filter @x402/fastify build
```

### DoD

- All 4 test scenarios pass
- `pnpm --filter @x402/fastify test`, `lint:check`, `build` pass

---

## T9 — Extension Error Isolation Unit Tests

| Field | Content |
|-------|---------|
| **Task ID** | T9 |
| **Wave** | Wave 2 |
| **Depends On** | T4 |
| **Intent** | Add unit tests proving that errors thrown in extension hooks are caught internally, logged to `console.error`, and do not abort the payment request. Spec §7.5 extension errors. |
| **Status** | DONE |
| **Spec Context** | spec.md §7.5; `typescript/packages/extensions/src/`; `typescript/packages/core/src/server/x402ResourceServer.ts` |

### File Impact List

| Path | Action | Reason |
|------|--------|--------|
| `typescript/packages/core/test/unit/server/x402ResourceServer.test.ts` | MODIFY | Add extension error isolation tests with `console.error` spy |

### Test Scenarios

| Scenario | Expected |
|----------|----------|
| Extension `onRequest` hook throws `Error("ext-fail")` | Request processing continues; `console.error` called with the error |
| Extension error does not propagate as 500 | Response is either 200 or 402 (not 500) |
| Non-throwing extension hook executes normally | Hook return value processed as expected |

### Steps

1. **Write failing tests** mocking a throwing extension and spying on `console.error` via `vi.spyOn`
2. **Run RED**: `cd typescript && pnpm --filter @x402/core test -- x402ResourceServer`
3. **Verify source handles this** — should already be caught per spec §7.5
4. **Run GREEN**: tests pass

### Commands

```bash
# RED
cd typescript && pnpm --filter @x402/core test -- --reporter=verbose x402ResourceServer

# GREEN
cd typescript && pnpm --filter @x402/core test && pnpm --filter @x402/core lint:check
```

### DoD

- 3 extension error isolation tests pass
- `pnpm --filter @x402/core test` and `lint:check` pass

---

## T10 — Default Facilitator URL Constant Guard + `HTTPFacilitatorClient` Unit Tests

| Field | Content |
|-------|---------|
| **Task ID** | T10 |
| **Wave** | Wave 3 |
| **Depends On** | T7 |
| **Intent** | Assert `DEFAULT_FACILITATOR_URL` constant equals `https://x402.org/facilitator`. Add unit tests for `HTTPFacilitatorClient` covering `/verify`, `/settle`, `/supported` request construction and error handling. Resolves TBC[1]. |
| **Status** | DONE |
| **Spec Context** | spec.md §7.1 TBC[1]; `typescript/packages/core/src/http/httpFacilitatorClient.ts` line 12 |

### File Impact List

| Path | Action | Reason |
|------|--------|--------|
| `typescript/packages/core/src/http/httpFacilitatorClient.ts` | MODIFY | Export `DEFAULT_FACILITATOR_URL` constant if not already exported |
| `typescript/packages/core/test/unit/http/httpFacilitatorClient.test.ts` | MODIFY | Add constant guard and request/error unit tests |

### Test Scenarios

| Scenario | Expected |
|----------|----------|
| `DEFAULT_FACILITATOR_URL` constant | Equals `"https://x402.org/facilitator"` |
| `HTTPFacilitatorClient.verify()` with 200 response | Returns `VerifyResponse` object |
| `HTTPFacilitatorClient.verify()` with non-OK response | Throws `VerifyError` with `statusCode` |
| `HTTPFacilitatorClient.settle()` with 200 response | Returns `SettleResponse` object |
| `HTTPFacilitatorClient.supported()` with 200 response | Returns `SupportedResponse` |
| `FacilitatorResponseError` thrown on malformed success body | `getFacilitatorResponseError()` helper extracts error from cause chain |

### Steps

1. **Write failing tests** using `vi.fn()` to mock `fetch`; import `DEFAULT_FACILITATOR_URL`
2. **Run RED**: `cd typescript && pnpm --filter @x402/core test -- httpFacilitatorClient`
3. **Export `DEFAULT_FACILITATOR_URL`** from `httpFacilitatorClient.ts` if not already exported; verify value is `https://x402.org/facilitator`
4. **Run GREEN**: tests pass; update `tbc-stubs.md` TBC[1] status to CONFIRMED

### Commands

```bash
# RED
cd typescript && pnpm --filter @x402/core test -- --reporter=verbose httpFacilitatorClient

# GREEN
cd typescript && pnpm --filter @x402/core test && pnpm --filter @x402/core lint:check && pnpm --filter @x402/core build
```

### DoD

- All 6 scenarios pass
- `DEFAULT_FACILITATOR_URL` exported from `httpFacilitatorClient.ts`
- `sdk-docs/REQ-1774838221786/tbc-stubs.md` updated: TBC[1] status → CONFIRMED
- `pnpm --filter @x402/core build` passes

---

## T11 — MCP Client/Server Unit Tests

| Field | Content |
|-------|---------|
| **Task ID** | T11 |
| **Wave** | Wave 3 |
| **Depends On** | T3, T5 |
| **Intent** | Add unit tests for `x402MCPClient` and `x402MCPServer` covering payment flow: tool call interception, payment injection, and MCP response processing. |
| **Status** | DONE |
| **Spec Context** | spec.md §4.2; `typescript/packages/mcp/src/client/x402MCPClient.ts`, `typescript/packages/mcp/src/server/` |

### File Impact List

| Path | Action | Reason |
|------|--------|--------|
| `typescript/packages/mcp/test/unit/x402MCPClient.test.ts` | CREATE | Unit tests for `x402MCPClient` tool-call payment injection |
| `typescript/packages/mcp/test/unit/x402MCPServer.test.ts` | CREATE | Unit tests for `x402MCPServer` payment gate on tool calls |

### Test Scenarios

| Scenario | Expected |
|----------|----------|
| MCP tool call to a paid resource — 402 returned by transport | `x402MCPClient` intercepts, creates payload, retries with `PAYMENT-SIGNATURE` |
| No registered scheme on `x402MCPClient` | Throws `Error("No network/scheme registered")` |
| `x402MCPServer` receives tool call without payment header | Returns 402-equivalent error with `PAYMENT-REQUIRED` content |
| `x402MCPServer` receives tool call with valid payment | Processes tool call normally; returns tool result |
| `createx402MCPClient()` factory creates client with provided config | Client has correct scheme registrations |

### Steps

1. **Write failing unit tests** using mock MCP transport (pattern from `test/integration/mcp-payment-flow.test.ts`)
2. **Run RED**: `cd typescript && pnpm --filter @x402/mcp test`
3. **Create unit test files** at `typescript/packages/mcp/test/unit/`
4. **Run GREEN**: `cd typescript && pnpm --filter @x402/mcp test && pnpm --filter @x402/mcp lint:check`

### Commands

```bash
# RED
cd typescript && pnpm --filter @x402/mcp test -- --reporter=verbose

# GREEN
cd typescript && pnpm --filter @x402/mcp test && pnpm --filter @x402/mcp lint:check && pnpm --filter @x402/mcp build
```

### DoD

- All 5 unit test scenarios pass
- `pnpm --filter @x402/mcp test`, `lint:check`, `build` pass

---

## T12 — Changeset Entries for Public API Changes

| Field | Content |
|-------|---------|
| **Task ID** | T12 |
| **Wave** | Wave 4 |
| **Depends On** | T8, T9, T10, T11 |
| **Intent** | Create `@changesets/cli` changeset fragments for every public API change introduced in prior tasks. The `settlePayment()` scheme guard (T2) is a breaking-change for callers relying on the previous behaviour; export of `DEFAULT_FACILITATOR_URL` (T10) is a minor addition. |
| **Status** | DONE |
| **Spec Context** | spec.md §10.2; `typescript/CONTRIBUTING.md` §Changelog |

### File Impact List

| Path | Action | Reason |
|------|--------|--------|
| `typescript/.changeset/<hash>.md` | CREATE | Changeset fragment for `@x402/core` scheme guard (patch) |
| `typescript/.changeset/<hash>.md` | CREATE | Changeset fragment for `@x402/core` `DEFAULT_FACILITATOR_URL` export (minor) |

### Test Scenarios

| Scenario | Expected |
|----------|----------|
| `pnpm changeset status` | Detects at least two pending changesets |
| Changeset for scheme guard describes `@x402/core` | Fragment mentions `settlePayment` and `settlement_override_not_supported` |
| Changeset for URL export describes `@x402/core` | Fragment mentions `DEFAULT_FACILITATOR_URL` export |

### Steps

1. **Run** `cd typescript && pnpm changeset` — select `@x402/core`; type `patch`; describe: "Fixed: settlePayment now throws SettleError when SettlementOverrides.amount is provided for non-upto schemes"
2. **Run** `cd typescript && pnpm changeset` again — select `@x402/core`; type `minor`; describe: "Added: export DEFAULT_FACILITATOR_URL constant from httpFacilitatorClient"
3. **Verify**: `ls typescript/.changeset/` shows new `.md` files; `pnpm changeset status` confirms pending

### Commands

```bash
# Create changesets (interactive)
cd typescript && pnpm changeset

# Verify
cd typescript && pnpm changeset status
```

### DoD

- At least two changeset fragments exist under `typescript/.changeset/`
- `pnpm changeset status` reports pending changes
- Each fragment accurately describes the change

---

## Appendix: TBC Items Reference

| ID | Section | Assumption | Status | Linked Task |
|----|---------|-----------|--------|-------------|
| TBC[1] | §7.1 | `DEFAULT_FACILITATOR_URL = "https://x402.org/facilitator"` | Pending confirmation | T10 |
| TBC[2] | §7.4 | `RouteConfigurationError.errors: RouteValidationError[]` with `{ message: string }` | Pending confirmation | T7 |
| TBC[3] | §8.2 | English-only paywall; no i18n | Product decision required | None |
| TBC[4] | §10.3 | No structured logging hooks in core | Architecture decision required | None |
| TBC[5] | §5 | Go/Python: EVM+SVM only; TypeScript-only features may not be portable | Audit deferred | None |
| TBC[6] | §4.4/§10.1 | No scheme guard on `settlePayment()` for non-`upto` schemes | Implemented by T2 | T2 |
