package com.okx.x402.facilitator;

import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.SettleRequest;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.model.v2.VerifyRequest;
import com.okx.x402.model.v2.VerifyResponse;
import com.okx.x402.util.Json;
import com.okx.x402.util.OKXAuth;

import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.Map;

/**
 * OKX facilitator client implementation.
 * Talks to OKX /api/v6/pay/x402 endpoints with HMAC-SHA256 auth.
 */
public class OKXFacilitatorClient implements FacilitatorClient {

    private static final String DEFAULT_BASE_URL = "https://www.okx.com";
    private static final String VERIFY_PATH = "/api/v6/pay/x402/verify";
    private static final String SETTLE_PATH = "/api/v6/pay/x402/settle";
    private static final String SUPPORTED_PATH = "/api/v6/pay/x402/supported";

    private static final Map<String, String> ERROR_MAP = Map.of(
            "50103", "Invalid API key",
            "50104", "Invalid API key or IP",
            "50113", "Invalid passphrase",
            "50001", "Service temporarily unavailable"
    );

    private final OKXAuth auth;
    private final HttpClient http;
    private final String baseUrl;

    /**
     * Creates an OKXFacilitatorClient with default base URL.
     *
     * @param apiKey OKX API key
     * @param secretKey OKX secret key
     * @param passphrase OKX passphrase
     */
    public OKXFacilitatorClient(String apiKey, String secretKey, String passphrase) {
        this(apiKey, secretKey, passphrase, DEFAULT_BASE_URL);
    }

    /**
     * Creates an OKXFacilitatorClient with custom base URL.
     *
     * @param apiKey OKX API key
     * @param secretKey OKX secret key
     * @param passphrase OKX passphrase
     * @param baseUrl base URL of OKX API
     */
    public OKXFacilitatorClient(String apiKey, String secretKey,
                                String passphrase, String baseUrl) {
        this.auth = new OKXAuth(apiKey, secretKey, passphrase);
        this.baseUrl = baseUrl.replaceAll("/+$", "");
        this.http = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(10))
                .build();
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
        SettleRequest req = new SettleRequest();
        req.x402Version = 2;
        req.paymentPayload = payload;
        req.paymentRequirements = requirements;

        String body = Json.MAPPER.writeValueAsString(req);
        String responseBody = doPost(SETTLE_PATH, body);
        return Json.MAPPER.readValue(responseBody, SettleResponse.class);
    }

    @Override
    public SupportedResponse supported() throws IOException, InterruptedException {
        String responseBody = doGet(SUPPORTED_PATH);
        return Json.MAPPER.readValue(responseBody, SupportedResponse.class);
    }

    private String doPost(String path, String body) throws IOException, InterruptedException {
        Map<String, String> headers = auth.createHeaders("POST", path, body);

        HttpRequest.Builder builder = HttpRequest.newBuilder()
                .uri(URI.create(baseUrl + path))
                .POST(HttpRequest.BodyPublishers.ofString(body))
                .timeout(Duration.ofSeconds(30));

        headers.forEach(builder::header);

        HttpResponse<String> resp = http.send(builder.build(),
                HttpResponse.BodyHandlers.ofString());

        return handleResponse(resp, path);
    }

    private String doGet(String path) throws IOException, InterruptedException {
        Map<String, String> headers = auth.createHeaders("GET", path, "");

        HttpRequest.Builder builder = HttpRequest.newBuilder()
                .uri(URI.create(baseUrl + path))
                .GET()
                .timeout(Duration.ofSeconds(30));

        headers.forEach(builder::header);

        HttpResponse<String> resp = http.send(builder.build(),
                HttpResponse.BodyHandlers.ofString());

        return handleResponse(resp, path);
    }

    private String handleResponse(HttpResponse<String> resp, String path)
            throws IOException {
        if (resp.statusCode() == 200) {
            return resp.body();
        }
        try {
            var node = Json.MAPPER.readTree(resp.body());
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
