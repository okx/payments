// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model;

/**
 * Represents an ERC-3009 transfer-with-authorization message.
 */
public class Authorization {

    /** The sender address. */
    public String from;

    /** The recipient address. */
    public String to;

    /** The transfer value. */
    public String value;

    /** The earliest timestamp the authorization is valid. */
    public String validAfter;

    /** The latest timestamp the authorization is valid. */
    public String validBefore;

    /** A unique nonce for replay protection. */
    public String nonce;

    /**
     * Default constructor.
     */
    public Authorization() {
    }
}
