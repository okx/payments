// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.model.v2.SupportedKind;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.subscription.facilitator.SubscriptionFacilitatorClient;
import com.okx.x402.subscription.server.SubscriptionSchemeHandler;
import com.okx.x402.subscription.server.access.AccessProofHook;
import com.okx.x402.subscription.server.store.SubscriptionStore;
import org.junit.jupiter.api.Test;
import org.mockito.ArgumentCaptor;

import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicBoolean;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.ArgumentMatchers.*;
import static org.mockito.Mockito.*;

/**
 * Subscription operation-route dispatch ordering and /supported offer enhancement:
 *
 * <ul>
 *   <li>CHANGE offers are served AFTER the merchant {@code onProtectedRequest} hooks, so
 *       bans / allowlists apply to change-offer requests (Rust middleware step 2 → 3a
 *       order); cancel routes stay BEFORE the hooks (Rust step 1b).</li>
 *   <li>{@link AccessProofHook} yields to the scheme handler on operation routes instead of
 *       swallowing the CHANGE request with a resource-level grant.</li>
 *   <li>Offer enhancement operates on a per-request copy — the seller's shared
 *       {@code RouteConfig} is never mutated (concurrent-first-request CME guard).</li>
 * </ul>
 */
class PaymentProcessorSubscriptionDispatchTest {

    private static final String SUB_CONTRACT = "0x4020000000000000000000000000000000000003";
    private static final String PERMIT2 = "0x000000000022d473030f116ddee9f6b43ac78ba3";

    // ------------------------------------------------------------------
    // R5-A: dispatch ordering
    // ------------------------------------------------------------------

    @Test
    void changeRouteRunsMerchantHooksBeforeServingOffers() throws Exception {
        SubscriptionSchemeHandler handler = mock(SubscriptionSchemeHandler.class);
        PaymentProcessor processor = changeRouteProcessor(handler, sellerChangeConfig());
        processor.onProtectedRequest((req, cfg) ->
                PaymentHooks.ProtectedRequestResult.abort("banned",
                        PaymentHooks.ProtectedRequestResult.AbortClass.DENIED));

        X402Response response = mock(X402Response.class);
        assertNull(processor.preHandle(request("GET", "/changePlan"), response));

        // The ban must answer before any change offers are built: a vetoed buyer is never
        // shown the change-plan menu.
        verify(handler, never()).handleChangeOffers(any(), any(), any());
        // DENIED → bare 402, no PAYMENT-REQUIRED envelope.
        verify(response, never()).setHeader(eq("PAYMENT-REQUIRED"), anyString());
        verify(response).setStatus(X402Response.SC_PAYMENT_REQUIRED);
    }

    @Test
    void changeRouteServesOffersWhenHooksProceed() throws Exception {
        SubscriptionSchemeHandler handler = mock(SubscriptionSchemeHandler.class);
        when(handler.handleChangeOffers(any(), any(), any())).thenReturn(true);
        PaymentProcessor processor = changeRouteProcessor(handler, sellerChangeConfig());
        AtomicBoolean hookRan = new AtomicBoolean();
        processor.onProtectedRequest((req, cfg) -> {
            hookRan.set(true);
            return PaymentHooks.ProtectedRequestResult.proceed();
        });

        X402Response response = mock(X402Response.class);
        assertNull(processor.preHandle(request("GET", "/changePlan"), response));

        assertTrue(hookRan.get(), "merchant hook must run before the change-offer phase");
        verify(handler).handleChangeOffers(any(), any(), any());
    }

    @Test
    void cancelRouteDispatchesBeforeMerchantHooks() throws Exception {
        SubscriptionSchemeHandler handler = mock(SubscriptionSchemeHandler.class);
        PaymentProcessor.RouteConfig config = sellerChangeConfig();
        config.subscriptionOperation = PaymentProcessor.SubscriptionOperation.CANCEL;
        PaymentProcessor processor = changeRouteProcessor(handler, config);
        AtomicBoolean hookRan = new AtomicBoolean();
        processor.onProtectedRequest((req, cfg) -> {
            hookRan.set(true);
            return PaymentHooks.ProtectedRequestResult.proceed();
        });

        X402Response response = mock(X402Response.class);
        assertNull(processor.preHandle(request("GET", "/changePlan"), response));

        // Cancel relays a buyer-SIGNED auth — no payment flow, no merchant-policy gate
        // (Rust middleware step 1b runs before the hook step 2).
        verify(handler).handleCancelOperation(any(), any(),
                eq(PaymentProcessor.SubscriptionOperation.CANCEL));
        assertFalse(hookRan.get(), "cancel operation routes dispatch before the hooks");
    }

