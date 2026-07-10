// SPDX-License-Identifier: Apache-2.0
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
        // Throw-away placeholder key with no value attached. The factory only
        // validates structural correctness (32-byte hex), so any well-formed
        // value works for this assertion.
        String testKey =
                "0x0000000000000000000000000000000000000000000000000000000000000001";
        OKXEvmSigner signer = OKXSignerFactory.createOKXSigner(
                new OKXSignerFactory.OKXSignerConfig().privateKey(testKey));
        assertNotNull(signer);
        assertNotNull(signer.getAddress());
        assertTrue(signer.getAddress().startsWith("0x"));
    }
}
