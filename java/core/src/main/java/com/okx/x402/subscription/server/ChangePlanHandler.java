// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server;

import com.okx.x402.server.AcceptOption;
import com.okx.x402.server.PaymentHooks;
import com.okx.x402.server.PaymentProcessor;
import com.okx.x402.server.X402Request;
import com.okx.x402.server.X402Response;
import com.okx.x402.subscription.model.AccessProof;
import com.okx.x402.subscription.server.access.AccessProofVerifier;
import com.okx.x402.subscription.server.access.PlanCatalog;
import com.okx.x402.subscription.server.store.StoredSubscription;
import com.okx.x402.subscription.server.store.SubscriptionStore;
import com.okx.x402.util.Json;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;

/**
 * Seller-side /changePlan route:
 *
 * <ol>
 *   <li><b>Offer phase</b> — buyer calls with only APP-Access: verify the proof, look up the
 *       buyer's current plan via subId, then return 402 + PAYMENT-REQUIRED whose accepts list
 *       carries every OTHER plan with a filled-in {@code changeFrom} block. The buyer's current
 *       plan is removed — and so is any plan sharing its TIER (the contract rejects same-tier
 *       changes, so such an offer would be dead on arrival). Downgrade offers drop their
 *       {@code initialCharge} (contract rejects scheduleDowngrade with a non-zero initial).</li>
 *   <li><b>Execute phase</b> — buyer signs newTerms(+permit) against the changeFrom metadata and
 *       re-requests with PAYMENT-SIGNATURE (legacy X-APP-PAYMENT accepted): delegated to
 *       {@link SubscriptionSchemeHandler} WITH the catalog offers for terms↔offer binding.</li>
 * </ol>
 *
 * <p>Prefer the processor-native form (RouteConfig.subscriptionOperation = CHANGE) for new
 * integrations; this standalone handler remains for servlet-style wiring.
 */
public class ChangePlanHandler {

    /** Server-level offer context shared by every plan option. */
    public static class OfferContext {
        public long chainIndex;
        public String network;                // CAIP-2, e.g. "eip155:196"
        public String subscriptionContract;
        public String permit2Contract;
        public String facilitatorAddress;     // facilitator EOA from /supported
    }

    private final PlanCatalog catalog;
    private final SubscriptionStore store;
    private final AccessProofVerifier verifier;
    private final SubscriptionSchemeHandler schemeHandler;
    private final OfferContext context;

    public ChangePlanHandler(PlanCatalog catalog, SubscriptionStore store,
                             SubscriptionSchemeHandler schemeHandler, OfferContext context) {
        this.catalog = Objects.requireNonNull(catalog);
        this.store = Objects.requireNonNull(store);
        this.schemeHandler = Objects.requireNonNull(schemeHandler);
        this.context = Objects.requireNonNull(context);
        this.verifier = new AccessProofVerifier();
    }

    /**
     * Handle a /changePlan request. Writes the response (402 offer, 200 change result, or 4xx
     * error) and returns true when the request was consumed.
     */
    public boolean handle(X402Request request, X402Response response) throws IOException {
        // Execute phase: a signed payment header means the buyer already picked a plan.
        String paymentHeader = request.getHeader(SubscriptionSchemeHandler.HEADER_PAYMENT_SIGNATURE);
        if (paymentHeader == null || paymentHeader.isEmpty()) {
            paymentHeader = request.getHeader(SubscriptionSchemeHandler.HEADER_APP_PAYMENT);
        }
        if (paymentHeader != null && !paymentHeader.isEmpty()) {
            PaymentProcessor.RouteConfig binding = new PaymentProcessor.RouteConfig();
            binding.scheme = PaymentProcessor.SCHEME_PERIOD;
            binding.accepts = catalogAccepts();
            PaymentHooks.ProtectedRequestResult result =
                    schemeHandler.handle(request, response, binding);
            if (result == null) {
                writeError(response, 400, "invalid payment header");
            } else if (result.decision
                    == PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS) {
                writeJson(response, 200, "{\"status\":\"changed\"}");
            } else {
                writeError(response, 402, result.reason != null ? result.reason : "change failed");
            }
            return true;
        }

        // Offer phase: requires a valid AccessProof identifying the buyer's current subscription.
        AccessProof proof = verifier.decode(AccessProofVerifier.readAccessHeader(request));
        if (proof == null) {
            writeError(response, 401, "missing or invalid access proof");
            return true;
        }
        String sigError = verifier.verify(proof);
        if (sigError != null) {
            writeError(response, 401, sigError);
            return true;
        }

        StoredSubscription sub = store.get(proof.subId);
        if (sub == null || sub.payer == null || !sub.payer.equalsIgnoreCase(proof.payer)) {
            writeError(response, 404, "subscription not found");
            return true;
        }

        // Change offers are gated on the current period being paid — an unpaid subscription
        // must not receive the change-plan menu (information leak).
        if (!schemeHandler.currentPeriodCharged(sub)) {
            writeError(response, 402,
                    com.okx.x402.subscription.error.SubscriptionErrorCodes.SUBSCRIPTION_PERIOD_UNPAID);
            return true;
        }

        List<Map<String, Object>> accepts = SubscriptionSchemeHandler.buildChangeAccepts(
                request, catalogAccepts(), sub.subId, sub.planId, sub.planTier);

        Map<String, Object> paymentRequired = new LinkedHashMap<>();
        paymentRequired.put("x402Version", 2);
        paymentRequired.put("accepts", accepts);
        String json = Json.MAPPER.writeValueAsString(paymentRequired);
        String b64 = Base64.getEncoder().encodeToString(json.getBytes(StandardCharsets.UTF_8));

        response.setStatus(X402Response.SC_PAYMENT_REQUIRED);
        response.setContentType("application/json; charset=UTF-8");
        response.setHeader("PAYMENT-REQUIRED", b64);
        response.setHeader(SubscriptionSchemeHandler.HEADER_APP_PAYMENT_REQUIRED, b64);
        response.setHeader("Access-Control-Expose-Headers",
                "PAYMENT-REQUIRED," + SubscriptionSchemeHandler.HEADER_APP_PAYMENT_REQUIRED);
        response.writeBody(json);
        return true;
    }

