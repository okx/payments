// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.facilitator;

import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.SettleRequest;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.model.v2.VerifyRequest;
import com.okx.x402.model.v2.VerifyResponse;
import com.fasterxml.jackson.databind.JsonNode;
import com.okx.x402.util.Json;
import com.okx.x402.util.OKXAuth;

import java.io.IOException;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.Map;
import java.util.Objects;

/**
 * OKX facilitator client implementation.
 * Talks to OKX /api/v6/pay/x402 endpoints with HMAC-SHA256 auth.
 */
public class OKXFacilitatorClient implements FacilitatorClient {

    private static final String VERIFY_PATH = "/api/v6/pay/x402/verify";
    private static final String SETTLE_PATH = "/api/v6/pay/x402/settle";
    private static final String SETTLE_STATUS_PATH = "/api/v6/pay/x402/settle/status";
    private static final String SUPPORTED_PATH = "/api/v6/pay/x402/supported";

    private static final int MAX_RETRIES = 3;
    private static final long BASE_RETRY_DELAY_MS = 1000;

    private static final Map<String, String> ERROR_MAP = Map.of(
            "50103", "Invalid API key",
            "50104", "Invalid API key or IP",
            "50113", "Invalid passphrase",
            "50001", "Service temporarily unavailable",
            "50011", "Too many requests (rate limit)",
            "8000", "TEE operation failed",
            "10002", "x402 AA account not found",
            "30001","parameter incorrect"
    );

    private final OKXAuth auth;
    private final HttpExecutor executor;
    private final String baseUrl;
    private final Duration requestTimeout;

    /**
     * Creates an OKXFacilitatorClient with default base URL and default
     * timeouts (10s connect, 30s request).
     *
     * @param apiKey OKX API key
     * @param secretKey OKX secret key
     * @param passphrase OKX passphrase
     */
    public OKXFacilitatorClient(String apiKey, String secretKey, String passphrase) {
        this(new OKXFacilitatorConfig(apiKey, secretKey, passphrase));
    }

    /**
     * Creates an OKXFacilitatorClient with a custom base URL (e.g. for test
     * environments) and default timeouts.
     *
     * @param apiKey OKX API key
     * @param secretKey OKX secret key
     * @param passphrase OKX passphrase
     * @param baseUrl base URL of OKX API
     */
    public OKXFacilitatorClient(String apiKey, String secretKey,
                                String passphrase, String baseUrl) {
        this(initConfig(apiKey, secretKey, passphrase, baseUrl));
    }

    /**
     * Creates an OKXFacilitatorClient from a full configuration object.
     *
     * <p>HTTP execution precedence: {@code config.httpExecutor} &gt;
     * {@code config.httpClient} (wrapped in {@link JdkHttpExecutor}) &gt;
     * default JDK client built from {@code config.connectTimeout}.
     */
    public OKXFacilitatorClient(OKXFacilitatorConfig config) {
        Objects.requireNonNull(config, "config");
        this.auth = new OKXAuth(config.apiKey, config.secretKey, config.passphrase);
        this.baseUrl = Objects.requireNonNull(config.baseUrl, "baseUrl")
                .replaceAll("/+$", "");
        this.requestTimeout = Objects.requireNonNull(
                config.requestTimeout, "requestTimeout");
        this.executor = resolveExecutor(config);
    }

    private static HttpExecutor resolveExecutor(OKXFacilitatorConfig config) {
        if (config.httpExecutor != null) {
            return config.httpExecutor;
        }
        HttpClient jdk = config.httpClient != null
                ? config.httpClient
                : HttpClient.newBuilder()
                        .connectTimeout(Objects.requireNonNull(
                                config.connectTimeout, "connectTimeout"))
                        .build();
        return new JdkHttpExecutor(jdk);
    }

    private static OKXFacilitatorConfig initConfig(String apiKey, String secretKey,
                                                   String passphrase, String baseUrl) {
        OKXFacilitatorConfig cfg = new OKXFacilitatorConfig(apiKey, secretKey, passphrase);
        cfg.baseUrl = Objects.requireNonNull(baseUrl, "baseUrl");
        return cfg;
    }

    @Override
    public VerifyResponse verify(PaymentPayload payload,
                                 PaymentRequirements requirements)
            throws IOException, InterruptedException {
        VerifyRequest req = new VerifyRequest();
        req.x402Version = 2;
        req.paymentPayload = payload;
        req.paymentRequirements = requirements;

        String body = Json.MAPPER.writeValueAsString(req);
        String responseBody = doPost(VERIFY_PATH, body);
        return Json.MAPPER.readValue(responseBody, VerifyResponse.class);
    }

    @Override
    public SettleResponse settle(PaymentPayload payload,
                                 PaymentRequirements requirements)
            throws IOException, InterruptedException {
        return settle(payload, requirements, false);
    }

    @Override
    public SettleResponse settle(PaymentPayload payload,
                                 PaymentRequirements requirements,
                                 boolean syncSettle)
            throws IOException, InterruptedException {
        SettleRequest req = new SettleRequest();
        req.x402Version = 2;
        req.paymentPayload = payload;
        req.paymentRequirements = requirements;
        if (syncSettle) {
            req.syncSettle = true;
        }

        String body = Json.MAPPER.writeValueAsString(req);
        String responseBody = doPost(SETTLE_PATH, body);
        return Json.MAPPER.readValue(responseBody, SettleResponse.class);
    }

