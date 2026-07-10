// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model;

public class PermitSingle {
    public PermitDetails details;
    public String spender;
    /**
     * uint256 as a decimal string (facilitator API wire format): a buyer may sign a
     * sigDeadline beyond 2^63-1 ("never expires" Permit2 convention), which a long cannot hold.
     */
    public String sigDeadline;
}
