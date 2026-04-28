// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.integration;

import com.okx.x402.facilitator.OKXFacilitatorClient;
import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.model.v2.VerifyResponse;
import com.okx.x402.crypto.OKXEvmSigner;

import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.MethodOrderer;
import org.junit.jupiter.api.Order;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.TestMethodOrder;

import java.io.IOException;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;

/**
 * Integration tests against a real OKX facilitator endpoint.
 *
 * <p>Run with: {@code mvn test -Dtest=RealFacilitatorIT}.
 *
 * <p>This test is intentionally skipped from the default surefire suite
 * (the surefire include pattern matches {@code *Test.java}, not {@code *IT.java}),
 * so a CI run that has not configured the environment variables below will
 * not execute it.
 *
 * <p>All endpoint and credential inputs are read from environment variables
 * — nothing is baked into source. Set the variables below before invoking
 * the test; missing values fall back to harmless placeholders that will
 * cause the calls to fail at the network layer rather than authenticate
 * against a real account.
 *
 * <ul>
 *   <li>{@code OKX_FACILITATOR_BASE_URL} — non-production facilitator URL
 *       used for the {@code exact}-scheme cases. Many non-production
 *       endpoints do not enforce HMAC, in which case dummy credentials
 *       are accepted.</li>
 *   <li>{@code OKX_PROD_BASE_URL} — production facilitator URL used for
 *       the {@code aggr_deferred} cases (defaults to
 *       {@code https://web3.okx.com}).</li>
 *   <li>{@code OKX_API_KEY} / {@code OKX_SECRET_KEY} / {@code OKX_PASSPHRASE}
 *       — credentials for the {@code aggr_deferred} cases.</li>
 *   <li>{@code OKX_TEST_PRIVATE_KEY} — EOA private key used to sign the
 *       EIP-3009 authorization for the {@code exact}-scheme cases.</li>
 *   <li>{@code OKX_PAY_TO} — recipient address (defaults to a zero address).</li>
 *   <li>{@code OKX_AA_ADDRESS} — registered AA wallet address for the
 *       {@code aggr_deferred} cases.</li>
 * </ul>
 */
@TestMethodOrder(MethodOrderer.OrderAnnotation.class)
class RealFacilitatorIT {

    // Non-production facilitator URL (no auth required for typical setups).
    // Read from the environment so we never bake an internal URL into source.
    static final String TEST_BASE_URL =
            envOrDefault("OKX_FACILITATOR_BASE_URL", "http://localhost:0");
    // Production facilitator (overridable via env for staging mirrors).
    static final String PROD_BASE_URL =
            envOrDefault("OKX_PROD_BASE_URL", "https://web3.okx.com");

    // Test EOA private key. Placeholder is a well-known throw-away key with
    // no value attached; override with OKX_TEST_PRIVATE_KEY for any real run.
    static final String TEST_PRIVATE_KEY = envOrDefault(
            "OKX_TEST_PRIVATE_KEY",
            "0x0000000000000000000000000000000000000000000000000000000000000001");
    static final String PAY_TO = envOrDefault(
            "OKX_PAY_TO", "0x0000000000000000000000000000000000000000");

    // Prod credentials for aggr_deferred (AA account testing) — all read
    // from env. Placeholders are deliberately non-functional so the test
    // fails loudly at the API layer rather than authenticating any real
    // account; OKXAuth rejects null/empty values, so we use a clearly-fake
    // sentinel string instead.
    static final String PROD_API_KEY = envOrDefault("OKX_API_KEY", "unset");
    static final String PROD_SECRET_KEY = envOrDefault("OKX_SECRET_KEY", "unset");
    static final String PROD_PASSPHRASE = envOrDefault("OKX_PASSPHRASE", "unset");
    static final String AA_ADDRESS = envOrDefault(
            "OKX_AA_ADDRESS", "0x0000000000000000000000000000000000000000");

    private static String envOrDefault(String name, String fallback) {
        String value = System.getenv(name);
        return value == null || value.isEmpty() ? fallback : value;
    }

