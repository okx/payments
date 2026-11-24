// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.error;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.ValueSource;

import static org.junit.jupiter.api.Assertions.*;

class SubscriptionErrorCodesTest {

    @ParameterizedTest
    @ValueSource(strings = {
            "missing_required_terms_fields", "invalid_address_format", "invalid_bytes32",
            "permit_spender_mismatch", "permit_hash_mismatch", "token_mismatch",
            "signature_high_s"
    })
    void localRejectClassification(String code) {
        assertEquals(SubscriptionErrorCodes.Category.LOCAL_REJECT,
                SubscriptionErrorCodes.classify(code));
    }

    @ParameterizedTest
    @ValueSource(strings = {
            "tier_same", "pending_change_exists",
            "feature_disabled", "unauthorized_caller"
    })
    void businessClassification(String code) {
        assertEquals(SubscriptionErrorCodes.Category.BUSINESS,
                SubscriptionErrorCodes.classify(code));
    }

    /** Store drifted from chain truth → resync, don't retry or surface. */
    @ParameterizedTest
    @ValueSource(strings = {
            "subscription_not_active", "subscription_not_found", "sub_not_found"
    })
    void selfHealClassification(String code) {
        assertEquals(SubscriptionErrorCodes.Category.SELF_HEAL,
                SubscriptionErrorCodes.classify(code));
    }

    @ParameterizedTest
    @ValueSource(strings = {"period_not_due", "charge_in_flight", "change_in_flight"})
    void retryableClassification(String code) {
        assertEquals(SubscriptionErrorCodes.Category.RETRYABLE,
                SubscriptionErrorCodes.classify(code));
    }

    @Test
    void unknownCodeDefaultsToBusiness() {
        assertEquals(SubscriptionErrorCodes.Category.BUSINESS,
                SubscriptionErrorCodes.classify("unknown_code"));
    }

    @Test
    void subscriptionExceptionCarriesCategory() {
        SubscriptionException ex = new SubscriptionException("charge_in_flight", "busy");
        assertEquals("charge_in_flight", ex.getCode());
        assertEquals(SubscriptionErrorCodes.Category.RETRYABLE, ex.getCategory());
        assertTrue(ex.isRetryable());
    }

    // ------------------------------------------------------------------
    // Code strings must match the spellings the facilitator emits verbatim —
    // eight of them also have an alternate documented spelling, and the
    // emitted spelling is the arbiter.
    // ------------------------------------------------------------------

    @Test
    void wireVisibleCodesMatchSaImplementation() {
        assertEquals("create_must_have_zero_changeFromSubId",
                SubscriptionErrorCodes.CREATE_MUST_HAVE_ZERO_CHANGE_FROM);
        assertEquals("create_must_have_none_changeEffectiveAt",
                SubscriptionErrorCodes.CREATE_MUST_HAVE_NONE_CHANGE_EFFECTIVE_AT);
        assertEquals("change_must_have_nonzero_changeFromSubId",
                SubscriptionErrorCodes.CHANGE_MUST_HAVE_NONZERO_CHANGE_FROM);
        assertEquals("change_must_have_non_none_effectiveAt",
                SubscriptionErrorCodes.CHANGE_MUST_HAVE_NON_NONE_EFFECTIVE_AT);
        assertEquals("changeFromSubId_mismatch",
                SubscriptionErrorCodes.CHANGE_FROM_SUB_ID_MISMATCH);
        assertEquals("cancel_subId_mismatch", SubscriptionErrorCodes.CANCEL_SUB_ID_MISMATCH);
        assertEquals("pending_cancel_subId_mismatch",
                SubscriptionErrorCodes.PENDING_CANCEL_SUB_ID_MISMATCH);
        assertEquals("no_pending_change_or_not_pending",
                SubscriptionErrorCodes.NO_PENDING_CHANGE);
        assertEquals("terms_deadline_expired", SubscriptionErrorCodes.TERMS_EXPIRED);
        assertEquals("permit_sig_deadline_expired", SubscriptionErrorCodes.SIG_EXPIRED);
        assertEquals("initial_charge_periods_exceeds_max",
                SubscriptionErrorCodes.INITIAL_CHARGE_PERIODS_EXCEED_MAX);
        assertEquals("initial_charge_exceeds_limit",
                SubscriptionErrorCodes.INITIAL_CHARGE_AMOUNT_EXCEEDS_CAP);
    }

