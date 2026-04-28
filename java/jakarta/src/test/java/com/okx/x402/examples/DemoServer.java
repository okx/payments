// SPDX-License-Identifier: Apache-2.0
// OKX x402 Java SDK — Demo Server (Runnable)
//
// Start:
//   mvn exec:java -Dexec.mainClass="com.okx.x402.examples.DemoServer" -Dexec.classpathScope=test
// Or run main() from IDE.
//
// Environment variables:
//   OKX_API_KEY      - OKX API key
//   OKX_SECRET_KEY   - OKX secret key
//   OKX_PASSPHRASE   - OKX passphrase
//   PAY_TO_ADDRESS   - Receiver wallet address. STRONGLY recommended to set
//                      this to your own treasury address. The fallback value
//                      below is a non-functional placeholder used only so the
//                      demo can boot for a quick smoke test; do NOT use it
//                      in any environment where real on-chain funds may flow.
//   PORT             - Listen port (default 8080)

package com.okx.x402.examples;

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
 * Runnable demo server with x402 payment middleware.
 *
 * <p>Exposes three endpoints:
 * <ul>
 *   <li>{@code GET /health} — free health check</li>
 *   <li>{@code GET /protected/xlayer-usdt} — paid with exact scheme, $0.01</li>
 *   <li>{@code GET /protected/xlayer-testnet} — paid with exact scheme on testnet</li>
 * </ul>
 *
 * <p><strong>Operator note:</strong> the {@code PAY_TO_ADDRESS} env var
 * controls the recipient of every settled payment. The fallback used when
 * the variable is not set is a placeholder ({@code 0x0000…0000}) and exists
 * solely so that the example compiles and starts; running this demo in any
 * environment where on-chain funds may flow without supplying your own
 * treasury address will silently send those funds to the placeholder. Always
 * set {@code PAY_TO_ADDRESS} before exposing the demo to any signed client.
 */
public class DemoServer {

    /**
     * Demo placeholder recipient. Intentionally the zero address so any
     * accidental on-chain settle against an unconfigured demo is a clear
     * loss-of-funds signal in block explorers rather than a dust transfer
     * to a real wallet. See class-level Javadoc for the operator-set
     * {@code PAY_TO_ADDRESS} env var that should always override this in
     * non-toy deployments.
     */
    private static final String DEMO_PLACEHOLDER_PAY_TO =
            "0x0000000000000000000000000000000000000000";

    public static void main(String[] args) throws Exception {
        String apiKey = requireEnv("OKX_API_KEY");
        String secretKey = requireEnv("OKX_SECRET_KEY");
        String passphrase = requireEnv("OKX_PASSPHRASE");
        String payTo = System.getenv("PAY_TO_ADDRESS") != null
                ? System.getenv("PAY_TO_ADDRESS")
                : DEMO_PLACEHOLDER_PAY_TO;
        int port = System.getenv("PORT") != null
                ? Integer.parseInt(System.getenv("PORT")) : 8080;

        OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
                apiKey, secretKey, passphrase);

        PaymentProcessor.RouteConfig xlayerUsdt = new PaymentProcessor.RouteConfig();
        xlayerUsdt.scheme = "exact";
        xlayerUsdt.network = "eip155:196";
        xlayerUsdt.payTo = payTo;
        xlayerUsdt.price = "$0.01";

        PaymentProcessor.RouteConfig xlayerTestnet = new PaymentProcessor.RouteConfig();
        xlayerTestnet.scheme = "exact";
        xlayerTestnet.network = "eip155:195";
        xlayerTestnet.payTo = payTo;
        xlayerTestnet.price = "$0.01";

        PaymentProcessor.RouteConfig xlayerAggrDeferred = new PaymentProcessor.RouteConfig();
        xlayerAggrDeferred.scheme = "aggr_deferred";
        xlayerAggrDeferred.network = "eip155:196";
        xlayerAggrDeferred.payTo = payTo;
        xlayerAggrDeferred.price = "$0.01";

        PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
                "GET /protected/xlayer-usdt", xlayerUsdt,
                "/protected/xlayer-usdt", xlayerUsdt,
                "GET /protected/xlayer-testnet", xlayerTestnet,
                "/protected/xlayer-testnet", xlayerTestnet,
                "GET /protected/xlayer-aggr-deferred", xlayerAggrDeferred,
                "/protected/xlayer-aggr-deferred", xlayerAggrDeferred
        ));

        Server jetty = new Server(port);
        ServletContextHandler ctx = new ServletContextHandler(ServletContextHandler.SESSIONS);
        ctx.setContextPath("/");

        ctx.addFilter(new FilterHolder(filter), "/*",
                EnumSet.of(jakarta.servlet.DispatcherType.REQUEST));

        ctx.addServlet(new ServletHolder(new HealthServlet()), "/health");
        ctx.addServlet(new ServletHolder(new PremiumServlet("X Layer USDT")), "/protected/xlayer-usdt");
        ctx.addServlet(new ServletHolder(new PremiumServlet("X Layer Testnet")), "/protected/xlayer-testnet");
        ctx.addServlet(new ServletHolder(new PremiumServlet("X Layer aggr_deferred")), "/protected/xlayer-aggr-deferred");

        jetty.setHandler(ctx);
        jetty.start();

        int actualPort = ((ServerConnector) jetty.getConnectors()[0]).getLocalPort();
        System.out.println("===========================================");
        System.out.println("  x402 Demo Server STARTED");
        System.out.println("  Port: " + actualPort);
        System.out.println("  PayTo: " + payTo);
        System.out.println("-------------------------------------------");
        System.out.println("  GET /health                  — free");
        System.out.println("  GET /protected/xlayer-usdt           — $0.01 (exact)");
        System.out.println("  GET /protected/xlayer-testnet        — $0.01 (exact, testnet)");
        System.out.println("  GET /protected/xlayer-aggr-deferred  — $0.01 (aggr_deferred)");
        System.out.println("===========================================");

        jetty.join();
    }

    static class HealthServlet extends HttpServlet {
        @Override
        protected void doGet(HttpServletRequest req, HttpServletResponse resp)
                throws IOException {
            resp.setContentType("application/json");
            resp.getWriter().write("{\"status\":\"ok\"}");
        }
    }

    static class PremiumServlet extends HttpServlet {
        private final String label;

        PremiumServlet(String label) {
            this.label = label;
        }

        @Override
        protected void doGet(HttpServletRequest req, HttpServletResponse resp)
                throws IOException {
            resp.setContentType("application/json");
            resp.getWriter().write("{\"data\":\"premium content\",\"source\":\"" + label + "\"}");
        }
    }

    private static String requireEnv(String name) {
        String value = System.getenv(name);
        if (value == null || value.isEmpty()) {
            System.err.println("ERROR: Environment variable " + name + " is required.");
            System.err.println("Set: export " + name + "=<value>");
            System.exit(1);
        }
        return value;
    }
}
