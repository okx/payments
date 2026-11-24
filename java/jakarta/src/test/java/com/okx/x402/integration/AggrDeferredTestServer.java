// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.integration;

import com.okx.x402.facilitator.OKXFacilitatorClient;
import com.okx.x402.server.PaymentFilter;
import com.okx.x402.server.PaymentProcessor;

import jakarta.servlet.http.HttpServlet;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;

import org.eclipse.jetty.server.Server;
import org.eclipse.jetty.server.ServerConnector;
import org.eclipse.jetty.servlet.FilterHolder;
import org.eclipse.jetty.servlet.ServletContextHandler;
import org.eclipse.jetty.servlet.ServletHolder;

import java.io.IOException;
import java.util.EnumSet;
import java.util.Map;

/**
 * Standalone test seller server for aggr_deferred integration testing.
 *
 * <p>Accepts both {@code "exact"} and {@code "aggr_deferred"} schemes on X
 * Layer. Talks to a real OKX facilitator endpoint (defaults to the public
 * {@code https://www.okx.com}; override via {@code OKX_FACILITATOR_BASE_URL}).
 *
 * <p>All credentials and the seller wallet are read from environment
 * variables — the test server will refuse to start if any are missing:
 *
 * <ul>
 *   <li>{@code OKX_API_KEY}    — OKX API key</li>
 *   <li>{@code OKX_SECRET_KEY} — OKX secret key</li>
 *   <li>{@code OKX_PASSPHRASE} — OKX API passphrase</li>
 *   <li>{@code OKX_PAY_TO}     — seller wallet address on X Layer</li>
 *   <li>{@code OKX_FACILITATOR_BASE_URL} — optional facilitator endpoint
 *       (default {@code https://www.okx.com})</li>
 *   <li>{@code OKX_TEST_PORT}  — optional listen port (default {@code 4402})</li>
 * </ul>
 *
 * <p>Start: {@code mvn exec:java
 * -Dexec.mainClass="com.okx.x402.integration.AggrDeferredTestServer"
 * -Dexec.classpathScope=test}; or run {@code main()} from your IDE.
 */
public class AggrDeferredTestServer {

    private static final String DEFAULT_BASE_URL = "https://web3.okx.com";
    private static final int DEFAULT_PORT = 4402;

    public static void main(String[] args) throws Exception {
        String apiKey = requireEnv("OKX_API_KEY");
        String secretKey = requireEnv("OKX_SECRET_KEY");
        String passphrase = requireEnv("OKX_PASSPHRASE");
        String payTo = requireEnv("OKX_PAY_TO");
        String baseUrl = envOrDefault("OKX_FACILITATOR_BASE_URL", DEFAULT_BASE_URL);
        int port = parsePort(envOrDefault("OKX_TEST_PORT",
                Integer.toString(DEFAULT_PORT)));

        // 1. Create facilitator client (HMAC-SHA256 auth is automatic)
        OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
                apiKey, secretKey, passphrase, baseUrl);

        // 2. Configure routes — accept BOTH exact and aggr_deferred
        //    The PaymentFilter uses scheme from the incoming payment header,
        //    so we create two route entries for the same path.
        PaymentProcessor.RouteConfig exactRoute = new PaymentProcessor.RouteConfig();
        exactRoute.scheme = "exact";
        exactRoute.network = "eip155:196";
        exactRoute.payTo = payTo;
        exactRoute.price = "$0.000001"; // 1 atomic unit = minimum
        exactRoute.maxTimeoutSeconds = 86400;

        PaymentProcessor.RouteConfig deferredRoute = new PaymentProcessor.RouteConfig();
        deferredRoute.scheme = "aggr_deferred";
        deferredRoute.network = "eip155:196";
        deferredRoute.payTo = payTo;
        deferredRoute.price = "$0.000001";
        deferredRoute.maxTimeoutSeconds = 86400;

