package com.okx.x402.client;

import com.okx.x402.crypto.CryptoSignException;
import com.okx.x402.crypto.EvmSigner;
import com.okx.x402.model.v2.PaymentRequirements;
import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;

class OKXHttpClientTest {

    @Test
    void constructorAcceptsValidSigner() {
        EvmSigner mockSigner = new EvmSigner() {
            @Override
            public Map<String, Object> signPaymentRequirements(PaymentRequirements r) {
                return Map.of("signature", "0xtest");
            }
            @Override
            public String getAddress() {
                return "0xtest";
            }
        };

        OKXHttpClient client = new OKXHttpClient(mockSigner);
        assertNotNull(client);
    }

    @Test
    void constructorWithNetwork() {
        EvmSigner mockSigner = new EvmSigner() {
            @Override
            public Map<String, Object> signPaymentRequirements(PaymentRequirements r) {
                return Map.of("signature", "0xtest");
            }
            @Override
            public String getAddress() {
                return "0xtest";
            }
        };

        OKXHttpClient client = new OKXHttpClient(mockSigner, "eip155:195");
        assertNotNull(client);
    }

    @Test
    void nullSignerThrows() {
        assertThrows(NullPointerException.class, () -> new OKXHttpClient(null));
    }
}
