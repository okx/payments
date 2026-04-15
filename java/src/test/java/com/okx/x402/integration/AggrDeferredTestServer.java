package com.okx.x402.integration;

import com.okx.x402.facilitator.OKXFacilitatorClient;
import com.okx.x402.server.PaymentFilter;

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
 * Accepts both "exact" and "aggr_deferred" schemes on X Layer.
 * Uses prod OKX facilitator (web3.okx.com) with real credentials.
 *
 * Start: mvn exec:java -Dexec.mainClass="com.okx.x402.integration.AggrDeferredTestServer" -Dexec.classpathScope=test
 * Or run main() directly from IDE.
 */
public class AggrDeferredTestServer {

    static final String PROD_BASE_URL = "https://web3.okx.com";
    static final String API_KEY = "da15f95b-b8ab-40ec-8abe-b74ef53e3e67";
    static final String SECRET_KEY = "80B2442B1B7D30D1F937CF071CB460B5";
    static final String PASSPHRASE = "Sdk!123456";
    static final String PAY_TO = "0xd872a137ce2ece0f54582ce0ffb94ba6565a5739";
    static final int PORT = 4402;

    public static void main(String[] args) throws Exception {
        // 1. Create facilitator client pointing to prod
        OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
                API_KEY, SECRET_KEY, PASSPHRASE, PROD_BASE_URL);

        // 2. Configure routes — accept BOTH exact and aggr_deferred
        //    The PaymentFilter uses scheme from the incoming payment header,
        //    so we create two route entries for the same path.
        PaymentFilter.RouteConfig exactRoute = new PaymentFilter.RouteConfig();
        exactRoute.scheme = "exact";
        exactRoute.network = "eip155:196";
        exactRoute.payTo = PAY_TO;
        exactRoute.price = "$0.000001"; // 1 atomic unit = minimum
        exactRoute.maxTimeoutSeconds = 86400;

        PaymentFilter.RouteConfig deferredRoute = new PaymentFilter.RouteConfig();
        deferredRoute.scheme = "aggr_deferred";
        deferredRoute.network = "eip155:196";
        deferredRoute.payTo = PAY_TO;
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
        Server jetty = new Server(PORT);
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
        System.out.println("  Port:       " + actualPort);
        System.out.println("  Facilitator: " + PROD_BASE_URL);
        System.out.println("  PayTo:      " + PAY_TO);
        System.out.println("  Network:    eip155:196 (X Layer)");
        System.out.println("  Asset:      USDT (0x779d...3736)");
        System.out.println("  Price:      $0.000001 (min amount)");
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
}