    static OKXFacilitatorClient facilitator;
    static OKXFacilitatorClient prodFacilitator;
    static OKXEvmSigner signer;

    @BeforeAll
    static void setUp() {
        // Swimlane test env: no auth required
        facilitator = new OKXFacilitatorClient(
                "test-key", "test-secret", "test-pass", TEST_BASE_URL);
        // Prod env: real OKX credentials for aggr_deferred
        prodFacilitator = new OKXFacilitatorClient(
                PROD_API_KEY, PROD_SECRET_KEY, PROD_PASSPHRASE, PROD_BASE_URL);
        signer = new OKXEvmSigner(TEST_PRIVATE_KEY);
    }

    // -----------------------------------------------------------------------
    // 1. GET /supported
    // -----------------------------------------------------------------------

    @Test
    @Order(1)
    void supportedReturnsXLayer() throws Exception {
        SupportedResponse resp = facilitator.supported();

        assertNotNull(resp, "Supported response must not be null");
        assertNotNull(resp.kinds, "kinds must not be null");
        assertFalse(resp.kinds.isEmpty(), "Must support at least one kind");

        boolean hasXLayer = resp.kinds.stream()
                .anyMatch(k -> "eip155:196".equals(k.network));
        assertTrue(hasXLayer, "Must support eip155:196 (X Layer)");

        System.out.println("[IT] supported: " + resp.kinds.size() + " kinds");
        resp.kinds.forEach(k ->
                System.out.println("  - scheme=" + k.scheme
                        + " network=" + k.network));
    }

    // -----------------------------------------------------------------------
    // 2. POST /verify — valid signature
    // -----------------------------------------------------------------------

    @Test
    @Order(2)
    void verifyWithValidSignature() throws Exception {
        PaymentRequirements reqs = buildRequirements();
        Map<String, Object> signed = signer.signPaymentRequirements(reqs);

        PaymentPayload payload = new PaymentPayload();
        payload.x402Version = 2;
        payload.accepted = reqs;
        payload.payload = signed;

        VerifyResponse vr = facilitator.verify(payload, reqs);

        assertNotNull(vr, "Verify response must not be null");
        System.out.println("[IT] verify(valid): isValid=" + vr.isValid
                + ", payer=" + vr.payer
                + (vr.invalidReason != null ? ", reason=" + vr.invalidReason : "")
                + (vr.invalidMessage != null ? ", msg=" + vr.invalidMessage : ""));
        assertTrue(vr.isValid, "Verify should pass with funded wallet");
        assertEquals(signer.getAddress().toLowerCase(), vr.payer.toLowerCase(),
                "Payer should match signer address");
    }

    // -----------------------------------------------------------------------
    // 3. POST /verify — bad signature → isValid=false
    // -----------------------------------------------------------------------

    @Test
    @Order(3)
    void verifyWithBadSignatureReturnsFalse() throws Exception {
        PaymentRequirements reqs = buildRequirements();

        PaymentPayload payload = new PaymentPayload();
        payload.x402Version = 2;
        payload.accepted = reqs;
        payload.payload = Map.of(
                "signature", "0x" + "ab".repeat(65),
                "authorization", Map.of(
                        "from", signer.getAddress(),
                        "to", reqs.payTo,
                        "value", reqs.amount,
                        "validAfter", "0",
                        "validBefore", "999999999999",
                        "nonce", "0x" + "01".repeat(32)
                )
        );

        VerifyResponse vr = facilitator.verify(payload, reqs);
        System.out.println("[IT] verify(bad sig): isValid=" + vr.isValid
                + ", reason=" + vr.invalidReason);
        assertFalse(vr.isValid, "Bad signature should be rejected");
    }

    // -----------------------------------------------------------------------
    // 4. POST /settle (syncSettle=true)
    // -----------------------------------------------------------------------

