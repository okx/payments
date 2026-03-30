# TBC Stubs: x402 Payments SDK

This document records all TBC (To Be Confirmed) items from the spec, their current status, assumptions made, and linked tasks.

## TBC Summary

| ID | Section | Description | Assumption | Status | Linked Task |
|----|---------|-------------|------------|--------|-------------|
| TBC[1] | §7.1 | Default facilitator base URL in `HTTPFacilitatorClient` | `DEFAULT_FACILITATOR_URL = "https://x402.org/facilitator"` | Pending confirmation | T10 |
| TBC[2] | §7.4 | `RouteConfigurationError.errors` typed field structure | `RouteValidationError[]` with `{ routePattern, scheme, network, reason, message }` fields | Pending confirmation | T7 |
| TBC[3] | §8.2 | i18n requirement for paywall | English-only; no i18n infrastructure | Product decision required | None |
| TBC[4] | §10.3 | Structured logging/metrics hooks in core | No structured logging hooks; operators add logging via lifecycle hooks | Architecture decision required | None |
| TBC[5] | §5 | Python/Go parity gaps | Go has EVM+SVM only; Python has EVM+SVM only; TypeScript-only features may not be portable | Audit deferred | None |
| TBC[6] | §4.4/§10.1 | `settlement-overrides` applied to non-`upto` scheme — no guard exists; silent corruption | Guard implemented: `settlePayment()` throws `SettleError` when `SettlementOverrides.amount` is provided for a non-`upto` scheme | Implemented by T2 | T2 |

## Source Annotations

| File | Comment Added |
|------|---------------|
| `typescript/packages/core/src/http/httpFacilitatorClient.ts` | `// TBC[1]: DEFAULT_FACILITATOR_URL confirmed as https://x402.org/facilitator — see T10` |
| `typescript/packages/core/src/http/x402HTTPResourceServer.ts` | `// TBC[2]: RouteConfigurationError.errors is RouteValidationError[] with {message: string} — see T7` |
| `typescript/packages/core/src/server/x402ResourceServer.ts` | `// TBC[6]: scheme guard not yet implemented — see T2` |
