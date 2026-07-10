// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.integration;

import com.github.tomakehurst.wiremock.WireMockServer;
import com.github.tomakehurst.wiremock.client.WireMock;
import com.github.tomakehurst.wiremock.core.WireMockConfiguration;
// WireMock matching utilities are used via static imports below
import com.okx.x402.client.OKXHttpClient;
import com.okx.x402.config.AssetConfig;
import com.okx.x402.config.AssetRegistry;
import com.okx.x402.config.ResolvedPrice;
import com.okx.x402.crypto.OKXEvmSigner;
import com.okx.x402.crypto.OKXSignerFactory;
import com.okx.x402.facilitator.OKXFacilitatorClient;
import com.okx.x402.model.SettlementResponseHeader;
import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequired;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.SupportedKind;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.model.v2.VerifyResponse;
import com.okx.x402.server.PaymentFilter;
import com.okx.x402.server.PaymentProcessor;
import com.okx.x402.util.Json;
import com.okx.x402.util.OKXAuth;

import jakarta.servlet.http.HttpServlet;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;

import org.eclipse.jetty.server.Server;
import org.eclipse.jetty.server.ServerConnector;
import org.eclipse.jetty.servlet.FilterHolder;
import org.eclipse.jetty.servlet.ServletContextHandler;
import org.eclipse.jetty.servlet.ServletHolder;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.MethodOrderer;
import org.junit.jupiter.api.Order;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.TestMethodOrder;

import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.util.Base64;
import java.util.EnumSet;
import java.util.List;
import java.util.Map;

import static com.github.tomakehurst.wiremock.client.WireMock.aResponse;
import static com.github.tomakehurst.wiremock.client.WireMock.containing;
import static com.github.tomakehurst.wiremock.client.WireMock.equalTo;
import static com.github.tomakehurst.wiremock.client.WireMock.post;
import static com.github.tomakehurst.wiremock.client.WireMock.postRequestedFor;
import static com.github.tomakehurst.wiremock.client.WireMock.get;
import static com.github.tomakehurst.wiremock.client.WireMock.getRequestedFor;
import static com.github.tomakehurst.wiremock.client.WireMock.urlEqualTo;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * End-to-end integration tests for the OKX x402 Java SDK payment flow.
 *
 * <p>Uses embedded Jetty 11 (from WireMock transitive dependency) to host a
 * real PaymentFilter + business servlet, and WireMock to mock the OKX
 * facilitator API endpoints. Validates all 28 acceptance criteria from the
 * execution plan across multiple component boundaries.</p>
 */
@TestMethodOrder(MethodOrderer.OrderAnnotation.class)
class E2EPaymentFlowTest {

    // Throw-away test EOA. Defaults to a non-functional placeholder so the
    // test source carries no usable signing material; override via
    // TEST_PRIVATE_KEY for any real run. The signer here only signs against
    // WireMock stubs — no on-chain transactions are produced.
    static final String TEST_PRIVATE_KEY = envOrPlaceholder(
            "TEST_PRIVATE_KEY",
            "0x0000000000000000000000000000000000000000000000000000000000000001");

    private static String envOrPlaceholder(String name, String fallback) {
        String value = System.getenv(name);
        return value == null || value.isEmpty() ? fallback : value;
    }

    static final String RECEIVER_ADDRESS = "0x2222222222222222222222222222222222222222";
    // Derived from TEST_PRIVATE_KEY so the test stays correct regardless of
    // which throw-away placeholder is used.
    static final String TEST_PAYER = new OKXEvmSigner(TEST_PRIVATE_KEY).getAddress();
    static final String TEST_TX_HASH =
            "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";

    static WireMockServer wireMock;
    static Server jetty;
    static int jettyPort;
    static HttpClient http;
    static OKXEvmSigner signer;

    /**
     * Start WireMock + embedded Jetty before all tests.
     *
     * @throws Exception if server start fails
     */
    @BeforeAll
    static void startServers() throws Exception {
        // 1. Start WireMock
        wireMock = new WireMockServer(
                WireMockConfiguration.wireMockConfig().dynamicPort());
        wireMock.start();
        WireMock.configureFor("localhost", wireMock.port());

        stubVerifySuccess();
        stubSettleSuccess();
        stubSupportedEndpoint();

        // 2. Create facilitator client pointing at WireMock
        OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
                "test-api-key", "test-secret-key", "test-passphrase",
                "http://localhost:" + wireMock.port());

        // 3. Configure route for protected endpoint
        PaymentProcessor.RouteConfig mainRoute = new PaymentProcessor.RouteConfig();
        mainRoute.network = "eip155:196";
        mainRoute.payTo = RECEIVER_ADDRESS;
        mainRoute.price = "$0.01";

