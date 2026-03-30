---
'@x402/core': patch
---

Fixed: settlePayment now throws SettleError when SettlementOverrides.amount is provided for non-upto schemes (e.g. exact). Prevents silent payment corruption from misapplied settlement overrides. Error reason: settlement_override_not_supported.
