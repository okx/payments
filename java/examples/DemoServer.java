// OKX x402 Java SDK — Demo Server
// Requires Spring Boot (not included in SDK dependencies)
//
// Run: mvn spring-boot:run (with a separate Spring Boot pom)
// Or: copy this into your Spring Boot project

package com.okx.x402.examples;

import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.facilitator.FacilitatorRouter;
import com.okx.x402.server.PaymentFilter;

import java.util.Map;

/**
 * Demo server showing x402 payment middleware integration.
 *
 * Environment variables required:
 *   OKX_API_KEY      - OKX API key
 *   OKX_SECRET_KEY   - OKX secret key
 *   OKX_PASSPHRASE   - OKX passphrase
 *   PAY_TO_ADDRESS   - Receiver wallet address
 *
 * Usage with Spring Boot:
 *   @SpringBootApplication
 *   public class DemoServer implements ServletContextInitializer {
 *       @Override
 *       public void onStartup(ServletContext ctx) {
 *           // Build facilitator
 *           FacilitatorClient facilitator = FacilitatorRouter.builder()
 *               .okx(System.getenv("OKX_API_KEY"),
 *                    System.getenv("OKX_SECRET_KEY"),
 *                    System.getenv("OKX_PASSPHRASE"))
 *               .build();
 *
 *           String payTo = System.getenv("PAY_TO_ADDRESS");
 *
 *           // Configure route pricing
 *           PaymentFilter.RouteConfig xlayerUsdt = new PaymentFilter.RouteConfig();
 *           xlayerUsdt.network = "eip155:196";
 *           xlayerUsdt.payTo = payTo;
 *           xlayerUsdt.price = "$0.01";
 *
 *           PaymentFilter.RouteConfig xlayerTestnet = new PaymentFilter.RouteConfig();
 *           xlayerTestnet.network = "eip155:195";
 *           xlayerTestnet.payTo = payTo;
 *           xlayerTestnet.price = "$0.01";
 *
 *           Map<String, PaymentFilter.RouteConfig> routes = Map.of(
 *               "GET /protected/xlayer-usdt", xlayerUsdt,
 *               "GET /protected/xlayer-testnet", xlayerTestnet
 *           );
 *
 *           // One-line middleware registration
 *           FilterRegistration.Dynamic filter = ctx.addFilter(
 *               "x402", PaymentFilter.create(facilitator, routes));
 *           filter.addMappingForUrlPatterns(null, false, "/protected/*");
 *       }
 *
 *       @RestController
 *       static class Routes {
 *           @GetMapping("/health")
 *           String health() { return "{\"status\":\"ok\"}"; }
 *
 *           @GetMapping("/protected/xlayer-usdt")
 *           String xlayerUsdt() { return "{\"data\":\"premium X Layer content\"}"; }
 *
 *           @GetMapping("/protected/xlayer-testnet")
 *           String xlayerTestnet() { return "{\"data\":\"testnet content\"}"; }
 *       }
 *   }
 */
public class DemoServer {
    // This is an example file - see Javadoc above for usage
    private DemoServer() {}
}