        PaymentProcessor.RouteConfig expensiveRoute = new PaymentProcessor.RouteConfig();
        expensiveRoute.network = "eip155:196";
        expensiveRoute.payTo = RECEIVER_ADDRESS;
        expensiveRoute.price = "$1.00";

        PaymentProcessor.RouteConfig flushableRoute = new PaymentProcessor.RouteConfig();
        flushableRoute.network = "eip155:196";
        flushableRoute.payTo = RECEIVER_ADDRESS;
        flushableRoute.price = "$0.01";

        PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
                "GET /api/weather", mainRoute,
                "GET /api/premium", expensiveRoute,
                "GET /api/flushable", flushableRoute
        ));

        // 4. Start embedded Jetty
        jetty = new Server(0);
        ServletContextHandler ctx = new ServletContextHandler(
                ServletContextHandler.SESSIONS);
        ctx.setContextPath("/");

        ctx.addFilter(new FilterHolder(filter), "/*",
                EnumSet.of(jakarta.servlet.DispatcherType.REQUEST));

        ctx.addServlet(new ServletHolder(new BusinessServlet()), "/api/weather");
        ctx.addServlet(new ServletHolder(new BusinessServlet()), "/api/premium");
        ctx.addServlet(new ServletHolder(new FlushingBusinessServlet()),
                "/api/flushable");

        jetty.setHandler(ctx);
        jetty.start();
        jettyPort = ((ServerConnector) jetty.getConnectors()[0]).getLocalPort();

        // 5. Create HTTP client and signer
        http = HttpClient.newHttpClient();
        signer = new OKXEvmSigner(TEST_PRIVATE_KEY);
    }

    /**
     * Stop WireMock and Jetty after all tests.
     *
     * @throws Exception if server stop fails
     */
    @AfterAll
    static void stopServers() throws Exception {
        if (jetty != null) {
            jetty.stop();
        }
        if (wireMock != null) {
            wireMock.stop();
        }
    }

    // -----------------------------------------------------------------------
    // F. X Layer Chain Support
    // -----------------------------------------------------------------------

    /**
     * F-1: X Layer USDT is auto-resolved from AssetRegistry.
     */
    @Test
    @Order(1)
    void f1XlayerUsdtAutoResolve() {
        AssetConfig config = AssetRegistry.getDefault("eip155:196");
        assertNotNull(config, "X Layer mainnet USDT must be pre-registered");
        assertEquals("USDT", config.getSymbol());
        assertEquals(6, config.getDecimals());
        assertNotNull(config.getContractAddress());
    }

    /**
     * F-2: Full X Layer payment flow (client → server → facilitator → settle).
     *
     * @throws Exception if flow fails
     */
    @Test
    @Order(2)
    void f2XlayerFullPaymentFlow() throws Exception {
        wireMock.resetRequests();
        stubVerifySuccess();
        stubSettleSuccess();

        OKXHttpClient payClient = new OKXHttpClient(signer, "eip155:196");
        HttpResponse<String> resp = payClient.get(
                URI.create("http://localhost:" + jettyPort + "/api/weather"));

        assertEquals(200, resp.statusCode(),
                "After auto-402 handling, status should be 200");
        assertTrue(resp.body().contains("weather"),
                "Business response body should be returned");

        // Verify WireMock received both verify and settle
        wireMock.verify(postRequestedFor(
                urlEqualTo("/api/v6/pay/x402/verify")));
        wireMock.verify(postRequestedFor(
                urlEqualTo("/api/v6/pay/x402/settle")));
    }

    /**
     * F-3: EIP-712 domain name is "USD₮0" (Unicode U+20AE).
     */
    @Test
    @Order(3)
    void f3Eip712DomainNameUnicode() {
        AssetConfig config = AssetRegistry.getDefault("eip155:196");
        assertNotNull(config);
        assertEquals("USD\u20AE0", config.getEip712Name(),
                "EIP-712 domain name must use Unicode '₮' (U+20AE)");
    }

    /**
     * F-4: X Layer Testnet is intentionally NOT pre-registered - callers
     * that need testnet must register their own real asset to avoid signing
     * against a placeholder contract address (the prior "TBD" caused
     * OKXEvmSigner to silently produce a corrupt EIP-712 domain).
     */
    @Test
    @Order(4)
    void f4XlayerTestnetNotPreRegistered() {
        assertNull(AssetRegistry.getDefault("eip155:195"),
                "X Layer testnet must stay unregistered by default");
    }

    /**
     * F-5: X Layer contract address and decimals are correct.
     */
    @Test
    @Order(5)
    void f5ContractAddressAndDecimals() {
        AssetConfig config = AssetRegistry.getDefault("eip155:196");
        assertNotNull(config);
        assertEquals("0x779ded0c9e1022225f8e0630b35a9b54be713736",
                config.getContractAddress());
        assertEquals(6, config.getDecimals());
    }

    /**
     * F-6 (P0 regression): PAYMENT-RESPONSE must survive a handler that
     * explicitly commits the response via {@code flushBuffer()}.
     *
     * <p>Before the buffered response wrapper landed in
     * {@code jakarta/server/internal/BufferedHttpServletResponse}, any handler
     * that committed the response (either by calling flushBuffer(), writing
     * past the default container buffer, or returning a streaming body) caused
     * the servlet container to reject the PAYMENT-RESPONSE header that
     * PaymentProcessor.postHandle tries to set, because setHeader() on a
     * committed response is a silent no-op per servlet spec. This test pins
     * the fix so the regression cannot sneak back in.
     *
     * @throws Exception on I/O or HTTP failure
     */
    @Test
    @Order(6)
    void f6PaymentResponseHeaderSurvivesFlushedHandler() throws Exception {
        wireMock.resetRequests();
        stubVerifySuccess();
        stubSettleSuccess();

        OKXHttpClient payClient = new OKXHttpClient(signer, "eip155:196");
        HttpResponse<String> resp = payClient.get(
                URI.create("http://localhost:" + jettyPort + "/api/flushable"));

        assertEquals(200, resp.statusCode(),
                "Auto-402 should resolve to a 200");
        String proof = resp.headers().firstValue("PAYMENT-RESPONSE")
                .orElse(null);
        assertNotNull(proof,
                "PAYMENT-RESPONSE header must survive a handler that "
                        + "committed the response via flushBuffer()");
        String json = new String(Base64.getDecoder().decode(proof));
        SettlementResponseHeader parsed = Json.MAPPER.readValue(
                json, SettlementResponseHeader.class);
        assertTrue(parsed.success);
        assertEquals(TEST_TX_HASH, parsed.transaction);
    }

    // -----------------------------------------------------------------------
    // G. OKX Facilitator
    // -----------------------------------------------------------------------

    /**
     * G-1: OKXFacilitatorClient can be created with credentials (config-and-go).
     */
    @Test
    @Order(10)
    void g1CredentialsConfigAndGo() {
        OKXFacilitatorClient client = new OKXFacilitatorClient(
                "my-key", "my-secret", "my-pass");
        assertNotNull(client, "Facilitator client should be created with 3 args");
    }

    /**
     * G-2: Auth headers are auto-attached to facilitator requests.
     *
     * @throws Exception if request fails
     */
    @Test
    @Order(11)
    void g2AuthHeadersAutoAttached() throws Exception {
        wireMock.resetRequests();
        stubVerifySuccess();

        OKXFacilitatorClient client = new OKXFacilitatorClient(
                "test-api-key", "test-secret-key", "test-passphrase",
                "http://localhost:" + wireMock.port());

        PaymentPayload payload = buildTestPayload();
        PaymentRequirements reqs = buildTestRequirements();

        client.verify(payload, reqs);

        // Verify OKX auth headers were sent
        wireMock.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/verify"))
                .withHeader("OK-ACCESS-KEY", equalTo("test-api-key"))
                .withHeader("OK-ACCESS-PASSPHRASE", equalTo("test-passphrase"))
                .withHeader("Content-Type", equalTo("application/json")));

        // OK-ACCESS-SIGN and OK-ACCESS-TIMESTAMP should be present
        wireMock.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/verify"))
                .withHeader("OK-ACCESS-SIGN",
                        com.github.tomakehurst.wiremock.client.WireMock.matching(".+")));
        wireMock.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/verify"))
                .withHeader("OK-ACCESS-TIMESTAMP",
                        com.github.tomakehurst.wiremock.client.WireMock.matching(".+")));
    }

    /**
     * G-3: Verify request format is correctly converted to V2.
     *
     * @throws Exception if request fails
     */
    @Test
    @Order(12)
    void g3VerifyFormatTransparentConversion() throws Exception {
        wireMock.resetRequests();
        stubVerifySuccess();

        OKXFacilitatorClient client = new OKXFacilitatorClient(
                "test-api-key", "test-secret-key", "test-passphrase",
                "http://localhost:" + wireMock.port());

        PaymentPayload payload = buildTestPayload();
        PaymentRequirements reqs = buildTestRequirements();
        VerifyResponse result = client.verify(payload, reqs);

        assertTrue(result.isValid, "Verify should return valid=true");

        // Verify the request body contains x402Version and paymentPayload
        wireMock.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/verify"))
                .withRequestBody(containing("\"x402Version\":2"))
                .withRequestBody(containing("\"paymentPayload\""))
                .withRequestBody(containing("\"paymentRequirements\"")));
    }

    /**
     * G-4: Settle request format is correctly converted to V2.
     *
     * @throws Exception if request fails
     */
    @Test
    @Order(13)
    void g4SettleFormatTransparentConversion() throws Exception {
        wireMock.resetRequests();
        stubSettleSuccess();

        OKXFacilitatorClient client = new OKXFacilitatorClient(
                "test-api-key", "test-secret-key", "test-passphrase",
                "http://localhost:" + wireMock.port());

        PaymentPayload payload = buildTestPayload();
        PaymentRequirements reqs = buildTestRequirements();
        SettleResponse result = client.settle(payload, reqs);

        assertTrue(result.success, "Settle should return success=true");
        assertEquals(TEST_TX_HASH, result.transaction);

        // Verify the request body
        wireMock.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/settle"))
                .withRequestBody(containing("\"x402Version\":2"))
                .withRequestBody(containing("\"paymentPayload\""))
                .withRequestBody(containing("\"paymentRequirements\"")));
    }

    /**
     * G-5: getSupported format transparent conversion.
     *
     * @throws Exception if request fails
     */
    @Test
    @Order(14)
    void g5GetSupportedFormatConversion() throws Exception {
        wireMock.resetRequests();
        stubSupportedEndpoint();

        OKXFacilitatorClient client = new OKXFacilitatorClient(
                "test-api-key", "test-secret-key", "test-passphrase",
                "http://localhost:" + wireMock.port());

        SupportedResponse result = client.supported();

        assertNotNull(result, "Supported response should not be null");
        assertFalse(result.kinds.isEmpty(),
                "Should have at least one supported kind");
        assertEquals("exact", result.kinds.get(0).scheme);
        assertEquals("eip155:196", result.kinds.get(0).network);
    }

    /**
     * G-8: Missing credentials throw immediately.
     */
    @Test
    @Order(17)
    void g8MissingCredentialsError() {
        assertThrows(IllegalArgumentException.class,
                () -> new OKXAuth(null, "secret", "pass"),
                "Null API key should throw");
        assertThrows(IllegalArgumentException.class,
                () -> new OKXAuth("key", "", "pass"),
                "Empty secret should throw");
        assertThrows(IllegalArgumentException.class,
                () -> new OKXAuth("key", "secret", null),
                "Null passphrase should throw");
    }

    /**
     * G-9: OKX error code 50103 is mapped to readable message.
     *
     * @throws Exception if unexpected
     */
    @Test
    @Order(18)
    void g9ErrorCodeMapping50103() throws Exception {
        WireMockServer errorMock = new WireMockServer(
                WireMockConfiguration.wireMockConfig().dynamicPort());
        errorMock.start();

        try {
            errorMock.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                    .willReturn(aResponse()
                            .withStatus(400)
                            .withHeader("Content-Type", "application/json")
                            .withBody("{\"code\":\"50103\","
                                    + "\"msg\":\"Invalid API key\"}")));

            OKXFacilitatorClient client = new OKXFacilitatorClient(
                    "bad-key", "bad-secret", "bad-pass",
                    "http://localhost:" + errorMock.port());

            PaymentPayload payload = buildTestPayload();
            PaymentRequirements reqs = buildTestRequirements();

            IOException ex = assertThrows(IOException.class,
                    () -> client.verify(payload, reqs));
            assertTrue(ex.getMessage().contains("Invalid API key"),
                    "Error message should contain mapped text, got: "
                            + ex.getMessage());
            assertTrue(ex.getMessage().contains("50103"),
                    "Error message should contain code");
        } finally {
            errorMock.stop();
        }
    }

    // -----------------------------------------------------------------------
    // I. Client Signing
    // -----------------------------------------------------------------------

    /**
     * I-1: PrivateKey signing produces valid X Layer transaction data.
     *
     * @throws Exception if signing fails
     */
    @Test
    @Order(20)
    void i1PrivateKeySigningXlayer() throws Exception {
        PaymentRequirements reqs = buildTestRequirements();
        Map<String, Object> signed = signer.signPaymentRequirements(reqs);

        assertNotNull(signed.get("signature"),
                "Signed payload must contain 'signature'");
        assertNotNull(signed.get("authorization"),
                "Signed payload must contain 'authorization'");

        String sig = (String) signed.get("signature");
        assertTrue(sig.startsWith("0x"),
                "Signature should be 0x-prefixed hex");
        // 65 bytes = 130 hex chars + "0x" prefix
        assertTrue(sig.length() >= 132,
                "Signature should be at least 65 bytes (130 hex chars)");

        @SuppressWarnings("unchecked")
        Map<String, Object> auth = (Map<String, Object>) signed.get("authorization");
        assertNotNull(auth.get("from"));
        assertNotNull(auth.get("to"));
        assertNotNull(auth.get("value"));
        assertNotNull(auth.get("validAfter"));
        assertNotNull(auth.get("validBefore"));
        assertNotNull(auth.get("nonce"));
    }

    /**
     * I-2: Signer address matches the well-known address for the test key.
     */
    @Test
    @Order(21)
    void i2SignerAddressCorrect() {
        String addr = signer.getAddress();
        assertNotNull(addr);
        assertTrue(addr.startsWith("0x"), "Address must be 0x-prefixed");
        assertEquals(42, addr.length(), "Address should be 42 chars (0x + 40 hex)");
        assertEquals(TEST_PAYER, addr,
                "Address should match the well-known test key address");
    }

    /**
     * I-3: No config (null privateKey) triggers immediate error.
     */
    @Test
    @Order(22)
    void i3NoConfigImmediateError() {
        assertThrows(IllegalArgumentException.class,
                () -> OKXSignerFactory.createOKXSigner(null),
                "Null config should throw");

        OKXSignerFactory.OKXSignerConfig emptyConfig =
                new OKXSignerFactory.OKXSignerConfig();
        assertThrows(IllegalArgumentException.class,
                () -> OKXSignerFactory.createOKXSigner(emptyConfig),
                "Config with no private key should throw");
    }

    // -----------------------------------------------------------------------
    // B. Basic Client (integration - full flow)
    // -----------------------------------------------------------------------

    /**
     * B-1: Client auto-handles 402 response (first request gets 402, retry with payment succeeds).
     *
     * @throws Exception if flow fails
     */
    @Test
    @Order(30)
    void b1Auto402Handling() throws Exception {
        wireMock.resetRequests();
        stubVerifySuccess();
        stubSettleSuccess();

        OKXHttpClient payClient = new OKXHttpClient(signer, "eip155:196");
        HttpResponse<String> resp = payClient.get(
                URI.create("http://localhost:" + jettyPort + "/api/weather"));

        assertEquals(200, resp.statusCode(),
                "OKXHttpClient should auto-handle 402 and return 200");
    }

    /**
     * B-2: 402 response body contains correct x402Version, accepts, and resource.
     *
     * @throws Exception if request fails
     */
    @Test
    @Order(31)
    void b2PaymentRequiredBodyCorrect() throws Exception {
        // Send a plain GET without payment header - should get 402
        HttpRequest req = HttpRequest.newBuilder()
                .uri(URI.create("http://localhost:" + jettyPort + "/api/weather"))
                .GET()
                .build();

        HttpResponse<String> resp = http.send(req,
                HttpResponse.BodyHandlers.ofString());

        assertEquals(402, resp.statusCode());

        PaymentRequired pr = Json.MAPPER.readValue(resp.body(),
                PaymentRequired.class);
        assertEquals(2, pr.x402Version, "Must be V2 protocol");
        assertNotNull(pr.resource, "Resource info must be present");
        assertNotNull(pr.resource.url, "Resource URL must be present");
        assertTrue(pr.resource.url.contains("/api/weather"),
                "Resource URL should contain the path");
        assertNotNull(pr.accepts, "Accepts list must be present");
        assertFalse(pr.accepts.isEmpty(), "Must have at least one accepted method");

        PaymentRequirements accept = pr.accepts.get(0);
        assertEquals("eip155:196", accept.network);
        assertEquals("exact", accept.scheme);
        assertEquals(RECEIVER_ADDRESS, accept.payTo);
        assertNotNull(accept.amount, "Amount must be resolved");
        assertNotNull(accept.asset, "Asset address must be resolved");
        assertNotNull(accept.extra, "Extra fields must be present");

        // Regression: EIP-712 domain name must survive JSON serialization as UTF-8.
        // If the servlet writer falls back to ISO-8859-1, '₮' (U+20AE) is silently
        // replaced with '?', which breaks EIP-712 signing against the real
        // on-chain contract name. See PaymentProcessor / *ResponseAdapter.
        assertEquals("USD₮0", accept.extra.get("name"),
                "extra.name must preserve U+20AE (₮) after JSON round-trip");
        byte[] rawBody = resp.body().getBytes(java.nio.charset.StandardCharsets.UTF_8);
        byte[] expected = "USD₮0".getBytes(java.nio.charset.StandardCharsets.UTF_8);
        boolean found = false;
        outer:
        for (int i = 0; i <= rawBody.length - expected.length; i++) {
            for (int j = 0; j < expected.length; j++) {
                if (rawBody[i + j] != expected[j]) {
                    continue outer;
                }
            }
            found = true;
            break;
        }
        assertTrue(found,
                "Raw response body must contain the UTF-8 byte sequence for 'USD₮0'");
    }

    /**
     * B-3: Successful payment returns biz data + PAYMENT-RESPONSE header.
     *
     * @throws Exception if flow fails
     */
    @Test
    @Order(32)
    void b3PaymentSuccessReturnsBizDataAndHeader() throws Exception {
        wireMock.resetRequests();
        stubVerifySuccess();
        stubSettleSuccess();

        OKXHttpClient payClient = new OKXHttpClient(signer, "eip155:196");
        HttpResponse<String> resp = payClient.get(
                URI.create("http://localhost:" + jettyPort + "/api/weather"));

        assertEquals(200, resp.statusCode());

        // Business data present
        assertTrue(resp.body().contains("weather"),
                "Business response body should be present");

        // PAYMENT-RESPONSE header present and decodable
        String prHeader = resp.headers().firstValue("PAYMENT-RESPONSE")
                .orElse(null);
        assertNotNull(prHeader, "PAYMENT-RESPONSE header must be present");

        byte[] decoded = Base64.getDecoder().decode(prHeader);
        SettlementResponseHeader srh = Json.MAPPER.readValue(decoded,
                SettlementResponseHeader.class);
        assertTrue(srh.success, "Settlement should be successful");
        assertEquals(TEST_TX_HASH, srh.transaction);
        assertEquals("eip155:196", srh.network);
    }

    // -----------------------------------------------------------------------
    // C. Basic Server
    // -----------------------------------------------------------------------

    /**
     * C-1: Different routes have different prices (route-level pricing).
     *
     * @throws Exception if request fails
     */
    @Test
    @Order(40)
    void c1RouteLevelPricing() throws Exception {
        // GET /api/weather → $0.01 = 10000 atomic units
        HttpResponse<String> resp1 = http.send(
                HttpRequest.newBuilder()
                        .uri(URI.create("http://localhost:" + jettyPort
                                + "/api/weather"))
                        .GET().build(),
                HttpResponse.BodyHandlers.ofString());

        assertEquals(402, resp1.statusCode());
        PaymentRequired pr1 = Json.MAPPER.readValue(resp1.body(),
                PaymentRequired.class);
        String amount1 = pr1.accepts.get(0).amount;

        // GET /api/premium → $1.00 = 1000000 atomic units
        HttpResponse<String> resp2 = http.send(
                HttpRequest.newBuilder()
                        .uri(URI.create("http://localhost:" + jettyPort
                                + "/api/premium"))
                        .GET().build(),
                HttpResponse.BodyHandlers.ofString());

        assertEquals(402, resp2.statusCode());
        PaymentRequired pr2 = Json.MAPPER.readValue(resp2.body(),
                PaymentRequired.class);
        String amount2 = pr2.accepts.get(0).amount;

        assertFalse(amount1.equals(amount2),
                "Different routes must have different prices: "
                        + amount1 + " vs " + amount2);
        assertEquals("10000", amount1, "$0.01 with 6 decimals = 10000");
        assertEquals("1000000", amount2, "$1.00 with 6 decimals = 1000000");
    }

    /**
     * C-2: Middleware one-line integration (PaymentFilter.create factory).
     */
    @Test
    @Order(41)
    void c2MiddlewareOneLineIntegration() {
        OKXFacilitatorClient client = new OKXFacilitatorClient(
                "k", "s", "p", "http://localhost:" + wireMock.port());

        PaymentProcessor.RouteConfig rc = new PaymentProcessor.RouteConfig();
        rc.network = "eip155:196";
        rc.payTo = RECEIVER_ADDRESS;
        rc.price = "$0.05";

        // One-line creation
        PaymentFilter filter = PaymentFilter.create(client,
                Map.of("GET /test", rc));
        assertNotNull(filter,
                "PaymentFilter.create should return a non-null filter");
    }

    // -----------------------------------------------------------------------
    // ERR. Error Scenarios
    // -----------------------------------------------------------------------

    /**
     * ERR-1: Invalid OKX API key returns mapped error from facilitator.
     *
     * @throws Exception if unexpected
     */
    @Test
    @Order(50)
    void err1InvalidOkxApiKey() throws Exception {
        WireMockServer errorMock = new WireMockServer(
                WireMockConfiguration.wireMockConfig().dynamicPort());
        errorMock.start();

        try {
            errorMock.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                    .willReturn(aResponse()
                            .withStatus(401)
                            .withHeader("Content-Type", "application/json")
                            .withBody("{\"code\":\"50103\","
                                    + "\"msg\":\"Invalid API key\"}")));

            OKXFacilitatorClient client = new OKXFacilitatorClient(
                    "invalid-key", "invalid-secret", "invalid-pass",
                    "http://localhost:" + errorMock.port());

            IOException ex = assertThrows(IOException.class,
                    () -> client.verify(buildTestPayload(),
                            buildTestRequirements()));
            assertTrue(ex.getMessage().contains("Invalid API key"));
        } finally {
            errorMock.stop();
        }
    }

    /**
     * ERR-2: Invalid signature rejected by facilitator verify.
     *
     * @throws Exception if unexpected
     */
    @Test
    @Order(51)
    void err2InvalidSignature() throws Exception {
        WireMockServer errorMock = new WireMockServer(
                WireMockConfiguration.wireMockConfig().dynamicPort());
        errorMock.start();

        try {
            errorMock.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                    .willReturn(aResponse()
                            .withStatus(200)
                            .withHeader("Content-Type", "application/json")
                            .withBody("{\"isValid\":false,"
                                    + "\"invalidReason\":\"invalid_signature\","
                                    + "\"invalidMessage\":"
                                    + "\"Signature verification failed\"}")));

            OKXFacilitatorClient client = new OKXFacilitatorClient(
                    "key", "secret", "pass",
                    "http://localhost:" + errorMock.port());

            VerifyResponse result = client.verify(buildTestPayload(),
                    buildTestRequirements());
            assertFalse(result.isValid, "Invalid signature should not pass");
            assertEquals("invalid_signature", result.invalidReason);
        } finally {
            errorMock.stop();
        }
    }

    /**
     * ERR-3: Insufficient balance rejected by facilitator.
     *
     * @throws Exception if unexpected
     */
    @Test
    @Order(52)
    void err3InsufficientBalance() throws Exception {
        WireMockServer errorMock = new WireMockServer(
                WireMockConfiguration.wireMockConfig().dynamicPort());
        errorMock.start();

        try {
            errorMock.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                    .willReturn(aResponse()
                            .withStatus(200)
                            .withHeader("Content-Type", "application/json")
                            .withBody("{\"isValid\":false,"
                                    + "\"invalidReason\":\"insufficient_balance\","
                                    + "\"invalidMessage\":"
                                    + "\"Payer has insufficient balance\"}")));

            OKXFacilitatorClient client = new OKXFacilitatorClient(
                    "key", "secret", "pass",
                    "http://localhost:" + errorMock.port());

            VerifyResponse result = client.verify(buildTestPayload(),
                    buildTestRequirements());
            assertFalse(result.isValid);
            assertEquals("insufficient_balance", result.invalidReason);
        } finally {
            errorMock.stop();
        }
    }

    /**
     * ERR-4: Signature expired rejected by facilitator.
     *
     * @throws Exception if unexpected
     */
    @Test
    @Order(53)
    void err4SignatureExpired() throws Exception {
        WireMockServer errorMock = new WireMockServer(
                WireMockConfiguration.wireMockConfig().dynamicPort());
        errorMock.start();

        try {
            errorMock.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                    .willReturn(aResponse()
                            .withStatus(200)
                            .withHeader("Content-Type", "application/json")
                            .withBody("{\"isValid\":false,"
                                    + "\"invalidReason\":\"signature_expired\","
                                    + "\"invalidMessage\":"
                                    + "\"Payment authorization has expired\"}")));

            OKXFacilitatorClient client = new OKXFacilitatorClient(
                    "key", "secret", "pass",
                    "http://localhost:" + errorMock.port());

            VerifyResponse result = client.verify(buildTestPayload(),
                    buildTestRequirements());
            assertFalse(result.isValid);
            assertEquals("signature_expired", result.invalidReason);
        } finally {
            errorMock.stop();
        }
    }

    /**
     * ERR-5: OKX API timeout handled gracefully.
     *
     * @throws Exception if unexpected
     */
    @Test
    @Order(54)
    void err5OkxApiTimeout() throws Exception {
        WireMockServer errorMock = new WireMockServer(
                WireMockConfiguration.wireMockConfig().dynamicPort());
        errorMock.start();

        try {
            errorMock.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                    .willReturn(aResponse()
                            .withStatus(200)
                            .withFixedDelay(35000))); // 35s > 30s client timeout

            OKXFacilitatorClient client = new OKXFacilitatorClient(
                    "key", "secret", "pass",
                    "http://localhost:" + errorMock.port());

            assertThrows(Exception.class,
                    () -> client.verify(buildTestPayload(),
                            buildTestRequirements()),
                    "Should throw on timeout");
        } finally {
            errorMock.stop();
        }
    }

    /**
     * ERR-6: Facilitator not configured for unknown network.
     */
    @Test
    @Order(55)
    void err6FacilitatorNotConfigured() {
        // AssetRegistry has no default for an unknown network
        assertNull(AssetRegistry.getDefault("eip155:9999"),
                "Unknown network should have no default asset");

        assertThrows(IllegalArgumentException.class,
                () -> AssetRegistry.resolvePrice("$0.01", "eip155:9999"),
                "Resolving price for unconfigured network should throw");
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    private static void stubVerifySuccess() {
        wireMock.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                .atPriority(5)
                .willReturn(aResponse()
                        .withStatus(200)
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"isValid\":true,\"payer\":\""
                                + TEST_PAYER + "\"}")));
    }

    private static void stubSettleSuccess() {
        wireMock.stubFor(post(urlEqualTo("/api/v6/pay/x402/settle"))
                .atPriority(5)
                .willReturn(aResponse()
                        .withStatus(200)
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"success\":true,"
                                + "\"transaction\":\"" + TEST_TX_HASH + "\","
                                + "\"network\":\"eip155:196\","
                                + "\"payer\":\"" + TEST_PAYER + "\"}")));
    }

    private static void stubSupportedEndpoint() {
        wireMock.stubFor(get(urlEqualTo("/api/v6/pay/x402/supported"))
                .willReturn(aResponse()
                        .withStatus(200)
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"kinds\":[{\"x402Version\":2,"
                                + "\"scheme\":\"exact\","
                                + "\"network\":\"eip155:196\"}],"
                                + "\"extensions\":[],"
                                + "\"signers\":{}}")));
    }

    private static PaymentPayload buildTestPayload() {
        PaymentPayload payload = new PaymentPayload();
        payload.x402Version = 2;
        payload.payload = Map.of(
                "signature", "0xdeadbeef",
                "authorization", Map.of(
                        "from", TEST_PAYER,
                        "to", RECEIVER_ADDRESS,
                        "value", "10000",
                        "validAfter", "0",
                        "validBefore", "999999999999",
                        "nonce", "0x01"
                )
        );
        return payload;
    }

    private static PaymentRequirements buildTestRequirements() {
        PaymentRequirements reqs = new PaymentRequirements();
        reqs.scheme = "exact";
        reqs.network = "eip155:196";
        reqs.amount = "10000";
        reqs.payTo = RECEIVER_ADDRESS;
        reqs.maxTimeoutSeconds = 86400;
        reqs.asset = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
        reqs.extra = Map.of(
                "name", "USD\u20AE0",
                "version", "1",
                "transferMethod", "eip3009"
        );
        return reqs;
    }

    /**
     * Simple business servlet that returns weather data.
     */
    static class BusinessServlet extends HttpServlet {
        private static final long serialVersionUID = 1L;

        @Override
        protected void doGet(HttpServletRequest req,
                             HttpServletResponse resp) throws IOException {
            resp.setContentType("application/json");
            resp.setStatus(HttpServletResponse.SC_OK);
            resp.getWriter().write(
                    "{\"weather\":\"sunny\",\"temp\":25}");
        }
    }

    /**
     * Business servlet that explicitly commits the response (via
     * {@code flushBuffer()}) before returning control to the filter.
     *
     * <p>Without the {@link com.okx.x402.server.internal.BufferedHttpServletResponse}
     * wrapper, this forces the underlying response into the committed state
     * before {@code PaymentProcessor.postHandle} runs, and the servlet
     * container silently drops the {@code PAYMENT-RESPONSE} header the
     * post-handler tries to set. Used by the P0 regression test.
     */
    static class FlushingBusinessServlet extends HttpServlet {
        private static final long serialVersionUID = 1L;

        @Override
        protected void doGet(HttpServletRequest req,
                             HttpServletResponse resp) throws IOException {
            resp.setContentType("application/json");
            resp.setStatus(HttpServletResponse.SC_OK);
            resp.getWriter().write("{\"data\":\"flushed-early\"}");
            // Force the response to commit before returning to the filter.
            resp.flushBuffer();
        }
    }
}
