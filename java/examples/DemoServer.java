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
//   PAY_TO_ADDRESS   - Receiver wallet address (default: demo address)

package com.okx.x402.examples;

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
 * Runnable demo server with x402 payment middleware.
 *
 * <p>Exposes three endpoints:
 * <ul>
 *   <li>{@code GET /health} — free health check</li>
 *   <li>{@code GET /protected/xlayer-usdt} — paid with exact scheme, $0.01</li>
 *   <li>{@code GET /protected/xlayer-testnet} — paid with exact scheme on testnet</li>
 * </ul>
 */
public class DemoServer {

    public static void main(String[] args) throws Exception {
        String apiKey = requireEnv("OKX_API_KEY");
        String secretKey = requireEnv("OKX_SECRET_KEY");
        String passphrase = requireEnv("OKX_PASSPHRASE");
        String payTo = System.getenv("PAY_TO_ADDRESS") != null
                ? System.getenv("PAY_TO_ADDRESS")
                : "0xd872a137ce2ece0f54582ce0ffb94ba6565a5739";
        int port = System.getenv("PORT") != null
                ? Integer.parseInt(System.getenv("PORT")) : 8080;

        OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
                apiKey, secretKey, passphrase);

        PaymentFilter.RouteConfig xlayerUsdt = new PaymentFilter.RouteConfig();
        xlayerUsdt.scheme = "exact";
        xlayerUsdt.network = "eip155:196";
        xlayerUsdt.payTo = payTo;
        xlayerUsdt.price = "$0.01";

        PaymentFilter.RouteConfig xlayerTestnet = new PaymentFilter.RouteConfig();
        xlayerTestnet.scheme = "exact";
        xlayerTestnet.network = "eip155:195";
        xlayerTestnet.payTo = payTo;
        xlayerTestnet.price = "$0.01";

        PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
                "GET /protected/xlayer-usdt", xlayerUsdt,
                "/protected/xlayer-usdt", xlayerUsdt,
                "GET /protected/xlayer-testnet", xlayerTestnet,
                "/protected/xlayer-testnet", xlayerTestnet
        ));

        Server jetty = new Server(port);
        ServletContextHandler ctx = new ServletContextHandler(ServletContextHandler.SESSIONS);
        ctx.setContextPath("/");

        ctx.addFilter(new FilterHolder(filter), "/*",
                EnumSet.of(jakarta.servlet.DispatcherType.REQUEST));

        ctx.addServlet(new ServletHolder(new HealthServlet()), "/health");
        ctx.addServlet(new ServletHolder(new PremiumServlet("X Layer USDT")), "/protected/xlayer-usdt");
        ctx.addServlet(new ServletHolder(new PremiumServlet("X Layer Testnet")), "/protected/xlayer-testnet");

        jetty.setHandler(ctx);
        jetty.start();

        int actualPort = ((ServerConnector) jetty.getConnectors()[0]).getLocalPort();
        System.out.println("===========================================");
        System.out.println("  x402 Demo Server STARTED");
        System.out.println("  Port: " + actualPort);
        System.out.println("  PayTo: " + payTo);
        System.out.println("-------------------------------------------");
        System.out.println("  GET /health                  — free");
        System.out.println("  GET /protected/xlayer-usdt   — $0.01 (exact)");
        System.out.println("  GET /protected/xlayer-testnet— $0.01 (exact)");
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
