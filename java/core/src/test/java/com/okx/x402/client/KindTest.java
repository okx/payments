// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.client;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

class KindTest {

    @Test
    void defaultConstructor() {
        Kind k = new Kind();
        assertNull(k.scheme);
        assertNull(k.network);
    }

    @Test
    void parameterizedConstructor() {
        Kind k = new Kind("exact", "eip155:196");
        assertEquals("exact", k.scheme);
        assertEquals("eip155:196", k.network);
    }
}
