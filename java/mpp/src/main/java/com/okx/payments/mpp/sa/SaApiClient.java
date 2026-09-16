package com.okx.payments.mpp.sa;

import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.protocol.session.payload.SessionStatusResponse;

/**
 * Smart-account MPP API client — 7 endpoints under {@code /api/v6/pay/mpp/}.
 *
 * <p>Implementations MUST:
 * <ul>
 *   <li>attach OKX-AK auth headers (see {@link OkxAuth})</li>
 *   <li>unwrap the {@code {code, msg, data}} envelope and throw
 *       {@link com.okx.payments.mpp.errors.MppError} on non-zero {@code code}</li>
 *   <li>retain {@code endpoint + chainId + channelId} in any thrown error context</li>
 * </ul>
 */
public interface SaApiClient {

    Receipt.ChargeReceipt chargeSettle(Object credentialBody);

    Receipt.ChargeReceipt chargeVerifyHash(Object credentialBody);

    Receipt.SessionReceipt sessionOpen(Object credentialBody);

    Receipt.SessionReceipt sessionTopUp(Object credentialBody);

    Receipt.SessionReceipt sessionSettle(Object credentialBody);

    Receipt.SessionReceipt sessionClose(Object credentialBody);

    SessionStatusResponse sessionStatus(String channelId);
}