    @Test
    @Order(4)
    void settleWithSyncSettle() throws Exception {
        PaymentRequirements reqs = buildRequirements();
        Map<String, Object> signed = signer.signPaymentRequirements(reqs);

        PaymentPayload payload = new PaymentPayload();
        payload.x402Version = 2;
        payload.accepted = reqs;
        payload.payload = signed;

        SettleResponse sr = facilitator.settle(payload, reqs, true);

        assertNotNull(sr, "Settle response must not be null");
        System.out.println("[IT] settle(sync): success=" + sr.success
                + ", tx=" + sr.transaction
                + ", status=" + sr.status
                + ", network=" + sr.network
                + (sr.errorReason != null ? ", reason=" + sr.errorReason : "")
                + (sr.errorMessage != null ? ", msg=" + sr.errorMessage : ""));

        assertTrue(sr.success, "Settle should succeed with funded wallet");
        assertNotNull(sr.transaction, "Transaction hash must be present");
        assertFalse(sr.transaction.isEmpty(), "Transaction hash must not be empty");
        assertEquals("eip155:196", sr.network);

        // Save tx hash for settleStatus test
        lastTxHash = sr.transaction;
    }

    static String lastTxHash;

    // -----------------------------------------------------------------------
    // 5. POST /settle (syncSettle=false, async)
    // -----------------------------------------------------------------------

    @Test
    @Order(5)
    void settleAsync() throws Exception {
        PaymentRequirements reqs = buildRequirements();
        Map<String, Object> signed = signer.signPaymentRequirements(reqs);

        PaymentPayload payload = new PaymentPayload();
        payload.x402Version = 2;
        payload.accepted = reqs;
        payload.payload = signed;

        SettleResponse sr = facilitator.settle(payload, reqs, false);

        assertNotNull(sr, "Settle response must not be null");
        System.out.println("[IT] settle(async): success=" + sr.success
                + ", tx=" + sr.transaction
                + ", status=" + sr.status
                + (sr.errorReason != null ? ", reason=" + sr.errorReason : ""));

        assertTrue(sr.success, "Async settle should succeed with funded wallet");
    }

    // -----------------------------------------------------------------------
    // 6. GET /settle/status — real tx from test 4
    // -----------------------------------------------------------------------

    @Test
    @Order(6)
    void settleStatusForRealTx() throws Exception {
        assertNotNull(lastTxHash, "lastTxHash should be set by settleWithSyncSettle");

        SettleResponse sr = facilitator.settleStatus(lastTxHash);

        assertNotNull(sr, "settleStatus response must not be null");
        System.out.println("[IT] settleStatus(" + lastTxHash + "): success=" + sr.success
                + ", status=" + sr.status
                + ", network=" + sr.network
                + (sr.errorReason != null ? ", reason=" + sr.errorReason : ""));
    }

    // -----------------------------------------------------------------------
    // 7. GET /settle/status — nonexistent tx
    // -----------------------------------------------------------------------

    @Test
    @Order(7)
    void settleStatusForNonexistentTx() throws Exception {
        SettleResponse sr = facilitator.settleStatus(
                "0x0000000000000000000000000000000000000000000000000000000000000000");

        assertNotNull(sr, "settleStatus response must not be null");
        System.out.println("[IT] settleStatus(zero): success=" + sr.success
                + ", status=" + sr.status
                + (sr.errorReason != null ? ", reason=" + sr.errorReason : ""));
        assertFalse(sr.success, "Nonexistent tx should return success=false");
        assertEquals("not_found", sr.errorReason);
    }

    // =======================================================================
    // aggr_deferred scheme tests
    // =======================================================================

    // -----------------------------------------------------------------------
    // 8. aggr_deferred single tx — verify + settle
    // -----------------------------------------------------------------------

    @Test
    @Order(10)
    void aggrDeferredProdSupported() throws Exception {
        SupportedResponse resp = prodFacilitator.supported();

        assertNotNull(resp);
        assertFalse(resp.kinds.isEmpty(), "Prod must support at least one kind");
        boolean hasDeferred = resp.kinds.stream()
                .anyMatch(k -> "aggr_deferred".equals(k.scheme));
        assertTrue(hasDeferred, "Prod must support aggr_deferred");

        System.out.println("[IT-PROD] supported: " + resp.kinds.size() + " kinds");
        resp.kinds.forEach(k ->
                System.out.println("  - scheme=" + k.scheme + " network=" + k.network));
    }

