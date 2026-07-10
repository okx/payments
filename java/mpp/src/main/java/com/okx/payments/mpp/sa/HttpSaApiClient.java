package com.okx.payments.mpp.sa;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.SerializationFeature;
import com.okx.payments.mpp.errors.MppError;
import com.okx.payments.mpp.errors.SaErrorMapper;
import com.okx.payments.mpp.errors.ServiceError;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.protocol.session.payload.SessionStatusResponse;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpRequest.BodyPublishers;
import java.net.http.HttpResponse;
import java.net.http.HttpResponse.BodyHandlers;
import java.nio.charset.StandardCharsets;
import java.util.Map;

/**
 * Default {@link SaApiClient} implementation backed by JDK {@link HttpClient}.
 *
 * <p>Wire shape:
 * <ul>
 *   <li>POST/GET to {@code config.baseUrl + /api/v6/pay/mpp/<endpoint>}</li>
 *   <li>OKX-AK headers via {@link OkxAuth}</li>
 *   <li>Outer envelope: {@code {code:int, msg:str, data:{...}}}; non-zero code → {@link MppError}</li>
 * </ul>
 */
public final class HttpSaApiClient implements SaApiClient {

    private static final Logger log = LoggerFactory.getLogger(HttpSaApiClient.class);

    private final SaApiConfig config;
    private final HttpClient http;
    private final OkxAuth auth;
    private final ObjectMapper mapper;

    public HttpSaApiClient(SaApiConfig config) {
        this(config, HttpClient.newBuilder().connectTimeout(config.connectTimeout()).build(),
            new OkxAuth(config),
            new ObjectMapper().disable(SerializationFeature.WRITE_DATES_AS_TIMESTAMPS));
    }

    public HttpSaApiClient(SaApiConfig config, HttpClient http, OkxAuth auth, ObjectMapper mapper) {
        this.config = config;
        this.http = http;
        this.auth = auth;
        this.mapper = mapper;
    }

    @Override
    public Receipt.ChargeReceipt chargeSettle(Object body) {
        return post("/charge/settle", body, Receipt.ChargeReceipt.class);
    }

    @Override
    public Receipt.ChargeReceipt chargeVerifyHash(Object body) {
        return post("/charge/verifyHash", body, Receipt.ChargeReceipt.class);
    }

    @Override
    public Receipt.SessionReceipt sessionOpen(Object body) {
        return post("/session/open", body, Receipt.SessionReceipt.class);
    }

    @Override
    public Receipt.SessionReceipt sessionTopUp(Object body) {
        return post("/session/topUp", body, Receipt.SessionReceipt.class);
    }

    @Override
    public Receipt.SessionReceipt sessionSettle(Object body) {
        return post("/session/settle", body, Receipt.SessionReceipt.class);
    }

    @Override
    public Receipt.SessionReceipt sessionClose(Object body) {
        return post("/session/close", body, Receipt.SessionReceipt.class);
    }

    @Override
    public SessionStatusResponse sessionStatus(String channelId) {
        String q = "?channelId=" + URLEncoder.encode(channelId, StandardCharsets.UTF_8);
        return get("/session/status", q, SessionStatusResponse.class)
            .data();
    }

    // ── internals ─────────────────────────────────────────────────────────────

    private <T> T post(String endpoint, Object body, Class<T> dataType) {
        String path = SaApiConfig.API_PREFIX + endpoint;
        String json;
        try {
            json = mapper.writeValueAsString(body);
        } catch (Exception e) {
            throw new ServiceError("serialization failed for " + endpoint, e);
        }
        HttpRequest.Builder rb = HttpRequest.newBuilder(URI.create(config.baseUrl() + path))
            .timeout(config.readTimeout())
            .POST(BodyPublishers.ofString(json, StandardCharsets.UTF_8));
        Map<String, String> headers = auth.headers("POST", path, json);
        headers.forEach(rb::header);

        return execute(endpoint, rb.build(), dataType).data();
    }

    private <T> Envelope<T> get(String endpoint, String query, Class<T> dataType) {
        String path = SaApiConfig.API_PREFIX + endpoint;
        String pathWithQuery = path + (query == null ? "" : query);
        HttpRequest.Builder rb = HttpRequest.newBuilder(URI.create(config.baseUrl() + pathWithQuery))
            .timeout(config.readTimeout())
            .GET();
        Map<String, String> headers = auth.headers("GET", pathWithQuery, null);
        headers.forEach(rb::header);
        return execute(endpoint, rb.build(), dataType);
    }

    private <T> Envelope<T> execute(String endpoint, HttpRequest req, Class<T> dataType) {
        try {
            HttpResponse<String> resp = http.send(req, BodyHandlers.ofString(StandardCharsets.UTF_8));
            String body = resp.body();
            if (resp.statusCode() < 200 || resp.statusCode() >= 300) {
                throw new ServiceError("HTTP " + resp.statusCode() + " from " + endpoint
                    + " body=" + truncate(body));
            }
            JsonNode tree = mapper.readTree(body);
            int code = tree.path("code").asInt(-1);
            String msg = tree.path("msg").asText("");
            JsonNode data = tree.path("data");
            if (code != 0) {
                MppError err = SaErrorMapper.map(code, "[" + endpoint + "] " + msg);
                err.put("endpoint", endpoint);
                err.put("saCode", code);
                throw err;
            }
            T parsed = data.isMissingNode() || data.isNull() ? null : mapper.treeToValue(data, dataType);
            return new Envelope<>(code, msg, parsed);
        } catch (IOException | InterruptedException e) {
            if (e instanceof InterruptedException) {
                Thread.currentThread().interrupt();
            }
            ServiceError err = new ServiceError("HTTP transport failure on " + endpoint, e);
            err.put("endpoint", endpoint);
            throw err;
        }
    }

    private static String truncate(String s) {
        return s == null ? "" : (s.length() > 200 ? s.substring(0, 200) + "..." : s);
    }

    @JsonIgnoreProperties(ignoreUnknown = true)
    public record Envelope<T>(
        @JsonProperty("code") int code,
        @JsonProperty("msg") String msg,
        @JsonProperty("data") T data) {}
}
