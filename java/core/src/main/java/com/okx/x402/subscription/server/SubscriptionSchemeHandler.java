// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server;

import com.fasterxml.jackson.databind.JsonNode;
import com.okx.x402.config.AssetRegistry;
import com.okx.x402.config.ResolvedPrice;
import com.okx.x402.server.AcceptOption;
import com.okx.x402.server.PaymentHooks;
import com.okx.x402.server.PaymentHooks.ProtectedRequestResult.AbortClass;
import com.okx.x402.server.PaymentProcessor;
import com.okx.x402.server.X402Request;
import com.okx.x402.server.X402Response;
import com.okx.x402.subscription.eip712.SubscriptionEip712;
import com.okx.x402.subscription.error.SubscriptionErrorCodes;
import com.okx.x402.subscription.error.SubscriptionException;
import com.okx.x402.subscription.facilitator.SubscriptionFacilitatorClient;
import com.okx.x402.subscription.model.AccessProof;
import com.okx.x402.subscription.model.CancelAuth;
import com.okx.x402.subscription.model.PendingChangeCancelAuth;
import com.okx.x402.subscription.model.PermitSingle;
import com.okx.x402.subscription.model.SubscriptionTerms;
import com.okx.x402.subscription.model.enums.PeriodMode;
import com.okx.x402.subscription.model.enums.SubscriptionState;
import com.okx.x402.subscription.model.resp.CancelPendingResp;
import com.okx.x402.subscription.model.resp.CancelResp;
import com.okx.x402.subscription.model.resp.ChangeResp;
import com.okx.x402.subscription.model.resp.CreateResp;
import com.okx.x402.subscription.model.resp.QueryResp;
import com.okx.x402.subscription.server.access.AccessProofVerifier;
import com.okx.x402.subscription.server.store.StoredSubscription;
import com.okx.x402.subscription.server.store.SubscriptionStore;
import com.okx.x402.util.Json;
import org.web3j.utils.Numeric;

import java.io.IOException;
import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;

/**
 * Seller-side handler for the {@code period} subscription scheme:
 *
 * <ul>
 *   <li>Inbound payment header: canonical {@code PAYMENT-SIGNATURE} (x402 V2 wrapped payload
 *       {@code {x402Version, accepted, payload:{permitSingle, permitSingleSignature, terms,
 *       termsSignature}}}), with the legacy {@code X-APP-PAYMENT} flat shape still accepted.</li>
 *   <li>Offer binding ({@code verifyTermsMatchOffer}): buyer-signed terms must match an
 *       advertised plan on the route (anti price/merchant/token tamper).</li>
 *   <li>Result header: {@code PAYMENT-RESPONSE} = base64({subId, txHash, state}) (+ legacy
 *       {@code X-APP-PAYMENT-RESPONSE} alias).</li>
 *   <li>Operation routes: change offers on APP-Access; cancel / cancel-pending-change relay a
 *       buyer-signed auth from the request body.</li>
 * </ul>
 */
public class SubscriptionSchemeHandler {

    private static final System.Logger LOG =
            System.getLogger(SubscriptionSchemeHandler.class.getName());

    /** Canonical inbound payment header (x402 V2). */
    public static final String HEADER_PAYMENT_SIGNATURE = "PAYMENT-SIGNATURE";
    /** Canonical result header. */
    public static final String HEADER_PAYMENT_RESPONSE = "PAYMENT-RESPONSE";
    /** Legacy header names (earlier releases); still honored. */
    public static final String HEADER_APP_PAYMENT = "X-APP-PAYMENT";
    public static final String HEADER_APP_PAYMENT_RESPONSE = "X-APP-PAYMENT-RESPONSE";
    public static final String HEADER_APP_PAYMENT_REQUIRED = "X-APP-PAYMENT-REQUIRED";

    private static final String ZERO_BYTES32 = "0x" + "0".repeat(64);

    /**
     * Writes the plain subscribe-offers 402 for this route. Wired by
     * {@link PaymentProcessor#subscriptionHandler} so the CHANGE route can degrade an
     * invalid/unknown/unpaid access proof into "here is how to subscribe" instead of a
     * bare error.
     */
    @FunctionalInterface
    public interface SubscribeOfferWriter {
        void write402(X402Request request, X402Response response,
                      PaymentProcessor.RouteConfig config, String error) throws IOException;
    }

    private final SubscriptionFacilitatorClient facilitator;
    private final SubscriptionStore store;
    private final String subscriptionContract;
    private final SubscriptionSettlePoller poller;
    private final AccessProofVerifier accessVerifier = new AccessProofVerifier();
    private final List<SubscriptionHooks.OnBeforeAccessHook> beforeAccessHooks = new ArrayList<>();
    private SubscribeOfferWriter subscribeOfferWriter;

    public SubscriptionSchemeHandler(SubscriptionFacilitatorClient facilitator,
                                     SubscriptionStore store,
                                     String subscriptionContract) {
        this.facilitator = Objects.requireNonNull(facilitator);
        this.store = Objects.requireNonNull(store);
        this.subscriptionContract = Objects.requireNonNull(subscriptionContract);
        this.poller = new SubscriptionSettlePoller(facilitator);
    }

    /** See {@link SubscribeOfferWriter}; set automatically by the processor wiring. */
    public void subscribeOfferWriter(SubscribeOfferWriter writer) {
        this.subscribeOfferWriter = writer;
    }

