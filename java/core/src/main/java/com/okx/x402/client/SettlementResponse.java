// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.client;

/**
 * JSON returned by POST /settle on a V1 facilitator.
 */
public class SettlementResponse {
    /** Whether settlement succeeded. */
    public boolean success;

    /** Error message if settlement failed. */
    public String error;

    /** Transaction hash. */
    public String txHash;

    /** Network ID. */
    public String networkId;
}