    @Test
    void accessProofHookYieldsOnOperationRoutes() {
        SubscriptionStore store = mock(SubscriptionStore.class);
        SubscriptionFacilitatorClient facilitator = mock(SubscriptionFacilitatorClient.class);
        AccessProofHook hook = new AccessProofHook(store, facilitator);

        PaymentProcessor.RouteConfig config = sellerChangeConfig();
        X402Request request = request("GET", "/changePlan");
        when(request.getHeader("APP-Access")).thenReturn("b64-proof");

        PaymentHooks.ProtectedRequestResult result = hook.onProtectedRequest(request, config);

        // Operation routes own their access-proof handling in the scheme handler; a grant
        // here would serve the underlying resource instead of the change-offer 402.
        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.PROCEED, result.decision);
        verifyNoInteractions(store, facilitator);
    }

    // ------------------------------------------------------------------
    // R5-C: enhancement never mutates the seller's shared RouteConfig
    // ------------------------------------------------------------------

    @Test
    void subscription402DoesNotMutateSellerConfig() throws Exception {
        FacilitatorClient facilitator = supportedFacilitator();
        PaymentProcessor.RouteConfig config = sellerChangeConfig();
        config.subscriptionOperation = null; // ordinary subscription resource route
        Map<String, PaymentProcessor.RouteConfig> routes = new HashMap<>();
        routes.put("GET /sub", config);
        PaymentProcessor processor = new PaymentProcessor(facilitator, routes);

        X402Response response = mock(X402Response.class);
        processor.respond402Subscription(response, request("GET", "/sub"), config, null);

        // The served envelope carries the /supported three-some…
        ArgumentCaptor<String> body = ArgumentCaptor.forClass(String.class);
        verify(response).writeBody(body.capture());
        assertTrue(body.getValue().contains("\"contracts\""), "envelope must carry contracts");
        assertTrue(body.getValue().contains("A2APaySubscription"), "envelope must carry domain");
        // …while the seller's shared config stays pristine: enhancement happened on a
        // per-request copy, so concurrent first requests cannot tear these maps.
        Map<String, Object> sellerExtra = config.accepts.get(0).extra;
        assertFalse(sellerExtra.containsKey("contracts"), "seller extra must not be mutated");
        assertFalse(sellerExtra.containsKey("domain"), "seller extra must not be mutated");
        assertFalse(sellerExtra.containsKey("facilitator"), "seller extra must not be mutated");
    }

    @Test
    void changeOffersReceiveEnhancedCopy() throws Exception {
        SubscriptionSchemeHandler handler = mock(SubscriptionSchemeHandler.class);
        when(handler.handleChangeOffers(any(), any(), any())).thenReturn(true);
        PaymentProcessor.RouteConfig config = sellerChangeConfig();
        Map<String, PaymentProcessor.RouteConfig> routes = new HashMap<>();
        routes.put("GET /changePlan", config);
        PaymentProcessor processor = new PaymentProcessor(supportedFacilitator(), routes);
        processor.subscriptionHandler(handler);

        X402Response response = mock(X402Response.class);
        assertNull(processor.preHandle(request("GET", "/changePlan"), response));

        ArgumentCaptor<PaymentProcessor.RouteConfig> served =
                ArgumentCaptor.forClass(PaymentProcessor.RouteConfig.class);
        verify(handler).handleChangeOffers(any(), any(), served.capture());
        // The handler sees the enhanced view (buyers need contracts/domain to sign)…
        assertNotSame(config, served.getValue());
        Map<String, Object> servedExtra = served.getValue().accepts.get(0).extra;
        assertTrue(servedExtra.containsKey("contracts"));
        assertTrue(servedExtra.containsKey("domain"));
        // …and the seller's config is untouched.
        assertFalse(config.accepts.get(0).extra.containsKey("contracts"));
    }

    // ------------------------------------------------------------------
    // fixtures
    // ------------------------------------------------------------------

    /** A CHANGE route whose single period plan relies on /supported for contracts/domain. */
    private static PaymentProcessor.RouteConfig sellerChangeConfig() {
        Map<String, Object> extra = new LinkedHashMap<>();
        extra.put("amountPerPeriod", "5000000");
        extra.put("periodSec", 2592000L);
        extra.put("maxPeriods", 12);
        extra.put("plan", Map.of("id", "pro_monthly", "tier", 2));
        AcceptOption option = AcceptOption.builder()
                .scheme(PaymentProcessor.SCHEME_PERIOD)
                .network("eip155:196")
                .payTo("0x2222222222222222222222222222222222222222")
                .price("$0.01")
                .extra(extra)
                .build();
        PaymentProcessor.RouteConfig config = new PaymentProcessor.RouteConfig();
        config.accepts = new java.util.ArrayList<>(List.of(option));
        config.subscriptionOperation = PaymentProcessor.SubscriptionOperation.CHANGE;
        return config;
    }

    private static PaymentProcessor changeRouteProcessor(SubscriptionSchemeHandler handler,
                                                         PaymentProcessor.RouteConfig config) {
        Map<String, PaymentProcessor.RouteConfig> routes = new HashMap<>();
        routes.put("GET /changePlan", config);
        PaymentProcessor processor =
                new PaymentProcessor(mock(FacilitatorClient.class), routes);
        processor.subscriptionHandler(handler);
        return processor;
    }

    /** Facilitator whose /supported advertises the subscription three-some for eip155:196. */
    private static FacilitatorClient supportedFacilitator() throws Exception {
        SupportedKind kind = new SupportedKind();
        kind.scheme = PaymentProcessor.SCHEME_PERIOD;
        kind.network = "eip155:196";
        kind.extra = Map.of(
                "facilitatorAddress", "0x8c71e6826f754300fea650f68625bf3f15e0a7fa",
                "subscriptionContract", SUB_CONTRACT,
                "permit2Contract", PERMIT2);
        SupportedResponse supported = new SupportedResponse();
        supported.kinds = List.of(kind);
        FacilitatorClient facilitator = mock(FacilitatorClient.class);
        when(facilitator.supported()).thenReturn(supported);
        return facilitator;
    }

    private static X402Request request(String method, String uri) {
        X402Request request = mock(X402Request.class);
        when(request.getMethod()).thenReturn(method);
        when(request.getRequestURI()).thenReturn(uri);
        when(request.getRequestURL()).thenReturn("https://seller.example" + uri);
        return request;
    }
}
