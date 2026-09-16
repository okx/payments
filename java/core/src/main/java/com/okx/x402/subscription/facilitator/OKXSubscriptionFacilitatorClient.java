// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.facilitator;

import com.fasterxml.jackson.databind.JsonNode;
import com.okx.x402.facilitator.HttpExecutor;
import com.okx.x402.subscription.error.SubscriptionException;
import com.okx.x402.subscription.model.CancelAuth;
import com.okx.x402.subscription.model.PendingChangeCancelAuth;
import com.okx.x402.subscription.model.PermitSingle;
import com.okx.x402.subscription.model.SubscriptionTerms;
import com.okx.x402.subscription.model.resp.*;
import com.okx.x402.util.Json;
import com.okx.x402.util.OKXAuth;

import java.io.IOException;
import java.net.URI;
import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;

/**
 * Client for the OKX facilitator subscription API: all write endpoints are body-based (AK auth
 * forbids path placeholders), field names are {@code termsSig}/{@code permitSig}, and the detail
 * query is the public {@code GET /subscriptions/detail?subId=}.
 */
public class OKXSubscriptionFacilitatorClient implements SubscriptionFacilitatorClient {

    private static final String BASE_PATH = "/api/v6/pay/x402";
    private static final int MAX_RETRIES = 3;
    private static final long BASE_RETRY_DELAY_MS = 1000;

    /**
     * Outbound wire mapper: request bodies must carry every key explicitly — the shared
     * {@code Json.MAPPER} is NON_NULL and would silently drop null POJO fields, changing the
     * wire shape for the same input.
     */
    private static final com.fasterxml.jackson.databind.ObjectMapper WIRE_MAPPER =
            Json.MAPPER.copy().setSerializationInclusion(
                    com.fasterxml.jackson.annotation.JsonInclude.Include.ALWAYS);

    private final OKXAuth auth;
    private final HttpExecutor executor;
    private final String baseUrl;
    private final Duration requestTimeout;

    public OKXSubscriptionFacilitatorClient(OKXAuth auth, HttpExecutor executor,
                                            String baseUrl, Duration requestTimeout) {
        this.auth = Objects.requireNonNull(auth);
        this.executor = Objects.requireNonNull(executor);
        this.baseUrl = Objects.requireNonNull(baseUrl).replaceAll("/+$", "");
        this.requestTimeout = Objects.requireNonNull(requestTimeout);
    }

    @Override
    public CreateResp subscribe(long chainIndex, SubscriptionTerms terms, PermitSingle permit,
                                String termsSig, String permitSig,
                                boolean syncSettle) throws IOException, InterruptedException {
        // A buyer that omitted planId still produces "planId":"" on the wire — the facilitator
        // expects the key present, never missing / null.
        if (terms.planId == null) terms.planId = "";
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("chainIndex", chainIndex);
        body.put("terms", terms);
        body.put("permit", permit);
        body.put("termsSig", termsSig);
        body.put("permitSig", permitSig);
        body.put("syncSettle", syncSettle);

        String resp = doPost(BASE_PATH + "/subscriptions", WIRE_MAPPER.writeValueAsString(body));
        CreateResp r = Json.MAPPER.readValue(resp, CreateResp.class);
        requireState(r.state, "create");
        return r;
    }

    @Override
    public ChargeResp charge(String subId, boolean syncSettle)
            throws IOException, InterruptedException {
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("subId", subId);
        body.put("syncSettle", syncSettle);

        String resp = doPost(BASE_PATH + "/subscriptions/charge", WIRE_MAPPER.writeValueAsString(body));
        ChargeResp r = Json.MAPPER.readValue(resp, ChargeResp.class);
        requireState(r.state, "charge");
        return r;
    }

