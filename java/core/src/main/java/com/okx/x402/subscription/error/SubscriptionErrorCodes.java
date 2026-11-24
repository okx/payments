// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.error;

import java.util.Set;

/**
 * Subscription error codes. String values are aligned VERBATIM with the smart-account
 * facilitator IMPLEMENTATION ({@code X402SubscriptionErrorEnum}, pre branch) — these strings
 * go on the wire (local pre-check 402s) and come back from SA (`msg` field), so any spelling
 * drift breaks buyer-side error handling across SDKs.
 *
 * <p>Doc §6 (`subscription-facilitator-api.md`) spells eight of these differently
 * (all-snake-case) from what SA actually emits (mixed camelCase); the implementation is the
 * arbiter, so the constants carry SA's real spellings and {@link #classify} additionally
 * tolerates the doc variants via {@code DOC_ALIASES} in case SA later aligns to the doc.
 */
public final class SubscriptionErrorCodes {

    private SubscriptionErrorCodes() {}

    public enum Category {
        LOCAL_REJECT,
        BUSINESS,
        RETRYABLE,
        SELF_HEAL
    }

    // Request-validation errors (SA §6; also emitted by the SDK's local pre-checks)
    public static final String MISSING_REQUIRED_TERMS_FIELDS = "missing_required_terms_fields";
    public static final String INVALID_ADDRESS_FORMAT = "invalid_address_format";
    public static final String INVALID_BYTES32 = "invalid_bytes32";
    public static final String AMOUNT_PER_PERIOD_INVALID = "amount_per_period_invalid";
    public static final String PERIOD_SEC_INVALID = "period_sec_invalid";
    public static final String MAX_PERIODS_INVALID = "max_periods_invalid";
    public static final String PLAN_TIER_INVALID = "plan_tier_invalid";
    public static final String START_AT_IN_PAST = "start_at_in_past";
    public static final String START_AT_MISMATCH = "start_at_mismatch";
    public static final String PERMIT_SPENDER_MISMATCH = "permit_spender_mismatch";
    public static final String PERMIT_HASH_MISMATCH = "permit_hash_mismatch";
    public static final String TOKEN_MISMATCH = "token_mismatch";
    public static final String ALLOWANCE_INSUFFICIENT = "allowance_insufficient";
    public static final String ALLOWANCE_EXPIRED = "allowance_expired";
    public static final String CREATE_MUST_HAVE_ZERO_CHANGE_FROM =
            "create_must_have_zero_changeFromSubId";
    public static final String CREATE_MUST_HAVE_NONE_CHANGE_EFFECTIVE_AT =
            "create_must_have_none_changeEffectiveAt";
    public static final String CHANGE_MUST_HAVE_NONZERO_CHANGE_FROM =
            "change_must_have_nonzero_changeFromSubId";
    public static final String CHANGE_MUST_HAVE_NON_NONE_EFFECTIVE_AT =
            "change_must_have_non_none_effectiveAt";
    public static final String CHANGE_EFFECTIVE_AT_MISMATCH = "change_effective_at_mismatch";
    public static final String CHANGE_FROM_SUB_ID_MISMATCH = "changeFromSubId_mismatch";
    public static final String PAYER_MISMATCH = "payer_mismatch";
    public static final String MERCHANT_MISMATCH = "merchant_mismatch";
    public static final String FACILITATOR_MISMATCH = "facilitator_mismatch";
    public static final String PERIOD_SEC_MISMATCH = "period_sec_mismatch";
    public static final String TERMS_EXPIRED = "terms_deadline_expired";
    public static final String SIG_EXPIRED = "permit_sig_deadline_expired";
    public static final String TERMS_SIGNATURE_INVALID = "terms_signature_invalid";
    public static final String TERMS_BINDING_INVALID = "terms_binding_invalid";
    public static final String PERMIT_SIGNATURE_INVALID = "permit_signature_invalid";
    public static final String SIGNATURE_HIGH_S = "signature_high_s";
    public static final String SIGNATURE_RECOVERY_FAILED = "signature_recovery_failed";
    public static final String INITIAL_CHARGE_PERIODS_EXCEED_MAX =
            "initial_charge_periods_exceeds_max";
    public static final String INITIAL_CHARGE_AMOUNT_EXCEEDS_CAP = "initial_charge_exceeds_limit";
    public static final String CANCEL_AUTH_REQUIRED = "cancel_auth_required";
    public static final String CANCEL_SUB_ID_MISMATCH = "cancel_subId_mismatch";
    public static final String CANCEL_DEADLINE_EXPIRED = "cancel_deadline_expired";
    public static final String CANCEL_SIGNATURE_INVALID = "cancel_signature_invalid";
    public static final String PENDING_CANCEL_SUB_ID_MISMATCH = "pending_cancel_subId_mismatch";
    public static final String PENDING_CANCEL_DEADLINE_EXPIRED =
            "pending_cancel_deadline_expired";
    public static final String PENDING_CANCEL_SIGNATURE_INVALID =
            "pending_cancel_signature_invalid";
    public static final String PENDING_CANCEL_TARGET_MISMATCH = "pending_cancel_target_mismatch";
    public static final String SALT_ALREADY_USED = "salt_already_used";
    public static final String INITIAL_CHARGE_MISMATCH = "initial_charge_mismatch";
    public static final String PARAM_MISMATCH = "param_mismatch";
    // SA's change-path verifiers use "_passed"/"_invalid" spellings where the create path
    // uses "_expired" (TERMS_EXPIRED / SIG_EXPIRED above) — both are live on the wire.
    public static final String TERMS_DEADLINE_PASSED = "terms_deadline_passed";
    public static final String PERMIT_SIG_DEADLINE_PASSED = "permit_sig_deadline_passed";
    public static final String PERMIT_SIG_INVALID = "permit_sig_invalid";