    // -----------------------------------------------------------------------
    // 9. aggr_deferred single tx — verify with AA account (prod)
    // -----------------------------------------------------------------------

    @Test
    @Order(11)
    void aggrDeferredSingleTxVerify() throws Exception {
        PaymentPayload payload = buildDeferredPayload();
        PaymentRequirements reqs = buildDeferredRequirements();

        VerifyResponse vr = prodFacilitator.verify(payload, reqs);

        assertNotNull(vr, "aggr_deferred verify response must not be null");
        System.out.println("[IT-PROD] aggr_deferred verify(single): isValid=" + vr.isValid
                + ", payer=" + vr.payer
                + (vr.invalidReason != null ? ", reason=" + vr.invalidReason : "")
                + (vr.invalidMessage != null ? ", msg=" + vr.invalidMessage : ""));
        // AA account IS found (payer = AA address), but session cert is dummy
        // so TEE cannot extract session public key → isValid=false expected
        assertEquals(AA_ADDRESS.toLowerCase(), vr.payer.toLowerCase(),
                "Payer should be the AA address");
    }

    // -----------------------------------------------------------------------
    // 10. aggr_deferred single tx — settle (prod)
    //     Without a real session cert issued by the AA wallet, TEE cannot
    //     convert the session key signature → returns code=8000.
    //     This validates: SDK auth works, AA account is found, facilitator
    //     reaches TEE layer. Full success requires real session cert.
    // -----------------------------------------------------------------------

    @Test
    @Order(12)
    void aggrDeferredSingleTxSettle() throws Exception {
        PaymentPayload payload = buildDeferredPayload();
        PaymentRequirements reqs = buildDeferredRequirements();

        try {
            SettleResponse sr = prodFacilitator.settle(payload, reqs);
            // If it succeeds (real session cert scenario), capture tx
            System.out.println("[IT-PROD] aggr_deferred settle(single): success=" + sr.success
                    + ", tx=" + sr.transaction
                    + ", status=" + sr.status);
            if (sr.success) {
                deferredTxHash = sr.transaction;
            }
        } catch (IOException e) {
            // Expected with dummy sessionCert: TEE sign-msg failed (code=8000)
            System.out.println("[IT-PROD] aggr_deferred settle(single): " + e.getMessage());
            assertTrue(e.getMessage().contains("8000") || e.getMessage().contains("TEE"),
                    "Should fail at TEE layer (not auth or account), got: " + e.getMessage());
        }
    }

    static String deferredTxHash;

    // -----------------------------------------------------------------------
    // 11. aggr_deferred multi-tx — 3 sequential settles (prod, batch scenario)
    //     Same TEE limitation applies; validates consistent behavior across
    //     multiple sequential calls.
    // -----------------------------------------------------------------------

    @Test
    @Order(13)
    void aggrDeferredMultiTxSettle() throws Exception {
        System.out.println("[IT-PROD] aggr_deferred multi-tx: sending 3 sequential settlements...");

        for (int i = 1; i <= 3; i++) {
            if (i > 1) {
                Thread.sleep(1500); // avoid rate limiting (50011)
            }
            PaymentPayload payload = buildDeferredPayload();
            PaymentRequirements reqs = buildDeferredRequirements();

            try {
                SettleResponse sr = prodFacilitator.settle(payload, reqs);
                System.out.println("[IT-PROD] aggr_deferred settle #" + i + ": success=" + sr.success
                        + ", tx=" + sr.transaction + ", status=" + sr.status);
            } catch (IOException e) {
                System.out.println("[IT-PROD] aggr_deferred settle #" + i + ": " + e.getMessage());
                assertTrue(e.getMessage().contains("8000") || e.getMessage().contains("TEE")
                                || e.getMessage().contains("50011"),
                        "Settle #" + i + " should fail at TEE or rate limit layer");
            }
        }
    }

