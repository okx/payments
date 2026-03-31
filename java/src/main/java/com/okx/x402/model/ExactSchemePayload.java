package com.okx.x402.model;

/**
 * Payload for the exact payment scheme.
 */
public class ExactSchemePayload {

    /** The cryptographic signature. */
    public String signature;

    /** The authorization details. */
    public Authorization authorization;

    /**
     * Default constructor.
     */
    public ExactSchemePayload() {
    }
}
