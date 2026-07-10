// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.crypto;

import com.okx.x402.model.v2.PaymentRequirements;

import org.junit.jupiter.api.Test;

import java.util.LinkedHashMap;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Covers the {@code OKXEvmSigner.signPaymentRequirements} dispatch logic
 * added for the Permit2 schemes: {@code exact + permit2} and
 * {@code upto + permit2}. The hash construction itself is exercised
 * implicitly via deterministic invariants on the returned payload.
 */
class OKXEvmSignerPermit2Test {

    // Anvil-style test key (well-known, never used for real funds)
    private static final String PRIVATE_KEY =
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    private static final String PAY_TO = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
    private static final String USDT = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
    private static final String FACILITATOR = "0x7031aa09e51501e86f0de032150060d9504c463a";

    private static PaymentRequirements baseReq(String scheme, Map<String, Object> extra) {
        PaymentRequirements r = new PaymentRequirements();
        r.scheme = scheme;
        r.network = "eip155:196";
        r.asset = USDT;
        r.amount = "1";
        r.payTo = PAY_TO;
        r.maxTimeoutSeconds = 60;
        r.extra = extra;
        return r;
    }

    private static Map<String, Object> permit2Extra(boolean upto) {
        Map<String, Object> e = new LinkedHashMap<>();
        e.put("assetTransferMethod", "permit2");
        if (upto) {
            e.put("facilitatorAddress", FACILITATOR);
        }
        return e;
    }

    @Test
    void exactPermit2_producesPermit2AuthorizationField() throws Exception {
        OKXEvmSigner signer = new OKXEvmSigner(PRIVATE_KEY);
        PaymentRequirements req = baseReq("exact", permit2Extra(false));

        Map<String, Object> payload = signer.signPaymentRequirements(req);

        assertNotNull(payload.get("signature"));
        assertTrue(((String) payload.get("signature")).startsWith("0x"));
        assertFalse(payload.containsKey("authorization"),
                "Permit2 payloads must not carry EIP-3009 'authorization'");

        @SuppressWarnings("unchecked")
        Map<String, Object> permit2Auth =
                (Map<String, Object>) payload.get("permit2Authorization");
        assertNotNull(permit2Auth, "must emit permit2Authorization");

        assertEquals(signer.getAddress(), permit2Auth.get("from"));
        assertEquals(Permit2Constants.EXACT_PERMIT2_PROXY_ADDRESS,
                permit2Auth.get("spender"));

        @SuppressWarnings("unchecked")
        Map<String, Object> permitted =
                (Map<String, Object>) permit2Auth.get("permitted");
        assertEquals(USDT, permitted.get("token"));
        assertEquals("1", permitted.get("amount"));

        @SuppressWarnings("unchecked")
        Map<String, Object> witness =
                (Map<String, Object>) permit2Auth.get("witness");
        assertEquals(PAY_TO, witness.get("to"));
        assertEquals("0", witness.get("validAfter"));
        assertNull(witness.get("facilitator"),
                "exact-scheme witness must not contain facilitator");
    }

    @Test
    void uptoPermit2_witnessCarriesFacilitator() throws Exception {
        OKXEvmSigner signer = new OKXEvmSigner(PRIVATE_KEY);
        PaymentRequirements req = baseReq("upto", permit2Extra(true));

        Map<String, Object> payload = signer.signPaymentRequirements(req);

        @SuppressWarnings("unchecked")
        Map<String, Object> permit2Auth =
                (Map<String, Object>) payload.get("permit2Authorization");
        assertEquals(Permit2Constants.UPTO_PERMIT2_PROXY_ADDRESS,
                permit2Auth.get("spender"));

        @SuppressWarnings("unchecked")
        Map<String, Object> witness =
                (Map<String, Object>) permit2Auth.get("witness");
        assertEquals(FACILITATOR, witness.get("facilitator"));
        assertEquals(PAY_TO, witness.get("to"));
        assertEquals("0", witness.get("validAfter"));
    }

    @Test
    void uptoPermit2_missingFacilitatorAddress_fails() {
        OKXEvmSigner signer = new OKXEvmSigner(PRIVATE_KEY);
        // permit2 but no facilitatorAddress
        Map<String, Object> badExtra = new LinkedHashMap<>();
        badExtra.put("assetTransferMethod", "permit2");
        PaymentRequirements req = baseReq("upto", badExtra);

        CryptoSignException ex = assertThrows(CryptoSignException.class,
                () -> signer.signPaymentRequirements(req));
        assertTrue(ex.getMessage().contains("facilitatorAddress"),
                "should mention facilitatorAddress: " + ex.getMessage());
    }

    @Test
    void eip3009Still_works_whenAssetTransferMethodMissing() throws Exception {
        OKXEvmSigner signer = new OKXEvmSigner(PRIVATE_KEY);
        // EIP-3009 path: extra needs name+version, no assetTransferMethod
        Map<String, Object> extra = new LinkedHashMap<>();
        extra.put("name", "USD₮0");
        extra.put("version", "1");
        PaymentRequirements req = baseReq("exact", extra);

        Map<String, Object> payload = signer.signPaymentRequirements(req);

        assertNotNull(payload.get("authorization"),
                "EIP-3009 path must emit 'authorization' field");
        assertFalse(payload.containsKey("permit2Authorization"),
                "EIP-3009 path must not emit permit2Authorization");
    }

    @Test
    void permit2_differentInputs_produceDifferentSignatures() throws Exception {
        OKXEvmSigner signer = new OKXEvmSigner(PRIVATE_KEY);
        PaymentRequirements req = baseReq("exact", permit2Extra(false));

        Map<String, Object> a = signer.signPaymentRequirements(req);
        Map<String, Object> b = signer.signPaymentRequirements(req);

        // Random nonces → signatures must differ even with identical inputs.
        assertFalse(a.get("signature").equals(b.get("signature")),
                "random nonce should produce distinct signatures");
    }
}
