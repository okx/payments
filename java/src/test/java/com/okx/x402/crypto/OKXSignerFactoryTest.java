package com.okx.x402.crypto;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

class OKXSignerFactoryTest {

    @Test
    void noConfigThrows() {
        assertThrows(IllegalArgumentException.class,
                () -> OKXSignerFactory.createOKXSigner(null));
    }

    @Test
    void emptyPrivateKeyThrows() {
        assertThrows(IllegalArgumentException.class,
                () -> OKXSignerFactory.createOKXSigner(
                        new OKXSignerFactory.OKXSignerConfig()));
    }

    @Test
    void blankPrivateKeyThrows() {
        assertThrows(IllegalArgumentException.class,
                () -> OKXSignerFactory.createOKXSigner(
                        new OKXSignerFactory.OKXSignerConfig().privateKey("")));
    }

    @Test
    void errorMessageSaysPrivateKey() {
        IllegalArgumentException ex = assertThrows(IllegalArgumentException.class,
                () -> OKXSignerFactory.createOKXSigner(null));
        assertTrue(ex.getMessage().contains("privateKey"),
                "Error should mention privateKey, got: " + ex.getMessage());
    }

    @Test
    void validPrivateKeyCreates() {
        // Well-known test private key
        String testKey = "0x4c0883a69102937d6231471b5dbb6204fe512961708279f78305b2a49a22b3e6";
        OKXEvmSigner signer = OKXSignerFactory.createOKXSigner(
                new OKXSignerFactory.OKXSignerConfig().privateKey(testKey));
        assertNotNull(signer);
        assertNotNull(signer.getAddress());
        assertTrue(signer.getAddress().startsWith("0x"));
    }
}