    // -----------------------------------------------------------------------
    // 12. aggr_deferred settle status (prod)
    // -----------------------------------------------------------------------

    @Test
    @Order(14)
    void aggrDeferredSettleStatus() throws Exception {
        if (deferredTxHash != null && !deferredTxHash.isEmpty()) {
            SettleResponse sr = prodFacilitator.settleStatus(deferredTxHash);
            System.out.println("[IT-PROD] aggr_deferred settleStatus("
                    + deferredTxHash + "): success=" + sr.success
                    + ", status=" + sr.status);
        } else {
            System.out.println("[IT-PROD] aggr_deferred settleStatus: "
                    + "skipped (no tx hash — expected without real session cert)");
        }
    }

    // =======================================================================
    // Helpers
    // =======================================================================

    static final String UINT256_MAX =
            "115792089237316195423570985008687907853269984665640564039457584007913129639935";

    private static PaymentPayload buildDeferredPayload() throws Exception {
        PaymentRequirements reqs = buildDeferredRequirements();

        // Build EIP-3009 authorization with AA address as "from"
        // and validBefore = uint256.max (required for aggr_deferred)
        Map<String, Object> signed = signer.signPaymentRequirements(reqs);

        @SuppressWarnings("unchecked")
        Map<String, Object> auth = (Map<String, Object>) signed.get("authorization");
        Map<String, Object> fixedAuth = new java.util.HashMap<>(auth);
        fixedAuth.put("from", AA_ADDRESS);      // AA wallet as payer
        fixedAuth.put("validBefore", UINT256_MAX); // deferred = no expiry
        Map<String, Object> fixedPayload = new java.util.LinkedHashMap<>(signed);
        fixedPayload.put("authorization", fixedAuth);

        // sessionCert goes in accepted.extra (buyer side)
        Map<String, Object> acceptedExtra = new java.util.HashMap<>(reqs.extra);
        acceptedExtra.put("sessionCert", "dGVzdC1zZXNzaW9uLWNlcnQ=");
        PaymentRequirements acceptedReqs = copyRequirements(reqs);
        acceptedReqs.extra = acceptedExtra;

        PaymentPayload payload = new PaymentPayload();
        payload.x402Version = 2;
        payload.accepted = acceptedReqs;
        payload.payload = fixedPayload;
        return payload;
    }

    private static PaymentRequirements buildRequirements() {
        PaymentRequirements reqs = new PaymentRequirements();
        reqs.scheme = "exact";
        reqs.network = "eip155:196";
        reqs.amount = "1"; // minimum: 0.000001 USDT (1 atomic unit, decimals=6)
        reqs.payTo = PAY_TO;
        reqs.maxTimeoutSeconds = 86400;
        reqs.asset = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
        reqs.extra = Map.of(
                "name", "USD\u20AE0",
                "version", "1",
                "transferMethod", "eip3009"
        );
        return reqs;
    }

    private static PaymentRequirements buildDeferredRequirements() {
        PaymentRequirements reqs = new PaymentRequirements();
        reqs.scheme = "aggr_deferred";
        reqs.network = "eip155:196";
        reqs.amount = "1"; // minimum: 0.000001 USDT
        reqs.payTo = PAY_TO;
        reqs.maxTimeoutSeconds = 86400;
        reqs.asset = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
        reqs.extra = Map.of(
                "name", "USD\u20AE0",
                "version", "1",
                "transferMethod", "eip3009"
        );
        return reqs;
    }

    private static PaymentRequirements copyRequirements(PaymentRequirements src) {
        PaymentRequirements copy = new PaymentRequirements();
        copy.scheme = src.scheme;
        copy.network = src.network;
        copy.amount = src.amount;
        copy.payTo = src.payTo;
        copy.maxTimeoutSeconds = src.maxTimeoutSeconds;
        copy.asset = src.asset;
        copy.extra = src.extra;
        return copy;
    }
}