    // Business errors
    public static final String TIER_SAME = "tier_same";
    public static final String PENDING_CHANGE_EXISTS = "pending_change_exists";
    public static final String SUB_NOT_ACTIVE_FOR_CHANGE = "sub_not_active_for_change";
    public static final String ALL_PERIODS_CHARGED = "all_periods_charged";
    public static final String SUBSCRIPTION_NOT_ACTIVE = "subscription_not_active";
    public static final String NO_PENDING_CHANGE = "no_pending_change_or_not_pending";
    public static final String NOT_ENDED = "not_ended";
    public static final String FEATURE_DISABLED = "feature_disabled";
    public static final String UNAUTHORIZED_CALLER = "unauthorized_caller";
    public static final String UNSUPPORTED_CHAIN = "unsupported_chain";
    public static final String CONTRACT_NOT_CONFIGURED = "contract_not_configured";
    public static final String FACILITATOR_NOT_REGISTERED = "facilitator_not_registered";
    public static final String SUBSCRIPTION_NOT_FOUND = "subscription_not_found";
    public static final String SUB_NOT_FOUND = "sub_not_found";

    // Retryable errors
    public static final String PERIOD_NOT_DUE = "period_not_due";
    public static final String CHARGE_IN_FLIGHT = "charge_in_flight";
    public static final String CHANGE_IN_FLIGHT = "change_in_flight";
    public static final String CREATE_IN_FLIGHT = "create_in_flight";
    public static final String CANCEL_IN_FLIGHT = "cancel_in_flight";
    public static final String CANCEL_PENDING_CHANGE_IN_FLIGHT =
            "cancel_pending_change_in_flight";
    public static final String FINALIZE_IN_FLIGHT = "finalize_in_flight";
    public static final String LOCK_ACQUIRE_INTERRUPTED = "lock_acquire_interrupted";
    public static final String ON_CHAIN_SIMULATION_ERROR = "on_chain_simulation_error";
    public static final String SYSTEM_ERROR = "system_error";

    // Access-gate errors (Seller SDK layer; surfaced as 402 + PAYMENT-REQUIRED)
    /** Buyer's planId is not in the route's accepts list. */
    public static final String PLAN_MISMATCH = "plan_mismatch";
    /** Wall-clock period has outrun the paid period (lastChargedPeriod < elapsedPeriods). */
    public static final String SUBSCRIPTION_PERIOD_UNPAID = "subscription_period_unpaid";
    /** Merchant on_before_access hook denied the request (e.g. seller-canceled subscription). */
    public static final String ACCESS_DENIED_BY_MERCHANT = "access_denied_by_merchant";

    // PeriodMode errors (mirror SA)
    public static final String PERIOD_MODE_INVALID = "period_mode_invalid";
    public static final String PERIOD_SEC_NOT_ALLOWED = "period_sec_not_allowed";
    public static final String PERIOD_MODE_MISMATCH = "period_mode_mismatch";


