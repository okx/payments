// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.resp;

public class ChangeResp {
    public String newSubId;
    public String txHash;
    /** Nullable so a missing field is detectable: the client rejects a response missing state. */
    public Integer state;
}
