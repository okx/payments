// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model;

public class CancelAuth {
    public int action;
    public String subId;
    public int initiator;
    public String nonce;
    public long deadline;
    public String signature;
}
