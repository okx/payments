// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v2;

import java.util.Map;

/** Response from facilitator /settle endpoint (V2). */
public class SettleResponse {
    /** Whether settlement succeeded. */
    public boolean success;
    /** Machine-readable error reason. */
    public String errorReason;
    /** Human-readable error message. */
    public String errorMessage;
    /** Payer wallet address. */
    public String payer;
    /** Transaction hash (was txHash in V1). */
    public String transaction;
    /** CAIP-2 network (was networkId in V1). */
    public String network;
    /** Actual settled amount (for upto scheme). */
    public String amount;
    /** OKX extension: "pending"/"success"/"timeout". */
    public String status;
    /** Optional extensions. */
    public Map<String, Object> extensions;
}
