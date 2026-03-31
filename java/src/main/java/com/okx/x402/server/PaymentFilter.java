package com.okx.x402.server;

import com.okx.x402.config.AssetRegistry;
import com.okx.x402.config.ResolvedPrice;
import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.model.SettlementResponseHeader;
import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequired;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.ResourceInfo;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.VerifyResponse;
import com.okx.x402.util.Json;

import jakarta.servlet.Filter;
import jakarta.servlet.FilterChain;
import jakarta.servlet.ServletException;
import jakarta.servlet.ServletRequest;
import jakarta.servlet.ServletResponse;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;

import java.io.IOException;
import java.util.Base64;
import java.util.List;
import java.util.Map;
import java.util.Objects;

/**
 * Servlet filter that enforces x402 V2 payments on configured routes.
 * Supports route-level pricing, multi-chain, and dynamic price resolution.
 */
public class PaymentFilter implements Filter {

    private static final String HEADER_PAYMENT_SIGNATURE = "PAYMENT-SIGNATURE";
    private static final String HEADER_PAYMENT_RESPONSE = "PAYMENT-RESPONSE";
    private static final String HEADER_X_PAYMENT = "X-PAYMENT";

    private final FacilitatorClient facilitator;
    private final Map<String, RouteConfig> routes;

    /**
     * Per-route payment configuration.
     */
    public static class RouteConfig {
        /** Payment scheme (default "exact"). */
        public String scheme = "exact";
        /** CAIP-2 network identifier (e.g. "eip155:196"). */
        public String network;
        /** Receiver wallet address. */
        public String payTo;
        /** Price as USD string (e.g. "$0.01") or atomic units. */
        public String price;
        /** Optional asset override (contract address). */
        public String asset;
        /** Maximum timeout in seconds (default 24h per spec). */
        public int maxTimeoutSeconds = 86400;
    }

    /**
     * Creates a PaymentFilter with the given facilitator and route configuration.
     *
     * @param facilitator V2 facilitator client
     * @param routes map of "METHOD /path" to route config
     */
    public PaymentFilter(FacilitatorClient facilitator, Map<String, RouteConfig> routes) {
        this.facilitator = Objects.requireNonNull(facilitator);
        this.routes = Objects.requireNonNull(routes);
    }

    /**
     * Convenience factory for one-line integration.
     *
     * @param facilitator V2 facilitator client
     * @param routes map of route keys to configs
     * @return configured PaymentFilter
     */
    public static PaymentFilter create(FacilitatorClient facilitator,
                                       Map<String, RouteConfig> routes) {
        return new PaymentFilter(facilitator, routes);
    }

    @Override
    public void doFilter(ServletRequest req, ServletResponse res, FilterChain chain)
            throws IOException, ServletException {

        if (!(req instanceof HttpServletRequest request)
                || !(res instanceof HttpServletResponse response)) {
            chain.doFilter(req, res);
            return;
        }

        String routeKey = request.getMethod() + " " + request.getRequestURI();
        RouteConfig config = routes.get(routeKey);
        if (config == null) {
            config = routes.get(request.getRequestURI());
        }
        if (config == null) {
            chain.doFilter(req, res);
            return;
        }

        // Read PAYMENT-SIGNATURE (V2) or X-PAYMENT (V1 fallback)
        String header = request.getHeader(HEADER_PAYMENT_SIGNATURE);
        if (header == null || header.isEmpty()) {
            header = request.getHeader(HEADER_X_PAYMENT);
        }
        if (header == null || header.isEmpty()) {
            respond402(response, request, config, null);
            return;
        }

        // Decode payment payload
        PaymentPayload payload;
        try {
            payload = PaymentPayload.fromHeader(header);
        } catch (Exception e) {
            respond402(response, request, config, "malformed payment header");
            return;
        }

        // Validate resource URL matches (prevent cross-route replay)
        if (payload.resource != null && payload.resource.url != null) {
            String requestUrl = request.getRequestURL().toString();
            if (!payload.resource.url.equals(requestUrl)) {
                respond402(response, request, config, "resource mismatch");
                return;
            }
        }

        PaymentRequirements requirements = buildRequirements(request, config);
        VerifyResponse vr;
        try {
            vr = facilitator.verify(payload, requirements);
        } catch (IOException e) {
            response.setStatus(HttpServletResponse.SC_INTERNAL_SERVER_ERROR);
            response.setContentType("application/json");
            response.getWriter().write(
                    "{\"error\":\"Payment verification failed: " + e.getMessage() + "\"}");
            return;
        } catch (Exception e) {
            response.setStatus(HttpServletResponse.SC_INTERNAL_SERVER_ERROR);
            response.setContentType("application/json");
            response.getWriter().write(
                    "{\"error\":\"Internal server error during payment verification\"}");
            return;
        }

        if (!vr.isValid) {
            respond402(response, request, config, vr.invalidReason);
            return;
        }

        // Execute business logic
        chain.doFilter(req, res);

        if (response.getStatus() >= 400) {
            return;
        }

        // Settle
        try {
            SettleResponse sr = facilitator.settle(payload, requirements);
            if (!sr.success) {
                if (!response.isCommitted()) {
                    respond402(response, request, config,
                            sr.errorReason != null ? sr.errorReason : "settlement failed");
                }
                return;
            }
            SettlementResponseHeader srh = new SettlementResponseHeader(
                    true, sr.transaction, sr.network, sr.payer);
            String b64 = Base64.getEncoder().encodeToString(
                    Json.MAPPER.writeValueAsBytes(srh));
            response.setHeader(HEADER_PAYMENT_RESPONSE, b64);
            response.setHeader("Access-Control-Expose-Headers", HEADER_PAYMENT_RESPONSE);
        } catch (Exception e) {
            if (!response.isCommitted()) {
                respond402(response, request, config, "settlement error: " + e.getMessage());
            }
        }
    }

    private PaymentRequirements buildRequirements(HttpServletRequest request,
                                                  RouteConfig config) {
        ResolvedPrice resolved = AssetRegistry.resolvePrice(config.price, config.network);

        PaymentRequirements pr = new PaymentRequirements();
        pr.scheme = config.scheme;
        pr.network = config.network;
        pr.amount = resolved.amount();
        pr.payTo = config.payTo;
        pr.maxTimeoutSeconds = config.maxTimeoutSeconds;
        pr.asset = config.asset != null ? config.asset : resolved.asset();
        pr.extra = resolved.extra();
        return pr;
    }

    private void respond402(HttpServletResponse resp, HttpServletRequest req,
                            RouteConfig config, String error) throws IOException {
        PaymentRequired pr = new PaymentRequired();
        pr.x402Version = 2;
        pr.error = error;
        pr.resource = new ResourceInfo();
        pr.resource.url = req.getRequestURL().toString();
        pr.resource.mimeType = "application/json";
        pr.accepts = List.of(buildRequirements(req, config));

        resp.setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        resp.setContentType("application/json");
        resp.getWriter().write(Json.MAPPER.writeValueAsString(pr));
    }
}