    /** Alternate documented spellings classify identically to the canonical emitted codes. */
    @ParameterizedTest
    @ValueSource(strings = {
            "create_must_have_zero_change_from_sub_id",
            "create_must_have_none_change_effective_at",
            "change_must_have_nonzero_change_from_sub_id",
            "change_must_have_non_none_effective_at",
            "change_from_sub_id_mismatch",
            "cancel_sub_id_mismatch",
            "pending_cancel_sub_id_mismatch"
    })
    void docAliasSpellingsClassifyAsLocalReject(String docSpelling) {
        assertEquals(SubscriptionErrorCodes.Category.LOCAL_REJECT,
                SubscriptionErrorCodes.classify(docSpelling));
    }

    @Test
    void docAliasNoPendingChangeIsBusinessAndCanonicalizes() {
        assertEquals(SubscriptionErrorCodes.Category.BUSINESS,
                SubscriptionErrorCodes.classify("no_pending_change"));
        assertEquals("no_pending_change_or_not_pending",
                SubscriptionErrorCodes.canonicalize("no_pending_change"));
        // Non-alias codes pass through untouched.
        assertEquals("subscription_not_active",
                SubscriptionErrorCodes.canonicalize("subscription_not_active"));
    }

    /** Exceptions built from an alternate documented spelling expose the canonical code. */
    @Test
    void subscriptionExceptionCanonicalizesDocAlias() {
        SubscriptionException ex =
                new SubscriptionException("cancel_sub_id_mismatch", "boom");
        assertEquals("cancel_subId_mismatch", ex.getCode());
        assertEquals(SubscriptionErrorCodes.Category.LOCAL_REJECT, ex.getCategory());
    }

    /** The facilitator's in-flight / lock / simulation codes are transient — retry, don't give up. */
    @ParameterizedTest
    @ValueSource(strings = {
            "create_in_flight", "cancel_in_flight", "cancel_pending_change_in_flight",
            "finalize_in_flight", "lock_acquire_interrupted", "on_chain_simulation_error"
    })
    void saTransientCodesAreRetryable(String code) {
        assertEquals(SubscriptionErrorCodes.Category.RETRYABLE,
                SubscriptionErrorCodes.classify(code));
    }

    /**
     * Reachable facilitator rejection codes absent from the facilitator API reference must not
     * fall into BUSINESS.
     */
    @ParameterizedTest
    @ValueSource(strings = {
            "salt_already_used", "initial_charge_mismatch", "pending_cancel_target_mismatch",
            "param_mismatch", "terms_deadline_passed", "permit_sig_deadline_passed",
            "permit_sig_invalid"
    })
    void saUndocumentedRejectionCodesAreLocalReject(String code) {
        assertEquals(SubscriptionErrorCodes.Category.LOCAL_REJECT,
                SubscriptionErrorCodes.classify(code));
    }

    @ParameterizedTest
    @ValueSource(strings = {
            "terms_deadline_expired", "permit_sig_deadline_expired",
            "terms_signature_invalid", "permit_signature_invalid",
            "allowance_insufficient", "allowance_expired",
            "start_at_in_past", "start_at_mismatch",
            "period_mode_invalid", "period_sec_not_allowed", "period_mode_mismatch",
            "cancel_signature_invalid", "pending_cancel_signature_invalid"
    })
    void saDocValidationCodesAreLocalReject(String code) {
        assertEquals(SubscriptionErrorCodes.Category.LOCAL_REJECT,
                SubscriptionErrorCodes.classify(code));
    }

    @ParameterizedTest
    @ValueSource(strings = {
            "unsupported_chain", "contract_not_configured", "facilitator_not_registered"
    })
    void saDocInfraCodesAreBusiness(String code) {
        assertEquals(SubscriptionErrorCodes.Category.BUSINESS,
                SubscriptionErrorCodes.classify(code));
    }

    @Test
    void systemErrorIsRetryable() {
        assertEquals(SubscriptionErrorCodes.Category.RETRYABLE,
                SubscriptionErrorCodes.classify("system_error"));
    }
}
