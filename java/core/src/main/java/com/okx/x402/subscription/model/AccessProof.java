// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model;

public class AccessProof {
    public String kind = "subscription-id";
    public String subId;
    public String payer;
    public long timestamp;
    public String signature;
}