    /**
     * Register a merchant access-override hook for the CHANGE-route offer phase: the veto
     * fires before the period gate; Denied → bare 402, no offers, no degradation. A veto list
     * used with the processor-native integration must be registered BOTH here and on the
     * AccessProofHook — the two gates protect different routes.
     */
    public SubscriptionSchemeHandler onBeforeAccess(SubscriptionHooks.OnBeforeAccessHook hook) {
        beforeAccessHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    /** Compat overload: no route config → offer binding is skipped (documented legacy path). */
    public PaymentHooks.ProtectedRequestResult handle(X402Request request, X402Response response)
            throws IOException {
        return handle(request, response, null);
    }

    public PaymentHooks.ProtectedRequestResult handle(X402Request request, X402Response response,
                                                      PaymentProcessor.RouteConfig config)
            throws IOException {
        String header = paymentHeader(request);
        if (header == null || header.isEmpty()) {
            return null;
        }

        SubscriptionPayment payment;
        try {
            payment = decodePayment(header);
        } catch (Exception e) {
            return PaymentHooks.ProtectedRequestResult.abort(
                    "invalid subscription payment header", AbortClass.PAYMENT_FAILED);
        }

        String preCheckError;
        try {
            preCheckError = runPreChecks(payment.terms, payment.permit);
        } catch (RuntimeException e) {
            // Pre-checks hash buyer-controlled bytes; any encoder blow-up on hostile input must
            // stay a structured 402, never escape the filter as a container 500.
            preCheckError = SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS;
        }
        if (preCheckError != null) {
            return PaymentHooks.ProtectedRequestResult.abort(preCheckError,
                    AbortClass.PAYMENT_FAILED);
        }

        // Offer binding (verifyTermsMatchOffer): the facilitator verifies signatures but
        // cannot see the offer, so without this a buyer could sign cheaper / mismatched economics.
        // Uses effectiveAccepts() so route-level payTo/asset defaults apply here exactly as they
        // do in the advertised 402 envelope.
        if (config != null && config.accepts != null && !config.accepts.isEmpty()) {
            List<AcceptOption> accepts = config.effectiveAccepts();

            // The buyer's accepted echo must name a
            // subscription option this route actually advertises — the buyer does not get to
            // pick the network (chainIndex) unilaterally.
            if (payment.acceptedNetwork != null && !payment.acceptedNetwork.isEmpty()) {
                AcceptOption matchedOption = null;
                for (AcceptOption option : accepts) {
                    if (PaymentProcessor.isSubscriptionSchemeName(option.scheme)
                            && payment.acceptedNetwork.equals(option.network)
                            && (payment.acceptedScheme == null || payment.acceptedScheme.isEmpty()
                                || PaymentProcessor.isSubscriptionSchemeName(payment.acceptedScheme))) {
                        matchedOption = option;
                        break;
                    }
                }
                if (matchedOption == null) {
                    return PaymentHooks.ProtectedRequestResult.abort(
                            "payment does not match any accepted payment option",
                            AbortClass.PAYMENT_FAILED);
                }
                // The settlement chain derives from the ROUTE's matched option,
                // never from a buyer-supplied chainIndex field — a wrapped payload echoing a
                // legal network must not be able to steer settlement onto another chain.
                // Unconditional: an unparseable route network yields 0 and is rejected below,
                // instead of letting an explicit buyer chainIndex survive the override.
                payment.chainIndex = chainIndexOf(matchedOption.network);
            }

            String bindError = verifyTermsMatchOffer(payment.terms, accepts);
            if (bindError != null) {
                return PaymentHooks.ProtectedRequestResult.abort(bindError,
                        AbortClass.PAYMENT_FAILED);
            }
        }

        // Never forward an unresolved chain to the facilitator: a 0 here would surface as an
        // opaque facilitator-side error, so reject locally with a clear code.
        if (payment.chainIndex <= 0) {
            return PaymentHooks.ProtectedRequestResult.abort(
                    "unsupported network for subscription", AbortClass.PAYMENT_FAILED);
        }

        // syncSettle is tri-state: a route may opt into async settlement submission; unset
        // (null) means synchronous.
        boolean syncSettle = config == null || config.syncSettle == null || config.syncSettle;

        try {
            if (isChange(payment.terms)) {
                return handleChange(payment, response, syncSettle);
            } else {
                return handleSubscribe(payment, response, syncSettle);
            }
        } catch (SubscriptionException e) {
            return PaymentHooks.ProtectedRequestResult.abort(e.getCode(),
                    AbortClass.PAYMENT_FAILED);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return PaymentHooks.ProtectedRequestResult.abort("interrupted",
                    AbortClass.PAYMENT_FAILED);
        }
    }

    // ------------------------------------------------------------------
    // Operation routes
    // ------------------------------------------------------------------

    /**
     * CHANGE operation, offer phase: an APP-Access proof means "show me the plans I can switch
     * to". Writes 402 + change-annotated accepts (buyer's current plan / tier dropped; downgrade
     * offers lose their initialCharge) and returns true. Returns false when a payment header is
     * present (caller proceeds with the normal subscribe/change path) or when neither header is
     * present (caller emits the plain 402 offers).
     */
    public boolean handleChangeOffers(X402Request request, X402Response response,
                                      PaymentProcessor.RouteConfig config) throws IOException {
        // Header precedence: an APP-Access proof means "show me my change options" even
        // when a payment header is also present; only its absence falls through to the
        // execute (payment) path or the plain 402.
        String access = AccessProofVerifier.readAccessHeader(request);
        if (access == null || access.isEmpty()) {
            return false;
        }

        AccessProof proof = accessVerifier.decode(access);
        if (proof == null) {
            degradeToSubscribeOffers(request, response, config, "invalid access proof");
            return true;
        }
        String sigError = accessVerifier.verify(proof);
        if (sigError != null) {
            degradeToSubscribeOffers(request, response, config, sigError);
            return true;
        }

        StoredSubscription sub = store.get(proof.subId);
        if (sub == null) {
            sub = rehydrateFromDetail(proof.subId);
        }
        if (sub == null || sub.payer == null || !sub.payer.equalsIgnoreCase(proof.payer)) {
            degradeToSubscribeOffers(request, response, config, "subscription not found");
            return true;
        }

        // Merchant veto: fires BEFORE the period gate and answers a
        // bare 402 — never offers, never degradation. A vetoed buyer must not be invited to
        // subscribe or change plans.
        for (SubscriptionHooks.OnBeforeAccessHook hook : beforeAccessHooks) {
            SubscriptionHooks.AccessDecision decision = hook.beforeAccess(proof, sub);
            if (decision != null && decision.denied) {
                writeError(response, 402, decision.reason != null ? decision.reason
                        : SubscriptionErrorCodes.ACCESS_DENIED_BY_MERCHANT);
                return true;
            }
        }

        // The change-offer phase enforces the period gate too — an unpaid/expired
        // subscription degrades to plain subscribe offers, not change offers.
        if (!currentPeriodCharged(sub)) {
            degradeToSubscribeOffers(request, response, config,
                    SubscriptionErrorCodes.SUBSCRIPTION_PERIOD_UNPAID);
            return true;
        }

        List<Map<String, Object>> accepts = buildChangeAccepts(
                request, config.effectiveAccepts(), proof.subId, sub.planId, sub.planTier);
        if (accepts.isEmpty()) {
            // Nothing to change to → bare 402 (offers would be an empty lie).
            writeError(response, 402, "no change-plan options available");
            return true;
        }
        writeOffers(response, request, accepts, null);
        return true;
    }

    /**
     * Change-route degradation: an invalid / unknown / unpaid access proof on the CHANGE
     * route answers the plain subscribe-offers 402 (how to become a subscriber) rather than a
     * bare error. Falls back to the bare error when the handler is used standalone (no
     * processor wiring).
     */
    private void degradeToSubscribeOffers(X402Request request, X402Response response,
                                          PaymentProcessor.RouteConfig config, String reason)
            throws IOException {
        if (subscribeOfferWriter != null && config != null) {
            subscribeOfferWriter.write402(request, response, config, reason);
        } else {
            writeError(response, 402, reason);
        }
    }

    /**
     * Period gate for the change-offer phase: local fast path when the store row is usable,
     * else authoritative detail. Mirrors the AccessProofHook judgment. Package-private so the
     * standalone {@link ChangePlanHandler} reuses the same gate.
     */
    boolean currentPeriodCharged(StoredSubscription sub) {
        long now = System.currentTimeMillis() / 1000;
        boolean anchorKnown = !PeriodMode.isCalendarMonth(sub.periodMode)
                || sub.billingAnchorAt > 0;
        if (sub.startAt > 0 && anchorKnown && sub.lastChargedPeriod != null) {
            long elapsed = com.okx.x402.subscription.support.PeriodMath.elapsedPeriods(
                    sub.periodMode, sub.startAt, sub.billingAnchorAt, sub.periodSec, now);
            if (elapsed > 0 && sub.lastChargedPeriod >= elapsed) {
                return true;
            }
        }
        try {
            QueryResp latest = facilitator.getSubscription(sub.subId);
            if (latest == null) {
                return false;
            }
            // The change-offer slow path write-throughs the fetched detail so the execute
            // phase / next access reuses it instead of re-querying.
            sub.applyDetail(latest);
            store.put(sub);
            return SubscriptionSettlePoller.settled(latest);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return false;
        } catch (Exception e) {
            return false;
        }
    }

    /** CANCEL / CANCEL_PENDING_CHANGE: relay the buyer-signed auth carried in the body. */
    public void handleCancelOperation(X402Request request, X402Response response,
                                      PaymentProcessor.SubscriptionOperation op)
            throws IOException {
        JsonNode root;
        try {
            root = Json.MAPPER.readTree(request.getBody());
        } catch (Exception e) {
            writeError(response, 400, "invalid or missing request body");
            return;
        }
        String subId = root.path("subId").asText("");
        JsonNode authNode = root.get("cancelAuth");
        if (authNode == null) {
            writeError(response, 400, "cancelAuth required");
            return;
        }

        try {
            if (op == PaymentProcessor.SubscriptionOperation.CANCEL_PENDING_CHANGE) {
                PendingChangeCancelAuth auth =
                        Json.MAPPER.treeToValue(authNode, PendingChangeCancelAuth.class);
                if (isBlank(auth.subId)) {
                    writeError(response, 400, "cancel-pending auth missing subId");
                    return;
                }
                if (isBlank(auth.newSubId)) {
                    writeError(response, 400, "cancel-pending auth missing newSubId");
                    return;
                }
                CancelPendingResp resp = facilitator.cancelPendingChange(
                        subId.isEmpty() ? auth.subId : subId, auth, true);
                rehydrateFromDetail(auth.subId); // full store refresh from chain truth
                writeJson(response, 200, Json.MAPPER.writeValueAsString(resp));
            } else {
                CancelAuth auth = Json.MAPPER.treeToValue(authNode, CancelAuth.class);
                if (isBlank(auth.subId)) {
                    writeError(response, 400, "cancel auth missing subId");
                    return;
                }
                CancelResp resp = facilitator.cancel(
                        subId.isEmpty() ? auth.subId : subId, auth, true);
                rehydrateFromDetail(auth.subId); // full store refresh from chain truth
                writeJson(response, 200, Json.MAPPER.writeValueAsString(resp));
            }
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            writeError(response, 502, "interrupted");
        } catch (Exception e) {
            writeError(response, 502, e.getMessage() == null ? "facilitator error"
                    : e.getMessage().replace('"', '\''));
        }
    }

    /**
     * Change-annotated accepts: every subscription plan on the route
     * except the buyer's current one (same plan id OR same tier — the contract rejects same-tier
     * changes, so such an offer would be dead on arrival). Downgrade offers drop initialCharge
     * (contract rejects scheduleDowngrade with a non-zero initial charge).
     */
    public static List<Map<String, Object>> buildChangeAccepts(List<AcceptOption> accepts,
            String fromSubId, String fromPlanId, int fromPlanTier) {
        return buildChangeAccepts(null, accepts, fromSubId, fromPlanId, fromPlanTier);
    }

    /**
     * Request-aware variant: change accepts are built with the SAME rules as the subscribe 402
     * entries — dynamic priceFunction resolution, registry extra (EIP-712
     * name/version/transferMethod) merged under the option extra, and warn-and-skip on an
     * unresolvable price (a raw "$0.01" config value must never leak onto the wire).
     */
    public static List<Map<String, Object>> buildChangeAccepts(X402Request request,
            List<AcceptOption> accepts, String fromSubId, String fromPlanId, int fromPlanTier) {
        List<Map<String, Object>> out = new ArrayList<>();
        if (accepts == null) {
            return out;
        }
        for (AcceptOption option : accepts) {
            if (!PaymentProcessor.isSubscriptionSchemeName(option.scheme)) {
                continue;
            }
            Map<String, Object> extra = option.extra;
            if (extra == null) continue;
            Object planObj = extra.get("plan");
            if (!(planObj instanceof Map<?, ?> plan)) continue;
            String planId = String.valueOf(plan.get("id"));
            int tier = plan.get("tier") instanceof Number n ? n.intValue() : 0;
            if (planId.equals(fromPlanId) || tier == fromPlanTier) {
                continue;
            }

            String price = option.priceFunction != null && request != null
                    ? option.priceFunction.resolve(request) : option.price;
            ResolvedPrice resolved;
            try {
                resolved = AssetRegistry.resolvePrice(price, option.network, option.asset);
            } catch (RuntimeException e) {
                // A mis-configured option is warn-and-skip; the remaining plans
                // still go out. Falling back to the raw config price would put an unsigned,
                // unparseable amount on the wire.
                LOG.log(System.Logger.Level.WARNING,
                        "failed to build change-plan payment requirements for plan "
                                + planId + ": " + e.getMessage());
                continue;
            }

            boolean upgrade = tier > fromPlanTier;
            // Registry-resolved extra (EIP-712 name/version/transferMethod) underneath, option
            // extra on top — same merge order as PaymentProcessor.buildRequirement.
            Map<String, Object> newExtra = new LinkedHashMap<>();
            if (resolved.extra() != null) {
                newExtra.putAll(resolved.extra());
            }
            newExtra.putAll(extra);
            if (!upgrade) {
                newExtra.remove("initialCharge");
            }
            Map<String, Object> changeFrom = new LinkedHashMap<>();
            changeFrom.put("fromSubId", fromSubId);
            changeFrom.put("fromPlanId", fromPlanId);
            changeFrom.put("fromPlanTier", fromPlanTier);
            changeFrom.put("direction", upgrade ? "upgrade" : "downgrade");
            changeFrom.put("effectiveAt", upgrade ? "immediate" : "period_end");
            newExtra.put("changeFrom", changeFrom);

            Map<String, Object> item = new LinkedHashMap<>();
            item.put("scheme", option.scheme);
            item.put("network", option.network);
            item.put("amount", resolved.amount());
            item.put("asset", option.asset != null ? option.asset : resolved.asset());
            item.put("payTo", option.payTo);
            item.put("maxTimeoutSeconds", option.maxTimeoutSeconds > 0
                    ? option.maxTimeoutSeconds : 86400);
            item.put("extra", newExtra);
            out.add(item);
        }
        return out;
    }

    // ------------------------------------------------------------------
    // Subscribe / change
    // ------------------------------------------------------------------

    private PaymentHooks.ProtectedRequestResult handleSubscribe(SubscriptionPayment payment,
                                                                 X402Response response,
                                                                 boolean syncSettle)
            throws IOException, InterruptedException {
        CreateResp resp = facilitator.subscribe(
                payment.chainIndex, payment.terms, payment.permit,
                payment.termsSignature, payment.permitSignature, syncSettle);

        StoredSubscription sub = storedFromTerms(resp.subId, resp.state, payment.terms);
        store.put(sub);

        int finalState = resp.state;
        if (resp.state == SubscriptionState.PENDING.getValue()) {
            Integer polled = refreshAfterPoll(sub);
            if (polled != null) finalState = polled;
        } else {
            // Non-pending settles (syncSettle immediate ACTIVE) still refresh once
            // so nextChargeableAt / billingAnchorAt / lastChargedPeriod are seeded — otherwise
            // the renewal scheduler never sees this subscription.
            rehydrateFromDetail(resp.subId);
        }

        writePaymentResponse(response, resp.subId, resp.txHash, finalState);
        return PaymentHooks.ProtectedRequestResult.grantAccess();
    }

    private PaymentHooks.ProtectedRequestResult handleChange(SubscriptionPayment payment,
                                                              X402Response response,
                                                              boolean syncSettle)
            throws IOException, InterruptedException {
        // The payload-level oldSubId is informational only; the server relies on the
        // buyer-SIGNED terms.changeFromSubId.
        String oldSubId = payment.terms.changeFromSubId;
        ChangeResp resp = facilitator.change(
                payment.chainIndex, oldSubId, payment.terms, payment.permit,
                payment.termsSignature, payment.permitSignature, syncSettle);

        StoredSubscription newSub = storedFromTerms(resp.newSubId, resp.state, payment.terms);
        store.put(newSub);

        // A downgrade legitimately stays pending until the period boundary — do not block-poll
        // on it. Only pending creates/upgrades poll.
        boolean isDowngrade = payment.terms.changeEffectiveAt == 2;
        int finalState = resp.state;
        if (resp.state == SubscriptionState.PENDING.getValue() && !isDowngrade) {
            Integer polled = refreshAfterPoll(newSub);
            if (polled != null) finalState = polled;
        } else {
            // Everything that doesn't block-poll (immediate ACTIVE settles and
            // legitimately-pending downgrades) still refreshes once to seed the scheduler
            // fields (nextChargeableAt / billingAnchorAt / lastChargedPeriod).
            rehydrateFromDetail(resp.newSubId);
        }

        // Never locally guess the old row into CHANGED at settle time — the
        // upgrade tx can still fail on-chain, and a mis-marked old row starves its renewal
        // scheduling. Refresh it from chain truth instead (state/changedToSubId land when the
        // chain confirms them).
        if (store.get(oldSubId) != null) {
            rehydrateFromDetail(oldSubId);
        }

        writePaymentResponse(response, resp.newSubId, resp.txHash, finalState);
        return PaymentHooks.ProtectedRequestResult.grantAccess();
    }

    private StoredSubscription storedFromTerms(String subId, int state, SubscriptionTerms terms) {
        StoredSubscription sub = new StoredSubscription();
        sub.subId = subId;
        sub.state = state;
        sub.payer = terms.payer;
        sub.merchant = terms.merchant;
        sub.token = terms.token;
        sub.amountPerPeriod = terms.amountPerPeriod;
        sub.periodSec = terms.periodSec;
        sub.periodMode = terms.periodMode;
        sub.maxPeriods = terms.maxPeriods;
        sub.startAt = terms.startAt;
        // The anchor and the charge watermark are UNKNOWN at settle time. Seeding
        // the anchor with terms.startAt is wrong for aligned upgrades (startAt = old period
        // boundary ≠ old anchor), and pre-filling lastChargedPeriod would let the access fast
        // path grant on a first charge that never confirmed on-chain. Both stay unset (0 /
        // null) so the anchorKnown / tri-state guards force one authoritative detail fetch,
        // which backfills the real values.
        sub.billingAnchorAt = 0;
        sub.lastChargedPeriod = null;
        sub.planTier = terms.planTier;
        sub.planId = terms.planId;
        sub.updatedAt = System.currentTimeMillis() / 1000;
        return sub;
    }

    /**
     * Poll detail (1s × 5, early-stop on charged-current-period OR FAILED) and fold the
     * authoritative values into the store. Returns the final state, or null if every poll failed.
     */
    private Integer refreshAfterPoll(StoredSubscription sub) throws InterruptedException {
        QueryResp latest = poller.poll(sub.subId);
        if (latest == null) {
            return null;
        }
        sub.applyDetail(latest);
        store.put(sub);
        return latest.state;
    }

    /**
     * Authoritative detail refresh. Starts from the EXISTING store row and merges — a detail
     * response that omits planId/planTier/anchor must never clobber the values seeded from
     * the buyer-signed terms, or a paid subscriber would start failing the plan gate.
     */
    private StoredSubscription rehydrateFromDetail(String subId) {
        try {
            QueryResp d = facilitator.getSubscription(subId);
            if (d == null || d.subId == null || d.subId.isEmpty()) {
                return null;
            }
            StoredSubscription sub = store.get(subId);
            if (sub == null) {
                sub = new StoredSubscription();
                sub.subId = d.subId;
            }
            sub.applyDetail(d);
            store.put(sub);
            return sub;
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return null;
        } catch (Exception e) {
            return null;
        }
    }

    // ------------------------------------------------------------------
    // Wire codecs
    // ------------------------------------------------------------------

    /** Canonical header first (PAYMENT-SIGNATURE), legacy X-APP-PAYMENT second. */
    private static String paymentHeader(X402Request request) {
        String v = request.getHeader(HEADER_PAYMENT_SIGNATURE);
        if (v == null || v.isEmpty()) {
            v = request.getHeader(HEADER_APP_PAYMENT);
        }
        return v;
    }

    /**
     * Peek the payment header's scheme/shape WITHOUT consuming it (mixed-route dispatch):
     * {@code accepted.scheme} decides when present;
     * otherwise a {@code terms} node marks the subscription shapes (wrapped or legacy flat).
     * No payment header at all delegates too — the subscription 402-offers path advertises
     * the route's full menu. Undecodable input also delegates so the handler produces its
     * structured 402 instead of the generic path guessing.
     */
    public static boolean looksLikeSubscriptionPayment(X402Request request) {
        String header = paymentHeader(request);
        if (header == null || header.isEmpty()) {
            header = request.getHeader("X-PAYMENT");
        }
        if (header == null || header.isEmpty()) {
            return true;
        }
        try {
            String json = new String(Base64.getDecoder().decode(header.trim()),
                    StandardCharsets.UTF_8);
            JsonNode root = Json.MAPPER.readTree(json);
            String scheme = root.path("accepted").path("scheme").asText(null);
            if (scheme != null && !scheme.isEmpty()) {
                return PaymentProcessor.isSubscriptionSchemeName(scheme);
            }
            JsonNode payload = root.has("payload") ? root.get("payload") : root;
            return payload.has("terms");
        } catch (Exception e) {
            return true;
        }
    }

    /**
     * Decode either payload shape into {@link SubscriptionPayment}:
     * x402 V2 wrapped {@code {x402Version, accepted, payload:{permitSingle,
     * permitSingleSignature, terms, termsSignature}}} (canonical; chainIndex derived from
     * accepted.network) or the legacy flat {@code {chainIndex, oldSubId, terms, permit,
     * termsSignature, permitSignature}}.
     */
    static SubscriptionPayment decodePayment(String headerB64) throws IOException {
        String json = new String(Base64.getDecoder().decode(headerB64), StandardCharsets.UTF_8);
        JsonNode root = Json.MAPPER.readTree(json);

        SubscriptionPayment out = new SubscriptionPayment();
        JsonNode payload = root.has("payload") ? root.get("payload") : root;

        out.terms = Json.MAPPER.treeToValue(payload.get("terms"), SubscriptionTerms.class);
        JsonNode permit = payload.has("permitSingle") ? payload.get("permitSingle")
                : payload.get("permit");
        out.permit = Json.MAPPER.treeToValue(permit, PermitSingle.class);
        out.termsSignature = firstText(payload, "termsSignature", "termsSig");
        out.permitSignature = firstText(payload, "permitSingleSignature", "permitSignature",
                "permitSig");

        out.acceptedScheme = root.path("accepted").path("scheme").asText(null);
        out.acceptedNetwork = root.path("accepted").path("network").asText(null);

        // accepted is a required field of the wrapped payload — a wrapped payload without it
        // is malformed, and tolerating it would hand chain selection back to the buyer's
        // explicit chainIndex. Only the legacy FLAT shape may omit accepted.
        if (root.has("payload")
                && (out.acceptedNetwork == null || out.acceptedNetwork.isEmpty())) {
            throw new IOException("wrapped payload missing accepted.network");
        }

        // Explicit chainIndex serves the legacy flat shape only; for wrapped payloads with a
        // route match, handle() overrides this with the route-derived value (buyer-proof).
        long chainIndex = root.path("chainIndex").asLong(0);
        if (chainIndex == 0) chainIndex = payload.path("chainIndex").asLong(0);
        if (chainIndex == 0) {
            chainIndex = chainIndexOf(root.path("accepted").path("network").asText(""));
        }
        out.chainIndex = chainIndex;

        String oldSubId = firstText(root, "oldSubId");
        if (oldSubId == null) oldSubId = firstText(payload, "oldSubId");
        out.oldSubId = oldSubId;

        // A payload missing terms/permitSingle/signatures must be rejected at decode (clean
        // 402); without this guard a null here NPEs out of the filter as a 500.
        if (out.terms == null || out.permit == null
                || out.termsSignature == null || out.termsSignature.isEmpty()
                || out.permitSignature == null || out.permitSignature.isEmpty()) {
            throw new IOException("missing terms/permitSingle/signatures in payload");
        }
        return out;
    }

    /** CAIP-2 {@code eip155:N} → chain index; 0 when absent or not parseable. */
    static long chainIndexOf(String network) {
        if (network != null && network.startsWith("eip155:")) {
            try {
                return Long.parseLong(network.substring("eip155:".length()));
            } catch (NumberFormatException ignored) {
                // not a numeric chain id
            }
        }
        return 0;
    }

    private static String firstText(JsonNode node, String... keys) {
        for (String k : keys) {
            JsonNode v = node.get(k);
            if (v != null && !v.isNull()) return v.asText();
        }
        return null;
    }

    private void writePaymentResponse(X402Response response, String subId, String txHash,
                                      int state) throws IOException {
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("subId", subId);
        body.put("txHash", txHash);
        body.put("state", state);
        String b64 = Base64.getEncoder().encodeToString(
                Json.MAPPER.writeValueAsString(body).getBytes(StandardCharsets.UTF_8));
        response.setHeader(HEADER_PAYMENT_RESPONSE, b64);
        response.setHeader(HEADER_APP_PAYMENT_RESPONSE, b64);
        response.setHeader("Access-Control-Expose-Headers",
                HEADER_PAYMENT_RESPONSE + "," + HEADER_APP_PAYMENT_RESPONSE);
    }

    private void writeOffers(X402Response response, X402Request request,
                             List<Map<String, Object>> accepts, String error) throws IOException {
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("x402Version", 2);
        if (error != null) body.put("error", error);
        Map<String, Object> resource = new LinkedHashMap<>();
        resource.put("url", request.getRequestURL());
        resource.put("mimeType", "application/json");
        body.put("resource", resource);
        body.put("accepts", accepts);
        String json = Json.MAPPER.writeValueAsString(body);
        String b64 = Base64.getEncoder().encodeToString(json.getBytes(StandardCharsets.UTF_8));
        response.setStatus(X402Response.SC_PAYMENT_REQUIRED);
        response.setContentType("application/json; charset=UTF-8");
        response.setHeader("PAYMENT-REQUIRED", b64);
        response.setHeader(HEADER_APP_PAYMENT_REQUIRED, b64);
        response.setHeader("Access-Control-Expose-Headers",
                "PAYMENT-REQUIRED," + HEADER_APP_PAYMENT_REQUIRED);
        response.writeBody(json);
    }

    private void writeJson(X402Response response, int status, String json) throws IOException {
        response.setStatus(status);
        response.setContentType("application/json; charset=UTF-8");
        response.writeBody(json);
    }

    private void writeError(X402Response response, int status, String reason) throws IOException {
        writeJson(response, status,
                "{\"x402Version\":2,\"error\":" + Json.MAPPER.writeValueAsString(reason) + "}");
    }

    // ------------------------------------------------------------------
    // Verification
    // ------------------------------------------------------------------

    /**
     * Bind the buyer-signed terms to an advertised plan on the route: planId must be offered;
     * merchant / per-period economics /
     * token must match verbatim. initialCharge is bound on CREATE only (a change's upfront
     * charge is contract-governed). startAt is intentionally NOT compared.
     * Returns null when bound, else a machine-readable error.
     */
    static String verifyTermsMatchOffer(SubscriptionTerms terms, List<AcceptOption> accepts) {
        AcceptOption match = null;
        for (AcceptOption option : accepts) {
            if (!PaymentProcessor.isSubscriptionSchemeName(option.scheme)) continue;
            Map<String, Object> extra = option.extra;
            if (extra == null) continue;
            Object plan = extra.get("plan");
            if (plan instanceof Map<?, ?> p && String.valueOf(p.get("id")).equals(terms.planId)) {
                match = option;
                break;
            }
        }
        if (match == null) {
            return SubscriptionErrorCodes.PLAN_MISMATCH;
        }
        Map<String, Object> extra = match.extra;

        if (match.payTo == null || !match.payTo.equalsIgnoreCase(terms.merchant)) {
            return "terms_merchant_mismatch";
        }
        if (!Objects.equals(str(extra, "amountPerPeriod"), terms.amountPerPeriod)) {
            return "terms_amount_per_period_mismatch";
        }
        if (num(extra, "periodSec") != terms.periodSec) {
            return "terms_period_sec_mismatch";
        }
        if (num(extra, "maxPeriods") != terms.maxPeriods) {
            return "terms_max_periods_mismatch";
        }
        // Absent periodMode in the offer extra means fixed_seconds (0) —
        // legacy offers written before the 17th field must keep matching periodMode=0 terms.
        long offerPeriodMode = extra.containsKey("periodMode") ? num(extra, "periodMode") : 0L;
        if (offerPeriodMode != terms.periodMode) {
            return "terms_period_mode_mismatch";
        }
        Object plan = extra.get("plan");
        long tier = plan instanceof Map<?, ?> p && p.get("tier") instanceof Number n
                ? n.longValue() : -1;
        if (tier != terms.planTier) {
            return "terms_plan_tier_mismatch";
        }
        // Token binds to the advertised asset (worthless-token guard); empty = skip.
        if (match.asset != null && !match.asset.isEmpty()
                && !match.asset.equalsIgnoreCase(terms.token)) {
            return "terms_token_mismatch";
        }
        // initialCharge: create only.
        if (ZERO_BYTES32.equalsIgnoreCase(terms.changeFromSubId)) {
            long offerPeriods = 0;
            String offerAmount = "0";
            Object ic = extra.get("initialCharge");
            if (ic instanceof Map<?, ?> icm) {
                if (icm.get("periodCount") instanceof Number n) offerPeriods = n.longValue();
                Object total = icm.get("totalAmount");
                if (total != null) offerAmount = String.valueOf(total);
            }
            if (terms.initialChargePeriods != offerPeriods) {
                return "terms_initial_charge_periods_mismatch";
            }
            if (!offerAmount.equals(terms.initialChargeAmount)) {
                return "terms_initial_charge_amount_mismatch";
            }
        }
        return null;
    }

    private static String str(Map<String, Object> extra, String key) {
        Object v = extra.get(key);
        return v == null ? null : String.valueOf(v);
    }

    private static long num(Map<String, Object> extra, String key) {
        Object v = extra.get(key);
        return v instanceof Number n ? n.longValue() : -1;
    }

    String runPreChecks(SubscriptionTerms terms, PermitSingle permit) {
        if (terms.payer == null || terms.merchant == null || terms.facilitator == null
                || terms.token == null || terms.amountPerPeriod == null
                || terms.initialChargeAmount == null || terms.permitHash == null
                || terms.salt == null || terms.changeFromSubId == null) {
            return SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS;
        }

        // Amounts are uint decimal strings on the wire; a non-numeric or negative value must be
        // a structured reject, not an uncaught NumberFormatException (container 500). Negative
        // numeric fields are equally malformed uints — and rejecting them also kills the
        // "-1 sentinel" collision that let sparse offers skip parts of the binding gate.
        if (!isNonNegativeDecimal(terms.amountPerPeriod)
                || !isNonNegativeDecimal(terms.initialChargeAmount)) {
            return SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS;
        }
        // uint256 upper bound: a digits-only value ≥ 2^256 would still blow the 32-byte pad
        // inside the EIP-712 encoders (container 500) — same class as the sigDeadline guard.
        if (new BigInteger(terms.amountPerPeriod).bitLength() > 256
                || new BigInteger(terms.initialChargeAmount).bitLength() > 256) {
            return SubscriptionErrorCodes.AMOUNT_PER_PERIOD_INVALID;
        }
        if (terms.periodSec < 0 || terms.maxPeriods < 0 || terms.startAt < 0
                || terms.initialChargePeriods < 0 || terms.planTier < 0
                || terms.periodMode < 0 || terms.changeEffectiveAt < 0
                || terms.termsDeadline < 0) {
            return SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS;
        }

        if (!isValidAddress(terms.payer) || !isValidAddress(terms.merchant)
                || !isValidAddress(terms.facilitator) || !isValidAddress(terms.token)) {
            return SubscriptionErrorCodes.INVALID_ADDRESS_FORMAT;
        }

        if (!isValidBytes32(terms.permitHash) || !isValidBytes32(terms.salt)
                || !isValidBytes32(terms.changeFromSubId)) {
            return SubscriptionErrorCodes.INVALID_BYTES32;
        }

        if (permit.details == null || permit.sigDeadline == null) {
            return SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS;
        }
        // details.token feeds padAddress inside hashPermitSingle — a null or non-hex value
        // must be a structured reject here, not an NPE/NFE out of the encoder (500).
        if (!isValidAddress(permit.details.token)) {
            return SubscriptionErrorCodes.INVALID_ADDRESS_FORMAT;
        }
        // permit.details.amount feeds the EIP-712 encoder before any facilitator call — a
        // non-decimal or negative value must reject here, not throw out of hashPermitSingle.
        // Permit2 amount is uint160: bound it so the 32-byte pad can never throw.
        if (!isNonNegativeDecimal(permit.details.amount)
                || new BigInteger(permit.details.amount).bitLength() > 160
                || permit.details.expiration < 0 || permit.details.nonce < 0) {
            return SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS;
        }
        BigInteger sigDeadline;
        try {
            sigDeadline = new BigInteger(permit.sigDeadline);
        } catch (NumberFormatException e) {
            return SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS;
        }
        // uint256 range: a value ≥ 2^256 (or negative) would make the 32-byte pad throw an
        // uncaught RuntimeException (container 500) inside hashPermitSingle below.
        if (sigDeadline.signum() < 0 || sigDeadline.bitLength() > 256) {
            return SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS;
        }

        if (permit.details.token != null && terms.token != null
                && !permit.details.token.equalsIgnoreCase(terms.token)) {
            return SubscriptionErrorCodes.TOKEN_MISMATCH;
        }

        if (permit.spender == null || !permit.spender.equalsIgnoreCase(subscriptionContract)) {
            return SubscriptionErrorCodes.PERMIT_SPENDER_MISMATCH;
        }

        byte[] computedPermitHash = SubscriptionEip712.hashPermitSingle(permit);
        byte[] declaredPermitHash = Numeric.hexStringToByteArray(
                terms.permitHash.startsWith("0x") ? terms.permitHash.substring(2) : terms.permitHash);
        if (!Arrays.equals(computedPermitHash, declaredPermitHash)) {
            return SubscriptionErrorCodes.PERMIT_HASH_MISMATCH;
        }

        long now = System.currentTimeMillis() / 1000;
        if (terms.termsDeadline < now) {
            return SubscriptionErrorCodes.TERMS_EXPIRED;
        }
        // uint256 decimal string; == now is still valid (contract reverts only on strict >).
        if (sigDeadline.compareTo(BigInteger.valueOf(now)) < 0) {
            return SubscriptionErrorCodes.SIG_EXPIRED;
        }

        if (terms.initialChargePeriods > terms.maxPeriods) {
            return SubscriptionErrorCodes.INITIAL_CHARGE_PERIODS_EXCEED_MAX;
        }

        BigInteger initialAmount = new BigInteger(terms.initialChargeAmount);
        BigInteger cap = new BigInteger(terms.amountPerPeriod)
                .multiply(BigInteger.valueOf(terms.initialChargePeriods));
        if (initialAmount.compareTo(cap) > 0) {
            return SubscriptionErrorCodes.INITIAL_CHARGE_AMOUNT_EXCEEDS_CAP;
        }

        if (terms.changeEffectiveAt == 0) {
            if (!ZERO_BYTES32.equalsIgnoreCase(terms.changeFromSubId)) {
                return SubscriptionErrorCodes.CREATE_MUST_HAVE_ZERO_CHANGE_FROM;
            }
        } else {
            if (ZERO_BYTES32.equalsIgnoreCase(terms.changeFromSubId)) {
                return SubscriptionErrorCodes.CHANGE_MUST_HAVE_NONZERO_CHANGE_FROM;
            }
        }

        // periodMode (17th field): calendar-month terms MUST carry periodSec == 0.
        if (terms.periodMode != PeriodMode.FIXED_SECONDS.getValue()
                && terms.periodMode != PeriodMode.CALENDAR_MONTH.getValue()) {
            return SubscriptionErrorCodes.PERIOD_MODE_INVALID;
        }
        if (PeriodMode.isCalendarMonth(terms.periodMode)) {
            if (terms.periodSec != 0) {
                return SubscriptionErrorCodes.PERIOD_SEC_NOT_ALLOWED;
            }
        } else if (terms.periodSec <= 0) {
            return SubscriptionErrorCodes.PERIOD_SEC_INVALID;
        }

        return null;
    }

    private static boolean isCreate(SubscriptionTerms terms) {
        return terms.changeEffectiveAt == 0
                && ZERO_BYTES32.equalsIgnoreCase(terms.changeFromSubId);
    }

    private static boolean isChange(SubscriptionTerms terms) {
        return !isCreate(terms);
    }

    private static boolean isBlank(String s) {
        return s == null || s.trim().isEmpty();
    }

    /** Wire uints are plain decimal strings: non-empty, digits only (no sign, no 0x, no dot). */
    private static boolean isNonNegativeDecimal(String s) {
        if (s == null || s.isEmpty()) {
            return false;
        }
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c < '0' || c > '9') {
                return false;
            }
        }
        return true;
    }

    private static boolean isValidAddress(String addr) {
        if (addr == null || !addr.startsWith("0x")) return false;
        String hex = addr.substring(2);
        return hex.length() == 40 && hex.chars().allMatch(c ->
                (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'));
    }

    private static boolean isValidBytes32(String val) {
        if (val == null || !val.startsWith("0x")) return false;
        String hex = val.substring(2);
        return hex.length() == 64 && hex.chars().allMatch(c ->
                (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'));
    }

    public static class SubscriptionPayment {
        /** Chain index; derived from accepted.network (eip155:X) for wrapped payloads. */
        public long chainIndex;
        /** Buyer's accepted echo (wrapped payloads only); bound against the route's accepts. */
        public String acceptedScheme;
        public String acceptedNetwork;
        /** Old subId for change requests (informational; the buyer-signed terms.changeFromSubId wins). */
        public String oldSubId;
        public SubscriptionTerms terms;
        public PermitSingle permit;
        public String termsSignature;
        public String permitSignature;
    }
}
