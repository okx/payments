package com.okx.x402.crypto;

import com.okx.x402.model.v2.PaymentRequirements;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;

class OKXEvmSignerTest {

    // Well-known test private key (do NOT use in production)
    private static final String TEST_PRIVATE_KEY =
            "0x4c0883a69102937d6231471b5dbb6204fe512961708279f78305b2a49a22b3e6";
    // Known address for the above key
    private static final String EXPECTED_ADDRESS = "0xD0fE261e729D65e6bE2dC279d04e72fB39aa611C";

    private OKXEvmSigner signer;

    @BeforeEach
    void setup() {
        signer = new OKXEvmSigner(TEST_PRIVATE_KEY);
    }

    @Test
    void addressMatchesPrivateKey() {
        assertEquals(EXPECTED_ADDRESS, signer.getAddress());
    }

    @Test
    void addressWithout0xPrefix() {
        OKXEvmSigner signer2 = new OKXEvmSigner(TEST_PRIVATE_KEY.substring(2));
        assertEquals(EXPECTED_ADDRESS, signer2.getAddress());
    }

    @Test
    void signatureValid() throws CryptoSignException {
        PaymentRequirements req = new PaymentRequirements();
        req.scheme = "exact";
        req.network = "eip155:196";
        req.amount = "10000";
        req.payTo = "0x1234567890123456789012345678901234567890";
        req.maxTimeoutSeconds = 86400;
        req.asset = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
        req.extra = Map.of(
                "name", "USD\u20AE0",
                "version", "1",
                "transferMethod", "eip3009"
        );

        Map<String, Object> result = signer.signPaymentRequirements(req);

        assertNotNull(result);
        assertNotNull(result.get("signature"));
        String sig = (String) result.get("signature");
        assertTrue(sig.startsWith("0x"), "Signature should be 0x-prefixed");
        // 65 bytes = 130 hex chars + "0x" prefix
        // But BigInteger toHexString may have different length. Check it's reasonable.
        assertTrue(sig.length() >= 130, "Signature should be at least 65 bytes hex");

        @SuppressWarnings("unchecked")
        Map<String, Object> auth = (Map<String, Object>) result.get("authorization");
        assertNotNull(auth);
        assertEquals(EXPECTED_ADDRESS, auth.get("from"));
        assertEquals(req.payTo, auth.get("to"));
        assertEquals("10000", auth.get("value"));
        assertNotNull(auth.get("validAfter"));
        assertNotNull(auth.get("validBefore"));
        assertNotNull(auth.get("nonce"));

        String nonce = (String) auth.get("nonce");
        assertTrue(nonce.startsWith("0x"), "Nonce should be 0x-prefixed hex");
    }

    @Test
    void validBeforeUsesMaxTimeout() throws CryptoSignException {
        PaymentRequirements req = new PaymentRequirements();
        req.scheme = "exact";
        req.network = "eip155:196";
        req.amount = "10000";
        req.payTo = "0x1234567890123456789012345678901234567890";
        req.maxTimeoutSeconds = 3600;
        req.asset = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
        req.extra = Map.of("name", "USD\u20AE0", "version", "1", "transferMethod", "eip3009");

        Map<String, Object> result = signer.signPaymentRequirements(req);
        @SuppressWarnings("unchecked")
        Map<String, Object> auth = (Map<String, Object>) result.get("authorization");

        long validAfter = Long.parseLong((String) auth.get("validAfter"));
        long validBefore = Long.parseLong((String) auth.get("validBefore"));

        // validBefore - validAfter should be approximately maxTimeoutSeconds + 5
        long diff = validBefore - validAfter;
        assertTrue(diff >= 3600 && diff <= 3700,
                "validBefore - validAfter should be ~3605, got: " + diff);
    }

    @Test
    void extractChainIdWorks() {
        assertEquals(196, OKXEvmSigner.extractChainId("eip155:196"));
        assertEquals(195, OKXEvmSigner.extractChainId("eip155:195"));
        assertEquals(1, OKXEvmSigner.extractChainId("eip155:1"));
    }

    @Test
    void legacySignThrows() {
        assertThrows(CryptoSignException.class,
                () -> signer.sign(Map.of("test", "data")));
    }

    @Test
    void nullRequirementsThrows() {
        assertThrows(CryptoSignException.class,
                () -> signer.signPaymentRequirements(null));
    }

    @Test
    void nullExtraThrows() {
        PaymentRequirements req = new PaymentRequirements();
        req.network = "eip155:196";
        req.payTo = "0x1234567890123456789012345678901234567890";
        req.amount = "10000";
        req.extra = null;

        CryptoSignException ex = assertThrows(CryptoSignException.class,
                () -> signer.signPaymentRequirements(req));
        assertTrue(ex.getMessage().contains("name"));
    }

    @Test
    void missingNameInExtraThrows() {
        PaymentRequirements req = new PaymentRequirements();
        req.network = "eip155:196";
        req.payTo = "0x1234567890123456789012345678901234567890";
        req.amount = "10000";
        req.extra = Map.of("version", "1");

        assertThrows(CryptoSignException.class,
                () -> signer.signPaymentRequirements(req));
    }

    @Test
    void nullPayToThrows() {
        PaymentRequirements req = new PaymentRequirements();
        req.network = "eip155:196";
        req.payTo = null;
        req.amount = "10000";
        req.extra = Map.of("name", "USD\u20AE0", "version", "1");

        assertThrows(CryptoSignException.class,
                () -> signer.signPaymentRequirements(req));
    }

    @Test
    void invalidNetworkFormatThrows() {
        assertThrows(IllegalArgumentException.class,
                () -> OKXEvmSigner.extractChainId("eip155"));
        assertThrows(IllegalArgumentException.class,
                () -> OKXEvmSigner.extractChainId(null));
    }
}
