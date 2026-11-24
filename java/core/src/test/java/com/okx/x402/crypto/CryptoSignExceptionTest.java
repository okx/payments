// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.crypto;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

class CryptoSignExceptionTest {

    @Test
    void messageOnlyConstructor() {
        CryptoSignException ex = new CryptoSignException("test error");
        assertEquals("test error", ex.getMessage());
        assertNull(ex.getCause());
    }

    @Test
    void messageAndCauseConstructor() {
        RuntimeException cause = new RuntimeException("root cause");
        CryptoSignException ex = new CryptoSignException("wrapped", cause);
        assertEquals("wrapped", ex.getMessage());
        assertEquals(cause, ex.getCause());
    }
}