    /**
     * The catalog as AcceptOptions (one per plan): plaintext plan.id, nested
     * {@code initialCharge {periodCount, totalAmount, coversFirstPeriods}} (omitted when zero),
     * scheme {@code period}.
     */
    List<AcceptOption> catalogAccepts() {
        List<AcceptOption> out = new ArrayList<>();
        for (Map.Entry<String, PlanCatalog.PlanEntry> entry : catalog.all().entrySet()) {
            out.add(toAcceptOption(entry.getKey(), entry.getValue(), context));
        }
        return out;
    }

    /** Build one subscription AcceptOption from a catalog plan (shared with demos). */
    public static AcceptOption toAcceptOption(String planId, PlanCatalog.PlanEntry plan,
                                              OfferContext ctx) {
        Map<String, Object> planInfo = new LinkedHashMap<>();
        planInfo.put("id", planId);
        planInfo.put("tier", plan.tier);
        if (!plan.features.isEmpty()) {
            planInfo.put("features", plan.features);
        }

        Map<String, Object> extra = new LinkedHashMap<>();
        extra.put("transferMethod", "permit2");
        extra.put("chainIndex", ctx.chainIndex);
        extra.put("contracts", Map.of(
                "subscription", ctx.subscriptionContract,
                "permit2", ctx.permit2Contract));
        extra.put("facilitator", ctx.facilitatorAddress);
        extra.put("amountPerPeriod", plan.amountPerPeriod);
        extra.put("periodSec", plan.periodSec);
        extra.put("periodMode", plan.periodMode);
        extra.put("maxPeriods", plan.maxPeriods);
        extra.put("startAt", 0);
        if (plan.initialChargePeriods > 0) {
            Map<String, Object> initialCharge = new LinkedHashMap<>();
            initialCharge.put("periodCount", plan.initialChargePeriods);
            initialCharge.put("totalAmount", plan.initialChargeAmount);
            initialCharge.put("coversFirstPeriods", true);
            extra.put("initialCharge", initialCharge);
        }
        extra.put("plan", planInfo);
        extra.put("domain", Map.of(
                "name", "A2APaySubscription",
                "version", "1",
                "chainId", ctx.chainIndex,
                "verifyingContract", ctx.subscriptionContract));

        return AcceptOption.builder()
                .scheme(PaymentProcessor.SCHEME_PERIOD)
                .network(ctx.network)
                .payTo(plan.payTo)
                .asset(plan.asset)
                .price(plan.price != null ? plan.price
                        : (plan.initialChargePeriods > 0 ? plan.initialChargeAmount
                                : plan.amountPerPeriod))
                .extra(extra)
                .build();
    }

    private void writeJson(X402Response response, int status, String json) throws IOException {
        response.setStatus(status);
        response.setContentType("application/json; charset=UTF-8");
        response.writeBody(json);
    }

    private void writeError(X402Response response, int status, String reason) throws IOException {
        writeJson(response, status, "{\"error_code\":\"" + reason + "\"}");
    }
}
