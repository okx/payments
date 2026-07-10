package com.okx.payments.mpp.seller;

import com.okx.payments.mpp.errors.InvalidPayloadError;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.sa.SaApiClient;

import java.util.Objects;

/**
 * Charge intent handler — dispatches transaction vs hash, rejects delegation (P2).
 */
public final class ChargeHandler {

    private final SaApiClient sa;

    public ChargeHandler(SaApiClient sa) {
        this.sa = Objects.requireNonNull(sa);
    }

    public Receipt.ChargeReceipt verifyTransaction(Object credentialBody, String authorizationType) {
        if ("delegation".equalsIgnoreCase(authorizationType)) {
            throw new InvalidPayloadError(
                "delegation authorization is reserved for Phase 2; SDK currently rejects");
        }
        return sa.chargeSettle(credentialBody);
    }

    public Receipt.ChargeReceipt verifyHash(Object credentialBody) {
        return sa.chargeVerifyHash(credentialBody);
    }
}
