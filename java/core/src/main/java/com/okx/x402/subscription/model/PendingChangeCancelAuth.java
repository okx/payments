// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model;

public class PendingChangeCancelAuth {
    public String subId;
    /**
     * Target scheduled downgrade's new subId: binds the cancellation to one specific scheduled
     * downgrade, so it must equal the live pending change's newSubId (the contract answers
     * pending_cancel_target_mismatch otherwise).
     */
    public String newSubId;
    public String nonce;
    public long deadline;
    public String signature;
}
