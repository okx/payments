// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model;

import com.fasterxml.jackson.annotation.JsonInclude;

/**
 * Response header returned after payment settlement.
 */
@JsonInclude(JsonInclude.Include.ALWAYS)
public class SettlementResponseHeader {

    /** Whether the settlement was successful. */
    public boolean success;

    /** The transaction hash. */
    public String transaction;

    /** The network the settlement occurred on. */
    public String network;

    /** The address of the payer. */
    public String payer;

    /**
     * Default constructor.
     */
    public SettlementResponseHeader() {
    }

    /**
     * Constructs a settlement response header with all fields.
     *
     * @param success     whether the settlement was successful
     * @param transaction the transaction hash
     * @param network     the network identifier
     * @param payer       the payer address
     */
    public SettlementResponseHeader(boolean success, String transaction,
                                    String network, String payer) {
        this.success = success;
        this.transaction = transaction;
        this.network = network;
        this.payer = payer;
    }
}