    @Override
    public ChangeResp change(long chainIndex, String oldSubId, SubscriptionTerms newTerms,
                             PermitSingle newPermit, String termsSig, String permitSig,
                             boolean syncSettle) throws IOException, InterruptedException {
        if (newTerms.planId == null) newTerms.planId = "";
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("chainIndex", chainIndex);
        body.put("oldSubId", oldSubId);
        body.put("newTerms", newTerms);
        body.put("permit", newPermit);
        body.put("termsSig", termsSig);
        body.put("permitSig", permitSig);
        body.put("syncSettle", syncSettle);

        String resp = doPost(BASE_PATH + "/subscriptions/change", WIRE_MAPPER.writeValueAsString(body));
        ChangeResp r = Json.MAPPER.readValue(resp, ChangeResp.class);
        requireState(r.state, "change");
        return r;
    }

    @Override
    public CancelResp cancel(String subId, CancelAuth cancelAuth, boolean syncSettle)
            throws IOException, InterruptedException {
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("subId", subId);
        body.put("cancelAuth", cancelAuth);
        body.put("syncSettle", syncSettle);

        String resp = doPost(BASE_PATH + "/subscriptions/cancel", WIRE_MAPPER.writeValueAsString(body));
        return Json.MAPPER.readValue(resp, CancelResp.class);
    }

    @Override
    public CancelPendingResp cancelPendingChange(String subId, PendingChangeCancelAuth cancelAuth,
                                                 boolean syncSettle)
            throws IOException, InterruptedException {
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("subId", subId);
        body.put("cancelAuth", cancelAuth);
        body.put("syncSettle", syncSettle);

        String resp = doPost(BASE_PATH + "/subscriptions/cancel-pending-change",
                WIRE_MAPPER.writeValueAsString(body));
        return Json.MAPPER.readValue(resp, CancelPendingResp.class);
    }

    @Override
    public FinalizeExpiredResp finalizeExpired(String subId) throws IOException, InterruptedException {
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("subId", subId);

        String resp = doPost(BASE_PATH + "/subscriptions/finalize-expired",
                WIRE_MAPPER.writeValueAsString(body));
        return Json.MAPPER.readValue(resp, FinalizeExpiredResp.class);
    }

    @Override
    public QueryResp getSubscription(String subId) throws IOException, InterruptedException {
        String resp = doGet(BASE_PATH + "/subscriptions/detail?subId=" + subId);
        return Json.MAPPER.readValue(resp, QueryResp.class);
    }

    @Override
    public ChargeListResp getCharges(String subId, int limit, int offset)
            throws IOException, InterruptedException {
        String resp = doGet(BASE_PATH + "/subscriptions/charges?subId=" + subId
                + "&limit=" + limit + "&offset=" + offset);
        return Json.MAPPER.readValue(resp, ChargeListResp.class);
    }

    /**
     * {@inheritDoc}
     *
     * @return the latest pending-change row, or {@code null} when the subscription has none
     *         (the facilitator answers an all-null body, which must not deserialize into a row
     *         whose primitive {@code state} reads 0 = PENDING).
     */
    @Override
    public PendingChangeResp getPendingChange(String subId) throws IOException, InterruptedException {
        String resp = doGet(BASE_PATH + "/subscriptions/pending?subId=" + subId);
        PendingChangeResp r = Json.MAPPER.readValue(resp, PendingChangeResp.class);
        return r == null || r.subId == null ? null : r;
    }

    @Override
    public AllowanceStatusResp getAllowanceStatus(String buyer, String token, long chainIndex)
            throws IOException, InterruptedException {
        String resp = doGet(BASE_PATH + "/buyers/" + buyer
                + "/allowance-status?token=" + token + "&chainIndex=" + chainIndex);
        return Json.MAPPER.readValue(resp, AllowanceStatusResp.class);
    }

    @Override
    public BuyerSubscriptionListResp getBuyerSubscriptions(String buyer, int limit, int offset)
            throws IOException, InterruptedException {
        String resp = doGet(BASE_PATH + "/buyers/" + buyer
                + "/subscriptions?limit=" + limit + "&offset=" + offset);
        return Json.MAPPER.readValue(resp, BuyerSubscriptionListResp.class);
    }

