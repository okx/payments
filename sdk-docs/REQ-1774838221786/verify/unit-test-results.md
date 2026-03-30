# Unit Test Results — REQ-1774838221786

**Date**: 2026-03-30
**Branch**: feature/REQ-1774838221786
**Package under test**: `@x402/core`

---

## 1. Summary

| Check      | Result |
|------------|--------|
| Lint       | PASS   |
| Typecheck  | PASS   |
| Unit Tests | FAIL (1 pre-existing failure, unrelated to this requirement) |

---

## 2. New Test Cases (Introduced by This Requirement)

All new test cases are in `typescript/packages/core/test/unit/server/x402ResourceServer.test.ts`.

| # | Test Case | Result |
|---|-----------|--------|
| 1 | `x402ResourceServer > Lifecycle hooks > onAfterVerify > afterVerify hook that throws propagates the error` | PASS |
| 2 | `x402ResourceServer > Lifecycle hooks > onAfterSettle > afterSettle hook that throws propagates the error` | PASS |
| 3 | `x402ResourceServer > settlePayment > throws SettleError when settlement override applied to non-upto scheme (exact)` | PASS |
| 4 | `x402ResourceServer > settlePayment > throws SettleError with settlement_override_not_supported reason for non-upto scheme` | PASS |
| 5 | `x402ResourceServer > settlePayment > does not throw when settlement override applied to upto scheme` | PASS |
| 6 | `x402ResourceServer > settlePayment > does not throw when empty SettlementOverrides with exact scheme` | PASS |
| 7 | `x402ResourceServer > settlePayment > does not throw when no settlementOverrides with exact scheme` | PASS |
| 8 | `x402ResourceServer > Extension error isolation > enrichPaymentRequiredResponse hook that throws logs to console.error and does not abort the request` | PASS |
| 9 | `x402ResourceServer > Extension error isolation > enrichSettlementResponse hook that throws logs to console.error and does not abort settlement` | PASS |
| 10 | `x402ResourceServer > Extension error isolation > non-throwing extension hook executes normally and its return value is included in extensions` | PASS |

All 10 new test cases PASS.

---

## 3. Failures

### Pre-existing Failure (not introduced by this requirement)

**Test**: `x402ResourceServer > Construction > should create default client if empty array provided`
**File**: `test/unit/server/x402ResourceServer.test.ts:61`
**Error**: `Test timed out in 5000ms`

**Root Cause**: This test calls `new x402ResourceServer([])` which falls back to creating a real `HTTPFacilitatorClient`. When `server.initialize()` is called, the client attempts a real outbound HTTP request to a facilitator server that is not reachable in this environment, causing the timeout. This failure was confirmed to exist on the commit _before_ the current change (`git stash` + re-run reproduced the same failure on the prior commit `da7c1e8`).

**Impact**: Pre-existing, not introduced by REQ-1774838221786. The current change to `x402ResourceServer.ts` is limited to a comment update on lines 783–787 only.

---

## 4. Raw Output (last 50 lines)

```
   ✓ x402ResourceServer > settlePayment > should settle payment through facilitator client 0ms
   ✓ x402ResourceServer > settlePayment > should use original amount when no overrides provided 0ms
   ✓ x402ResourceServer > settlePayment > should override amount when settlementOverrides.amount is provided 0ms
   ✓ x402ResourceServer > settlePayment > should not mutate original requirements when overrides applied 0ms
   ✓ x402ResourceServer > settlePayment > should use original amount when overrides has undefined amount 0ms
   ✓ x402ResourceServer > settlePayment > should allow settling for zero amount 0ms
   ✓ x402ResourceServer > settlePayment > should pass overridden requirements to beforeSettle hooks 0ms
   ✓ x402ResourceServer > settlePayment > throws SettleError when settlement override applied to non-upto scheme (exact) 0ms
   ✓ x402ResourceServer > settlePayment > throws SettleError with settlement_override_not_supported reason for non-upto scheme 0ms
   ✓ x402ResourceServer > settlePayment > does not throw when settlement override applied to upto scheme 0ms
   ✓ x402ResourceServer > settlePayment > does not throw when empty SettlementOverrides with exact scheme 0ms
   ✓ x402ResourceServer > settlePayment > does not throw when no settlementOverrides with exact scheme 0ms
   ✓ x402ResourceServer > findMatchingRequirements > should match v2 requirements by deep equality 0ms
   ✓ x402ResourceServer > findMatchingRequirements > should match v1 requirements by scheme and network 0ms
   ✓ x402ResourceServer > findMatchingRequirements > should return undefined if no match found 0ms
   ✓ x402ResourceServer > findMatchingRequirements > should handle objects with different property order (v2) 0ms
   ✓ x402ResourceServer > createPaymentRequiredResponse > should create v2 response 0ms
   ✓ x402ResourceServer > createPaymentRequiredResponse > should include error message if provided 0ms
   ✓ x402ResourceServer > createPaymentRequiredResponse > should include extensions if provided 0ms
   ✓ x402ResourceServer > createPaymentRequiredResponse > should omit extensions if empty 0ms
   ✓ x402ResourceServer > getSupportedKind and getFacilitatorExtensions > should return supported kind after initialization 0ms
   ✓ x402ResourceServer > getSupportedKind and getFacilitatorExtensions > should return undefined if not found 0ms
   ✓ x402ResourceServer > getSupportedKind and getFacilitatorExtensions > should return facilitator extensions 0ms
   ✓ x402ResourceServer > getSupportedKind and getFacilitatorExtensions > should return empty array if no extensions 0ms
   ✓ x402ResourceServer > Extension error isolation > enrichPaymentRequiredResponse hook that throws logs to console.error and does not abort the request 2ms
   ✓ x402ResourceServer > Extension error isolation > enrichSettlementResponse hook that throws logs to console.error and does not abort settlement 0ms
   ✓ x402ResourceServer > Extension error isolation > non-throwing extension hook executes normally and its return value is included in extensions 0ms
   ✓ resolveSettlementOverrideAmount > raw atomic units > passes through a plain numeric string unchanged 0ms
   ✓ resolveSettlementOverrideAmount > percent format > resolves '50%' to half of requirements.amount 0ms
   ✓ resolveSettlementOverrideAmount > dollar price format > converts '$1.00' using default 6 decimals 0ms

⎯⎯⎯⎯⎯⎯⎯ Failed Tests 1 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  test/unit/server/x402ResourceServer.test.ts > x402ResourceServer > Construction > should create default client if empty array provided
Error: Test timed out in 5000ms.
If this is a long-running test, pass a timeout value as the last argument or configure it globally with "testTimeout".
 ❯ test/unit/server/x402ResourceServer.test.ts:61:5

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[1/1]⎯

 Test Files  1 failed | 15 passed (16)
      Tests  1 failed | 389 passed (390)
   Start at  04:21:55
   Duration  5.45s (transform 801ms, setup 0ms, collect 1.74s, tests 5.17s, environment 2ms, prepare 951ms)

 ELIFECYCLE  Test failed. See above for more details.
```
