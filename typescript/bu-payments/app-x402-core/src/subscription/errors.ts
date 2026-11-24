/**
 * Canonical facilitator error reasons. The SDK uses these literal strings
 * verbatim in verify / settle / charge return values so seller code can
 * switch on a single set of codes regardless of whether the rejection
 * originated in the SDK pre-check or in the facilitator response.
 */
export const ErrorCode = {
  // subscribe / change
  TermsBindingInvalid: "terms_binding_invalid",
  AllowanceInsufficient: "allowance_insufficient",
  AllowanceExpired: "allowance_expired",
  // charge
  PeriodNotDue: "period_not_due",
  InsufficientBalance: "insufficient_balance",
  // charge / cancel / access
  SubscriptionNotActive: "subscription_not_active",
  /**
   * SDK-local code. Surfaced by `verifyAccess` when local period math
   * yields `currentCalculatePeriod === 0` — subscription exists but
   * `nowSec < startAt`, i.e. has not yet entered its first chargeable
   * period.
   */
  SubscriptionNotYetActive: "subscription_not_yet_active",
  UnauthorizedCaller: "unauthorized_caller",
  // cancel
  CancelSignatureInvalid: "cancel_signature_invalid",
  CancelNonceUsed: "cancel_nonce_used",
  // change
  TierSame: "tier_same",
  ChangeEffectiveAtMismatch: "change_effective_at_mismatch",
  MerchantMismatch: "merchant_mismatch",
  PayerMismatch: "payer_mismatch",
  PendingChangeExists: "pending_change_exists",
  SubNotActiveForChange: "sub_not_active_for_change",
  // cancel-pending-change
  NoPendingChange: "no_pending_change",
  // all writes
  ConfirmationTimeout: "confirmation_timeout",
} as const;

export type ErrorCode = (typeof ErrorCode)[keyof typeof ErrorCode];

/**
 * Charge-flow subset of ErrorCode. Seller scheduler switches on these 6
 * codes.
 */
export const ChargeErrorCode = {
  PeriodNotDue: ErrorCode.PeriodNotDue,
  SubscriptionNotActive: ErrorCode.SubscriptionNotActive,
  InsufficientBalance: ErrorCode.InsufficientBalance,
  AllowanceExpired: ErrorCode.AllowanceExpired,
  UnauthorizedCaller: ErrorCode.UnauthorizedCaller,
  ConfirmationTimeout: ErrorCode.ConfirmationTimeout,
} as const;

export type ChargeErrorCode = (typeof ChargeErrorCode)[keyof typeof ChargeErrorCode];

export class ChargeError extends Error {
  public readonly code: ChargeErrorCode;
  public readonly subId: string;
  public readonly txHash?: string;

  constructor(code: ChargeErrorCode, subId: string, txHash?: string) {
    super(`charge failed: ${code} (sub=${subId})`);
    this.name = "ChargeError";
    this.code = code;
    this.subId = subId;
    this.txHash = txHash;
  }
}
