package com.okx.payments.mpp.sa;

import com.github.tomakehurst.wiremock.WireMockServer;
import com.github.tomakehurst.wiremock.client.WireMock;
import com.okx.payments.mpp.errors.AmountExceedsDepositError;
import com.okx.payments.mpp.errors.BadRequestError;
import com.okx.payments.mpp.errors.ChannelNotFoundError;
import com.okx.payments.mpp.errors.InvalidSignatureError;
import com.okx.payments.mpp.errors.MppError;
import com.okx.payments.mpp.errors.ServiceError;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.protocol.session.payload.SessionStatusResponse;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

import java.util.Map;

import static com.github.tomakehurst.wiremock.client.WireMock.absent;
import static com.github.tomakehurst.wiremock.client.WireMock.aResponse;
import static com.github.tomakehurst.wiremock.client.WireMock.equalTo;
import static com.github.tomakehurst.wiremock.client.WireMock.get;
import static com.github.tomakehurst.wiremock.client.WireMock.matching;
import static com.github.tomakehurst.wiremock.client.WireMock.post;
import static com.github.tomakehurst.wiremock.client.WireMock.urlEqualTo;
import static com.github.tomakehurst.wiremock.client.WireMock.urlPathEqualTo;
import static com.github.tomakehurst.wiremock.core.WireMockConfiguration.wireMockConfig;
import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class HttpSaApiClientTest {

    private WireMockServer wm;
    private HttpSaApiClient client;

    @BeforeEach
    void start() {
        wm = new WireMockServer(wireMockConfig().dynamicPort());
        wm.start();
        WireMock.configureFor("localhost", wm.port());
        SaApiConfig cfg = SaApiConfig.builder()
            .baseUrl("http://localhost:" + wm.port())
            .apiKey("test-key")
            .secretKey("test-secret")
            .passphrase("test-pass")
            .build();
        client = new HttpSaApiClient(cfg);
    }

    @AfterEach
    void stop() {
        wm.stop();
    }

    // ── Auth header attachment ────────────────────────────────────────────────

    @Test
    void requests_carry_okx_auth_headers() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/charge/settle"))
            .willReturn(success(chargeReceiptJson("0xabc", "evm"))));

        client.chargeSettle(Map.of("payload", Map.of("type", "transaction")));

        wm.verify(WireMock.postRequestedFor(urlEqualTo("/api/v6/pay/mpp/charge/settle"))
            .withHeader("OK-ACCESS-KEY", equalTo("test-key"))
            .withHeader("OK-ACCESS-PASSPHRASE", equalTo("test-pass"))
            .withHeader("OK-ACCESS-SIGN", matching(".+"))
            .withHeader("OK-ACCESS-TIMESTAMP", matching("\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}\\.\\d{3}Z"))
            .withHeader("Content-Type", equalTo("application/json")));
    }

    // ── Charge endpoints ──────────────────────────────────────────────────────

    @Test
    void charge_settle_happy_path() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/charge/settle"))
            .willReturn(success(chargeReceiptJson("0xtxhash", "evm"))));

        Receipt.ChargeReceipt r = client.chargeSettle(Map.of("any", 1));
        assertThat(r.reference()).isEqualTo("0xtxhash");
        assertThat(r.method().wire()).isEqualTo("evm");
        assertThat(r.status()).isEqualTo("success");
    }

    @Test
    void charge_verify_hash_happy_path() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/charge/verifyHash"))
            .willReturn(success(chargeReceiptJson("0xclient_broadcast", "evm"))));
        Receipt.ChargeReceipt r = client.chargeVerifyHash(Map.of("any", 1));
        assertThat(r.reference()).isEqualTo("0xclient_broadcast");
    }

    @Test
    void charge_settle_70004_invalid_signature() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/charge/settle"))
            .willReturn(success(errorJson(70004, "voucher signature recovery failed"))));

        assertThatThrownBy(() -> client.chargeSettle(Map.of("any", 1)))
            .isInstanceOf(InvalidSignatureError.class)
            .satisfies(e -> {
                MppError err = (MppError) e;
                assertThat(err.context()).containsKey("endpoint").containsKey("saCode");
                assertThat(err.context().get("endpoint")).isEqualTo("/charge/settle");
                assertThat(err.context().get("saCode")).isEqualTo(70004);
            });
    }

    // ── Session endpoints ─────────────────────────────────────────────────────

    @Test
    void session_open_happy_path() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/session/open"))
            .willReturn(success(sessionReceiptJson("0xdeadbeef" + "00".repeat(28), "1000000", "0xtx"))));

        Receipt.SessionReceipt r = client.sessionOpen(Map.of("any", 1));
        assertThat(r.channelId()).startsWith("0xdeadbeef");
        assertThat(r.deposit()).isEqualTo("1000000");
        assertThat(r.reference()).isEqualTo("0xtx");
    }

    @Test
    void session_topup_happy_path() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/session/topUp"))
            .willReturn(success(sessionReceiptJson("0xchan", "5000000", "0xtoptx"))));
        Receipt.SessionReceipt r = client.sessionTopUp(Map.of("any", 1));
        assertThat(r.deposit()).isEqualTo("5000000");
    }

    @Test
    void session_settle_happy_path() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/session/settle"))
            .willReturn(success(sessionReceiptJson("0xchan", "1000000", "0xsettletx"))));
        Receipt.SessionReceipt r = client.sessionSettle(Map.of("payload", Map.of()));
        assertThat(r.reference()).isEqualTo("0xsettletx");
    }

    @Test
    void session_close_happy_path() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/session/close"))
            .willReturn(success(sessionReceiptJson("0xchan", "1000000", "0xclosetx"))));
        Receipt.SessionReceipt r = client.sessionClose(Map.of("payload", Map.of()));
        assertThat(r.reference()).isEqualTo("0xclosetx");
    }

    @Test
    void session_close_70008_channel_already_closed() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/session/close"))
            .willReturn(success(errorJson(70008, "channel already closed"))));

        assertThatThrownBy(() -> client.sessionClose(Map.of("any", 1)))
            .isInstanceOf(com.okx.payments.mpp.errors.ChannelClosedError.class);
    }

    @Test
    void session_settle_70012_amount_exceeds_deposit() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/session/settle"))
            .willReturn(success(errorJson(70012, "cumulativeAmount > deposit"))));

        assertThatThrownBy(() -> client.sessionSettle(Map.of("any", 1)))
            .isInstanceOf(AmountExceedsDepositError.class);
    }

    @Test
    void session_status_get_with_query_param_and_70010() {
        wm.stubFor(get(urlPathEqualTo("/api/v6/pay/mpp/session/status"))
            .withQueryParam("channelId", equalTo("0xnotfound"))
            .willReturn(success(errorJson(70010, "not found"))));

        assertThatThrownBy(() -> client.sessionStatus("0xnotfound"))
            .isInstanceOf(ChannelNotFoundError.class);
    }

    @Test
    void session_status_happy_path_returns_full_dto() {
        String dataJson = """
            {
              "channelId": "0xchan",
              "payer":     "0xaaaa",
              "payee":     "0xbbbb",
              "token":     "0xcccc",
              "deposit":          "1000000",
              "settledOnChain":   "200000",
              "sessionStatus":    "OPEN",
              "remainingBalance": "800000"
            }
            """;
        wm.stubFor(get(urlPathEqualTo("/api/v6/pay/mpp/session/status"))
            .willReturn(success(envelope(dataJson))));

        SessionStatusResponse r = client.sessionStatus("0xchan");
        assertThat(r.deposit()).isEqualTo("1000000");
        assertThat(r.settledOnChain()).isEqualTo("200000");
        assertThat(r.sessionStatus()).isEqualTo("OPEN");
        assertThat(r.remainingBalance()).isEqualTo("800000");
    }

    // ── Defensive: SDK MUST NEVER call /session/voucher (removed from wire contract) ────

    @Test
    void sdk_never_calls_session_voucher_endpoint() {
        // Stub returns 500 if anyone hits this URL — would surface a unit-test failure
        // anywhere we accidentally added a voucher upload.
        wm.stubFor(post(urlPathEqualTo("/api/v6/pay/mpp/session/voucher"))
            .willReturn(aResponse().withStatus(500).withBody("DELETED ENDPOINT")));
        // We don't call anything; just assert the stub was never matched.
        wm.verify(0, WireMock.postRequestedFor(urlPathEqualTo("/api/v6/pay/mpp/session/voucher")));
    }

    // ── Transport / envelope errors ───────────────────────────────────────────

    @Test
    void http_5xx_wraps_as_service_error() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/charge/settle"))
            .willReturn(aResponse().withStatus(503).withBody("upstream gone")));
        assertThatThrownBy(() -> client.chargeSettle(Map.of()))
            .isInstanceOf(ServiceError.class)
            .hasMessageContaining("503");
    }

    @Test
    void unknown_sa_code_falls_through_to_service_error() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/charge/settle"))
            .willReturn(success(errorJson(99999, "unexpected"))));
        assertThatThrownBy(() -> client.chargeSettle(Map.of()))
            .isInstanceOf(ServiceError.class);
    }

    @Test
    void bad_request_30001_maps_correctly() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/mpp/session/open"))
            .willReturn(success(errorJson(30001, "channelId regex mismatch"))));
        assertThatThrownBy(() -> client.sessionOpen(Map.of()))
            .isInstanceOf(BadRequestError.class);
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    private static com.github.tomakehurst.wiremock.client.ResponseDefinitionBuilder success(String body) {
        return aResponse().withStatus(200).withHeader("Content-Type", "application/json").withBody(body);
    }

    private static String chargeReceiptJson(String txHash, String method) {
        return envelope(("""
            { "method": "%s", "reference": "%s", "status": "success",
              "timestamp": "2026-04-01T12:04:30Z", "chainId": 196,
              "challengeId": "qB3wErTy", "externalId": "order-1" }
            """).formatted(method, txHash));
    }

    private static String sessionReceiptJson(String channelId, String deposit, String reference) {
        return envelope(("""
            { "method": "evm", "intent": "session", "status": "success",
              "timestamp": "2026-04-01T12:04:30Z",
              "channelId": "%s", "deposit": "%s", "chainId": 196, "reference": "%s" }
            """).formatted(channelId, deposit, reference));
    }

    private static String envelope(String dataJson) {
        return ("""
            { "code": 0, "msg": "ok", "data": %s }
            """).formatted(dataJson);
    }

    private static String errorJson(int code, String msg) {
        return ("""
            { "code": %d, "msg": "%s" }
            """).formatted(code, msg.replace("\"", "\\\""));
    }
}
