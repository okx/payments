package com.okx.payments.mpp.seller;

import com.okx.payments.mpp.errors.InvalidPayloadError;
import com.okx.payments.mpp.protocol.Method;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.sa.SaApiClient;
import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.when;

class ChargeHandlerTest {

    private final SaApiClient sa = mock(SaApiClient.class);
    private final ChargeHandler handler = new ChargeHandler(sa);

    @Test
    void verify_transaction_calls_charge_settle() {
        Receipt.ChargeReceipt mock = new Receipt.ChargeReceipt(
            Method.EVM, "0xtx", "success", "ts", 196L, "challenge-id", null);
        when(sa.chargeSettle(any())).thenReturn(mock);
        Receipt.ChargeReceipt r = handler.verifyTransaction(Map.of(), "eip-3009");
        assertThat(r.reference()).isEqualTo("0xtx");
    }

    @Test
    void verify_transaction_rejects_delegation_authorization() {
        assertThatThrownBy(() -> handler.verifyTransaction(Map.of(), "delegation"))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("Phase 2");
    }

    @Test
    void verify_hash_calls_charge_verify_hash() {
        Receipt.ChargeReceipt mock = new Receipt.ChargeReceipt(
            Method.EVM, "0xclient", "success", "ts", 196L, null, null);
        when(sa.chargeVerifyHash(any())).thenReturn(mock);
        Receipt.ChargeReceipt r = handler.verifyHash(Map.of());
        assertThat(r.reference()).isEqualTo("0xclient");
    }
}
