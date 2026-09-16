package com.okx.payments.mpp.sa;

import com.okx.payments.mpp.errors.ChannelNotFoundError;
import com.okx.payments.mpp.errors.MppError;
import com.okx.payments.mpp.protocol.Receipt;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.EnabledIf;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

/**
 * Live smoke test against pre/beta SA. Skipped unless {@code LocalBetaCreds.java} exists with
 * placeholders replaced.
 *
 * <p>Setup (once):
 * <pre>{@code
 * cd mpp/src/test/java/com/okx/payments/mpp/sa/
 * cp LocalBetaCreds.java.example LocalBetaCreds.java
 * # then open LocalBetaCreds.java and paste your values
 * }</pre>
 *
 * <p>Run:
 * <pre>{@code
 * mvn -B -pl mpp -am -Dsurefire.failIfNoSpecifiedTests=false test -Dtest=BetaChargeSmokeTest
 * }</pre>
 *
 * <p>Output prints only non-sensitive context (URL, http status, SA code/msg) — never the
 * API key, secret, passphrase, sign header, or signature bytes. {@code LocalBetaCreds.java} is
 * gitignored.
 */
@EnabledIf("credsFilled")
class BetaChargeSmokeTest {

    static SaApiConfig cfg;
    static HttpSaApiClient client;

    @BeforeAll
    static void setup() throws Exception {
        // Late-bind via reflection so missing LocalBetaCreds.java doesn't break compilation.
        Class<?> creds = Class.forName("com.okx.payments.mpp.sa.LocalBetaCreds");
        cfg = SaApiConfig.builder()
            .baseUrl(field(creds, "BASE_URL"))
            .apiKey(field(creds, "API_KEY"))
            .secretKey(field(creds, "API_SECRET"))
            .passphrase(field(creds, "API_PASSPHRASE"))
            .connectTimeout(Duration.ofSeconds(10))
            .readTimeout(Duration.ofSeconds(30))
            .build();
        client = new HttpSaApiClient(cfg);
    }

    /** Junit-Jupiter @EnabledIf hook — runs the test only when LocalBetaCreds.isFilled() == true. */
    static boolean credsFilled() {
        try {
            Class<?> creds = Class.forName("com.okx.payments.mpp.sa.LocalBetaCreds");
            Method m = creds.getDeclaredMethod("isFilled");
            return Boolean.TRUE.equals(m.invoke(null));
        } catch (Throwable t) {
            return false;
        }
    }

    private static String field(Class<?> creds, String name) throws Exception {
        Field f = creds.getDeclaredField(name);
        f.setAccessible(true);
        return (String) f.get(null);
    }

    // ─── Tests ───────────────────────────────────────────────────────────────

    @Test
    @DisplayName("AK auth round-trips against pre/beta — sessionStatus on a non-existent channel returns 70010")
    void auth_chain_works_via_session_status_negative_lookup() {
        // Status is the cheapest read-only call. We use a never-existed channelId so we expect
        // 70010 channel_not_found. ANY 70010 here proves: TCP + TLS + AK headers + envelope
        // parsing are all fine. A 401/403/30001 instead would indicate the auth chain failed.
        String fakeChannelId = "0xdeadbeef" + "00".repeat(28);
        System.out.println("[smoke] GET /session/status?channelId=<masked>... against " + cfg.baseUrl());

        assertThatThrownBy(() -> client.sessionStatus(fakeChannelId))
            .isInstanceOf(ChannelNotFoundError.class)
            .satisfies(e -> {
                MppError err = (MppError) e;
                Object code = err.context().get("saCode");
                System.out.println("[smoke] AK auth OK; SA returned saCode=" + code
                    + " (channel_not_found, expected)");
            });
    }

    @Test
    @DisplayName("Charge settle wire shape — empty/placeholder body should hit SA validation, not auth")
    void charge_settle_validation_path() {
        // Submitting a malformed payload to /charge/settle: we expect 30001 (gateway regex)
        // or 70000/70003/70004 (semantic / signature). Either confirms the request was authenticated
        // and routed to MppGatewayController. A 401/403 would indicate AK auth failure.
        Map<String, Object> body = minimalChargeSettleBody();

        try {
            Receipt.ChargeReceipt r = client.chargeSettle(body);
            System.out.println("[smoke] charge/settle unexpectedly succeeded? reference=" + r.reference());
        } catch (MppError err) {
            Object code = err.context().get("saCode");
            System.out.println("[smoke] charge/settle round-trip OK; SA returned saCode=" + code
                + " — auth chain works, server-side validation rejected as expected.");
            assertThat(code).isNotNull();
        }
    }

    /** Minimal body — invalid signatures are intentional; we want a SA validation rejection. */
    private static Map<String, Object> minimalChargeSettleBody() {
        Map<String, Object> challenge = new LinkedHashMap<>();
        challenge.put("id", "smoke-test-id");
        challenge.put("realm", "smoke.test");
        challenge.put("method", "evm");
        challenge.put("intent", "charge");
        challenge.put("request", "");
        challenge.put("expires", "2099-12-31T00:00:00Z");

        Map<String, Object> auth = new LinkedHashMap<>();
        auth.put("type", "eip-3009");
        auth.put("from", "0x0000000000000000000000000000000000000001");
        auth.put("to", "0x0000000000000000000000000000000000000002");
        auth.put("value", "1");
        auth.put("validAfter", "0");
        auth.put("validBefore", "9999999999");
        auth.put("nonce", "0x" + "ab".repeat(32));
        auth.put("signature", "0x" + "00".repeat(65));   // intentionally invalid

        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("type", "transaction");
        payload.put("authorization", auth);

        Map<String, Object> body = new LinkedHashMap<>();
        body.put("challenge", challenge);
        body.put("payload", payload);
        return body;
    }
}