    private String doPost(String path, String body) throws IOException, InterruptedException {
        for (int attempt = 0; ; attempt++) {
            Map<String, String> headers = this.auth.createHeaders("POST", path, body != null ? body : "");
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
            Map<String, String> headers = this.auth.createHeaders("GET", path, "");
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
     * {@code state} is a required field on write responses — a partial/async backend write must
     * surface loudly instead of reading as a fabricated pending(0).
     */
    private static void requireState(Integer state, String op) throws SubscriptionException {
        if (state == null) {
            throw new SubscriptionException(
                    com.okx.x402.subscription.error.SubscriptionErrorCodes.SYSTEM_ERROR,
                    "facilitator " + op + " response missing state");
        }
    }

    private boolean shouldRetry(HttpExecutor.HttpExecResult resp, int attempt) {
        if (attempt >= MAX_RETRIES) return false;
        if (resp.statusCode() == 429) return true;
        if (is2xx(resp.statusCode())) {
            try {
                JsonNode root = Json.MAPPER.readTree(resp.body());
                if (root.has("code") && "50011".equals(root.get("code").asText())) return true;
            } catch (Exception ignored) {}
        }
        return false;
    }

    /** Any 2xx counts as HTTP success (gateways may answer 201/202/204). */
    private static boolean is2xx(int statusCode) {
        return statusCode >= 200 && statusCode < 300;
    }

    private String handleResponse(HttpExecutor.HttpExecResult resp, String path)
            throws IOException {
        if (!is2xx(resp.statusCode())) {
            throw parseError(resp, path);
        }

        String body = resp.body();
        JsonNode root;
        try {
            root = Json.MAPPER.readTree(body);
        } catch (Exception e) {
            // A 2xx that is not JSON is anomalous — surfacing it beats handing the raw text
            // to a downstream readValue that would fabricate an all-default object.
            throw new IOException("unparseable facilitator response on " + path);
        }
        // Envelope gate is `code` ONLY: a business error without a data key must never fall
        // through and deserialize into an all-null "success".
        if (root != null && root.has("code")) {
            String codeStr = root.get("code").asText();
            if (!"0".equals(codeStr)) {
                String msg = envelopeMessage(root, codeStr);
                throw new SubscriptionException(extractCode(msg),
                        "Facilitator error: " + msg + " on " + path);
            }
            JsonNode data = root.get("data");
            return data == null || data.isNull() ? "{}" : Json.MAPPER.writeValueAsString(data);
        }
        return body;
    }

    /**
     * Error-message extraction priority: {@code error_message} → {@code msg} → the code itself.
     * Null JSON nodes are skipped — a null {@code msg} node must never surface as the literal
     * string "null", which would mis-drive {@code SubscriptionErrorCodes.classify}.
     */
    private static String envelopeMessage(JsonNode root, String fallback) {
        String m = textOrNull(root, "error_message");
        if (m == null) m = textOrNull(root, "msg");
        return m != null ? m : fallback;
    }

    private static String textOrNull(JsonNode node, String key) {
        JsonNode v = node.get(key);
        return v != null && !v.isNull() && !v.asText().isEmpty() ? v.asText() : null;
    }

    private IOException parseError(HttpExecutor.HttpExecResult resp, String path) {
        try {
            JsonNode node = Json.MAPPER.readTree(resp.body());
            String msg = envelopeMessage(node, "HTTP " + resp.statusCode());
            return new SubscriptionException(extractCode(msg),
                    "Facilitator error: " + msg + " on " + path);
        } catch (Exception e) {
            return new IOException("HTTP " + resp.statusCode() + " on " + path + ": " + resp.body());
        }
    }

    /**
     * The facilitator interpolates a detail suffix into {@code msg}
     * ({@code "subscription_not_active: state=3"}); the machine-readable code is the segment
     * before the first colon. Exact-match consumers (self-heal switch,
     * {@code SubscriptionErrorCodes.classify}) need that bare code, so strip the suffix here
     * and keep the full message on the exception for humans.
     */
    private static String extractCode(String msg) {
        if (msg == null) return null;
        int colon = msg.indexOf(':');
        return (colon > 0 ? msg.substring(0, colon) : msg).trim();
    }
}