    /** Request itself is invalid — retrying unchanged cannot succeed. */
    private static final Set<String> LOCAL_REJECT_CODES = Set.of(
            MISSING_REQUIRED_TERMS_FIELDS, INVALID_ADDRESS_FORMAT, INVALID_BYTES32,
            AMOUNT_PER_PERIOD_INVALID, PERIOD_SEC_INVALID, MAX_PERIODS_INVALID,
            PLAN_TIER_INVALID, START_AT_IN_PAST, START_AT_MISMATCH,
            PERMIT_SPENDER_MISMATCH, PERMIT_HASH_MISMATCH, TOKEN_MISMATCH,
            ALLOWANCE_INSUFFICIENT, ALLOWANCE_EXPIRED,
            CREATE_MUST_HAVE_ZERO_CHANGE_FROM, CREATE_MUST_HAVE_NONE_CHANGE_EFFECTIVE_AT,
            CHANGE_MUST_HAVE_NONZERO_CHANGE_FROM, CHANGE_MUST_HAVE_NON_NONE_EFFECTIVE_AT,
            CHANGE_EFFECTIVE_AT_MISMATCH, CHANGE_FROM_SUB_ID_MISMATCH,
            PAYER_MISMATCH, MERCHANT_MISMATCH, FACILITATOR_MISMATCH, PERIOD_SEC_MISMATCH,
            TERMS_EXPIRED, SIG_EXPIRED,
            TERMS_SIGNATURE_INVALID, TERMS_BINDING_INVALID, PERMIT_SIGNATURE_INVALID,
            SIGNATURE_HIGH_S, SIGNATURE_RECOVERY_FAILED,
            INITIAL_CHARGE_PERIODS_EXCEED_MAX, INITIAL_CHARGE_AMOUNT_EXCEEDS_CAP,
            PERIOD_MODE_INVALID, PERIOD_SEC_NOT_ALLOWED, PERIOD_MODE_MISMATCH,
            CANCEL_AUTH_REQUIRED, CANCEL_SUB_ID_MISMATCH, CANCEL_DEADLINE_EXPIRED,
            CANCEL_SIGNATURE_INVALID, PENDING_CANCEL_SUB_ID_MISMATCH,
            PENDING_CANCEL_DEADLINE_EXPIRED, PENDING_CANCEL_SIGNATURE_INVALID,
            PENDING_CANCEL_TARGET_MISMATCH, SALT_ALREADY_USED, INITIAL_CHARGE_MISMATCH,
            PARAM_MISMATCH, TERMS_DEADLINE_PASSED, PERMIT_SIG_DEADLINE_PASSED,
            PERMIT_SIG_INVALID
    );

    private static final Set<String> BUSINESS_CODES = Set.of(
            TIER_SAME, PENDING_CHANGE_EXISTS, SUB_NOT_ACTIVE_FOR_CHANGE,
            ALL_PERIODS_CHARGED, NO_PENDING_CHANGE,
            NOT_ENDED, FEATURE_DISABLED, UNAUTHORIZED_CALLER,
            UNSUPPORTED_CHAIN, CONTRACT_NOT_CONFIGURED, FACILITATOR_NOT_REGISTERED
    );

    private static final Set<String> RETRYABLE_CODES = Set.of(
            PERIOD_NOT_DUE, CHARGE_IN_FLIGHT, CHANGE_IN_FLIGHT, CREATE_IN_FLIGHT,
            CANCEL_IN_FLIGHT, CANCEL_PENDING_CHANGE_IN_FLIGHT, FINALIZE_IN_FLIGHT,
            LOCK_ACQUIRE_INTERRUPTED, ON_CHAIN_SIMULATION_ERROR, SYSTEM_ERROR
    );

    /**
     * Local state has drifted from chain truth — resync the store row (detail refresh /
     * successor follow) instead of retrying or surfacing to the buyer. Matches
     * {@code SubscriptionService.handleChargeError}'s self-heal path.
     */
    private static final Set<String> SELF_HEAL_CODES = Set.of(
            SUBSCRIPTION_NOT_ACTIVE, SUBSCRIPTION_NOT_FOUND, SUB_NOT_FOUND
    );

    /**
     * Doc §6 spellings that differ from what SA's {@code X402SubscriptionErrorEnum} actually
     * emits. Keyed by the doc variant, valued by the canonical (SA implementation) code —
     * tolerated on input so the SDK keeps working whichever way SA resolves the drift.
     */
    private static final java.util.Map<String, String> DOC_ALIASES = java.util.Map.of(
            "create_must_have_zero_change_from_sub_id", CREATE_MUST_HAVE_ZERO_CHANGE_FROM,
            "create_must_have_none_change_effective_at", CREATE_MUST_HAVE_NONE_CHANGE_EFFECTIVE_AT,
            "change_must_have_nonzero_change_from_sub_id", CHANGE_MUST_HAVE_NONZERO_CHANGE_FROM,
            "change_must_have_non_none_effective_at", CHANGE_MUST_HAVE_NON_NONE_EFFECTIVE_AT,
            "change_from_sub_id_mismatch", CHANGE_FROM_SUB_ID_MISMATCH,
            "cancel_sub_id_mismatch", CANCEL_SUB_ID_MISMATCH,
            "pending_cancel_sub_id_mismatch", PENDING_CANCEL_SUB_ID_MISMATCH,
            "no_pending_change", NO_PENDING_CHANGE
    );

    /** Map a doc-§6 alias to its canonical SA-implementation code; pass through otherwise. */
    public static String canonicalize(String code) {
        if (code == null) return null;
        return DOC_ALIASES.getOrDefault(code, code);
    }

    public static Category classify(String code) {
        if (code == null) return Category.BUSINESS;
        code = canonicalize(code);
        if (LOCAL_REJECT_CODES.contains(code)) return Category.LOCAL_REJECT;
        if (RETRYABLE_CODES.contains(code)) return Category.RETRYABLE;
        if (SELF_HEAL_CODES.contains(code)) return Category.SELF_HEAL;
        return Category.BUSINESS;
    }
}