    @Override
    public SettleResponse settleStatus(String txHash)
            throws IOException, InterruptedException {
        Objects.requireNonNull(txHash, "txHash");
        // URL-encode in case a caller passes an unexpectedly-shaped value;
        // hex tx hashes are unchanged by encoding, while any stray '&' or
        // '#' would otherwise corrupt the query string (and the HMAC input).
        String path = SETTLE_STATUS_PATH
                + "?txHash=" + URLEncoder.encode(txHash, StandardCharsets.UTF_8);
        String responseBody = doGet(path);
        return Json.MAPPER.readValue(responseBody, SettleResponse.class);
    }

    @Override
    public SupportedResponse supported() throws IOException, InterruptedException {
        String responseBody = doGet(SUPPORTED_PATH);
        return Json.MAPPER.readValue(responseBody, SupportedResponse.class);
    }

    private String doPost(String path, String body) throws IOException, InterruptedException {
        for (int attempt = 0; ; attempt++) {
            // Generate fresh auth headers each attempt (timestamp must be current)
            Map<String, String> headers = auth.createHeaders("POST", path, body);

            HttpExecutor.HttpExecResult resp = executor.execute(
                    "POST", URI.create(baseUrl + path), body, headers, requestTimeout);

            if (shouldRetry(resp, attempt)) {
                Thread.sleep(BASE_RETRY_DELAY_MS * (1L << attempt));
                continue;
            }

            return handleResponse(resp, path);
        }
    }

    private String doGet(String path) throws IOException, InterruptedException {
        for (int attempt = 0; ; attempt++) {
            Map<String, String> headers = auth.createHeaders("GET", path, "");

            HttpExecutor.HttpExecResult resp = executor.execute(
                    "GET", URI.create(baseUrl + path), null, headers, requestTimeout);

            if (shouldRetry(resp, attempt)) {
                Thread.sleep(BASE_RETRY_DELAY_MS * (1L << attempt));
                continue;
            }

            return handleResponse(resp, path);
        }
    }

    /**
     * Determines if the request should be retried based on HTTP status
     * or OKX rate-limit error code (50011).
     */
    private boolean shouldRetry(HttpExecutor.HttpExecResult resp, int attempt) {
        if (attempt >= MAX_RETRIES) {
            return false;
        }
        if (resp.statusCode() == 429) {
            return true;
        }
        // Check for OKX rate-limit code in 200 envelope
        if (resp.statusCode() == 200) {
            try {
                JsonNode root = Json.MAPPER.readTree(resp.body());
                if (root.has("code") && "50011".equals(root.get("code").asText())) {
                    return true;
                }
            } catch (Exception ignored) {
                // Not parseable — don't retry
            }
        }
        return false;
    }

    /**
     * Handles the OKX API response, unwrapping the envelope if present.
     *
     * <p>OKX APIs return responses in two formats:
     * <ul>
     *   <li>Envelope: {@code {"code":0,"data":{...},"msg":""}} — unwrap to data</li>
     *   <li>Direct: the response body is the payload itself (e.g. from WireMock/tests)</li>
     * </ul>
     */
    private String handleResponse(HttpExecutor.HttpExecResult resp, String path)
            throws IOException {
        if (resp.statusCode() != 200) {
            return handleErrorResponse(resp, path);
        }

        String body = resp.body();
        try {
            JsonNode root = Json.MAPPER.readTree(body);

            // Check for OKX envelope: {"code":...,"data":...}
            if (root.has("code") && root.has("data")) {
                int code = root.get("code").asInt(-1);
                if (code != 0) {
                    // Non-zero code = business error even on HTTP 200
                    String msg = root.has("msg") ? root.get("msg").asText() : "Unknown error";
                    String errCode = root.get("code").asText();
                    String mapped = ERROR_MAP.getOrDefault(errCode, msg);
                    throw new IOException("OKX API error on " + path
                            + " (code=" + errCode + "): " + mapped);
                }
                // Unwrap: return the "data" node as JSON string
                return Json.MAPPER.writeValueAsString(root.get("data"));
            }

            // No envelope — return body as-is (direct V2 format)
            return body;

        } catch (IOException rethrow) {
            throw rethrow;
        } catch (Exception e) {
            // Not valid JSON or unexpected structure — return raw body
            return body;
        }
    }

    private String handleErrorResponse(HttpExecutor.HttpExecResult resp, String path)
            throws IOException {
        try {
            JsonNode node = Json.MAPPER.readTree(resp.body());
            String code = node.has("code") ? node.get("code").asText() : "";
            String msg = ERROR_MAP.getOrDefault(code,
                    node.has("msg") ? node.get("msg").asText() : "Unknown error");
            throw new IOException("OKX API error on " + path
                    + " (code=" + code + "): " + msg);
        } catch (IOException rethrow) {
            throw rethrow;
        } catch (Exception e) {
            throw new IOException("OKX API HTTP " + resp.statusCode()
                    + " on " + path + ": " + resp.body());
        }
    }
}