        // Route key: "GET /path" — PaymentFilter matches on this
        // For now, use exact route as default (filter reads scheme from payment header)
        PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
                "/api/weather", exactRoute,
                "GET /api/weather", exactRoute,
                "/api/deferred", deferredRoute,
                "GET /api/deferred", deferredRoute
        ));

        // 3. Start embedded Jetty
        Server jetty = new Server(port);
        ServletContextHandler ctx = new ServletContextHandler(ServletContextHandler.SESSIONS);
        ctx.setContextPath("/");

        ctx.addFilter(new FilterHolder(filter), "/*",
                EnumSet.of(jakarta.servlet.DispatcherType.REQUEST));

        ctx.addServlet(new ServletHolder(new WeatherServlet()), "/api/weather");
        ctx.addServlet(new ServletHolder(new DeferredServlet()), "/api/deferred");
        ctx.addServlet(new ServletHolder(new HealthServlet()), "/health");

        jetty.setHandler(ctx);
        jetty.start();

        int actualPort = ((ServerConnector) jetty.getConnectors()[0]).getLocalPort();
        System.out.println("===========================================");
        System.out.println("  x402 Test Seller Server STARTED");
        System.out.println("===========================================");
        System.out.println("  Port:        " + actualPort);
        System.out.println("  Facilitator: " + baseUrl);
        System.out.println("  PayTo:       " + payTo);
        System.out.println("  Network:     eip155:196 (X Layer)");
        System.out.println("  Asset:       USDT (0x779d...3736)");
        System.out.println("  Price:       $0.000001 (min amount)");
        System.out.println("-------------------------------------------");
        System.out.println("  Endpoints:");
        System.out.println("    GET /health          — free (health check)");
        System.out.println("    GET /api/weather     — paid (exact scheme)");
        System.out.println("    GET /api/deferred    — paid (aggr_deferred)");
        System.out.println("-------------------------------------------");
        System.out.println("  Test with curl:");
        System.out.println("    curl http://localhost:" + actualPort + "/health");
        System.out.println("    curl http://localhost:" + actualPort + "/api/weather");
        System.out.println("    curl http://localhost:" + actualPort + "/api/deferred");
        System.out.println("===========================================");

        jetty.join();
    }

    static class WeatherServlet extends HttpServlet {
        @Override
        protected void doGet(HttpServletRequest req, HttpServletResponse resp)
                throws IOException {
            resp.setContentType("application/json");
            resp.setStatus(200);
            resp.getWriter().write("{\"weather\":\"sunny\",\"temp\":25,\"scheme\":\"exact\"}");
        }
    }

    static class DeferredServlet extends HttpServlet {
        @Override
        protected void doGet(HttpServletRequest req, HttpServletResponse resp)
                throws IOException {
            resp.setContentType("application/json");
            resp.setStatus(200);
            resp.getWriter().write("{\"data\":\"premium\",\"scheme\":\"aggr_deferred\"}");
        }
    }

    static class HealthServlet extends HttpServlet {
        @Override
        protected void doGet(HttpServletRequest req, HttpServletResponse resp)
                throws IOException {
            resp.setContentType("application/json");
            resp.setStatus(200);
            resp.getWriter().write("{\"status\":\"ok\"}");
        }
    }

    private static String requireEnv(String name) {
        String value = System.getenv(name);
        if (value == null || value.isEmpty()) {
            System.err.println("ERROR: Environment variable " + name + " is required.");
            System.err.println("       Set: export " + name + "=<value>");
            System.exit(1);
        }
        return value;
    }

    private static String envOrDefault(String name, String fallback) {
        String value = System.getenv(name);
        return (value == null || value.isEmpty()) ? fallback : value;
    }

    private static int parsePort(String raw) {
        try {
            int port = Integer.parseInt(raw);
            if (port < 0 || port > 65535) {
                throw new NumberFormatException("out of range");
            }
            return port;
        } catch (NumberFormatException e) {
            System.err.println("ERROR: OKX_TEST_PORT is not a valid TCP port: " + raw);
            System.exit(1);
            return -1;
        }
    }
}
