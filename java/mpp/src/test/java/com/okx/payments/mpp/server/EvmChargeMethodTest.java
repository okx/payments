package com.okx.payments.mpp.server;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.okx.payments.mpp.errors.InvalidPayloadError;
import com.okx.payments.mpp.protocol.Credential;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.Method;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.sa.SaApiClient;
import com.okx.payments.mpp.seller.ChargeHandler;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.times;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

class EvmChargeMethodTest {

    private final ObjectMapper mapper = new ObjectMapper();
    private SaApiClient sa;
    private EvmChargeMethod method;

    @BeforeEach
    void setup() {
        sa = mock(SaApiClient.class);
        method = new EvmChargeMethod(new ChargeHandler(sa));
    }

    @Test
    void transaction_payload_routes_to_charge_settle() throws Exception {
        Receipt.ChargeReceipt mock = new Receipt.ChargeReceipt(
            Method.EVM, "0xtx", "success", "ts", 196L, "ch-1", null);
        when(sa.chargeSettle(any())).thenReturn(mock);

        String body = "{\"type\":\"transaction\","
            + "\"authorization\":{\"type\":\"eip-3009\",\"from\":\"0xpayer\",\"to\":\"0xpayee\","
            + "\"value\":\"100\",\"validAfter\":\"0\",\"validBefore\":\"99999999\","
            + "\"nonce\":\"0xff\",\"signature\":\"0x" + "11".repeat(65) + "\"}}";
        Credential credential = credential(mapper.readTree(body));

        Receipt.ChargeReceipt r = method.verifyCharge(credential, expected("100"));
        assertThat(r.reference()).isEqualTo("0xtx");
        verify(sa, times(1)).chargeSettle(any());
        verify(sa, times(0)).chargeVerifyHash(any());
    }

    @Test
    void hash_payload_routes_to_verify_hash() throws Exception {
        Receipt.ChargeReceipt mock = new Receipt.ChargeReceipt(
            Method.EVM, "0xtx2", "success", "ts", 196L, "ch-2", null);
        when(sa.chargeVerifyHash(any())).thenReturn(mock);

        String body = "{\"type\":\"hash\",\"hash\":\"0xabc\"}";
        Credential credential = credential(mapper.readTree(body));

        Receipt.ChargeReceipt r = method.verifyCharge(credential, expected("100"));
        assertThat(r.reference()).isEqualTo("0xtx2");
        verify(sa, times(0)).chargeSettle(any());
        verify(sa, times(1)).chargeVerifyHash(any());
    }

    @Test
    void delegation_authorization_rejected() throws Exception {
        String body = "{\"type\":\"transaction\",\"authorization\":{\"type\":\"delegation\"}}";
        Credential credential = credential(mapper.readTree(body));
        assertThatThrownBy(() -> method.verifyCharge(credential, expected("100")))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("delegation");
    }

    @Test
    void unknown_payload_type_throws_invalid_payload() throws Exception {
        Credential credential = credential(mapper.readTree("{\"type\":\"weird\"}"));
        assertThatThrownBy(() -> method.verifyCharge(credential, expected("100")))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("unknown charge payload type: 'weird'");
    }

    @Test
    void missing_payload_throws_invalid_payload() {
        Credential credential = credential(null);
        assertThatThrownBy(() -> method.verifyCharge(credential, expected("100")))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("payload is missing");
    }
    private com.okx.payments.mpp.protocol.charge.ChargeRequest expected(String amount) {
        return new com.okx.payments.mpp.protocol.charge.ChargeRequest(amount, "fixture-token", "fixture-payee", null, null, null);
    }

    private Credential credential(com.fasterxml.jackson.databind.JsonNode payload) {
        String encoded = new com.okx.payments.mpp.protocol.encoding.Base64UrlJson(mapper).encode(expected("100"));
        return new Credential(new com.okx.payments.mpp.protocol.Challenge("fixture", "fixture", Method.EVM,
            Intent.CHARGE, encoded, null, null, null, null), payload, null);
    }

    @Test
    void low_price_credential_is_rejected_before_sa() throws Exception {
        Credential low = credential(mapper.readTree("{\"type\":\"hash\",\"hash\":\"fixture\"}"));
        assertThatThrownBy(() -> method.verifyCharge(low, expected("50000")))
            .isInstanceOf(com.okx.payments.mpp.errors.InvalidChallengeError.class);
        org.mockito.Mockito.verifyNoInteractions(sa);
    }

}
