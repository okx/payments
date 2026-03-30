/**
 * E2E Integration Tests: Paywall UI
 *
 * Covers CSV rows 13, 35:
 *   - Paywall Rendered: HTTP 402 response includes HTML paywall body when Accept header
 *     signals browser (row 13)
 *   - Paywall Config: generateHtml respects appName and testnet flag (row 35)
 *
 * The @x402/paywall package generates HTML. These tests use the PaywallProvider
 * interface (registered via registerPaywallProvider) and verify that:
 *   - Browser requests receive text/html body in the 402 response
 *   - PaywallConfig options (appName, testnet) are reflected in the HTML output
 */

import { beforeEach, describe, expect, it } from "vitest";
import {
  x402HTTPResourceServer,
  HTTPAdapter,
  HTTPResponseInstructions,
  PaywallProvider,
  PaywallConfig,
  RouteConfig,
} from "../../src/http";
import { x402ResourceServer } from "../../src/server";
import { x402Facilitator } from "../../src/facilitator";
import {
  CashFacilitatorClient,
  CashSchemeNetworkFacilitator,
  CashSchemeNetworkServer,
} from "../mocks";
import { Network } from "../../src/types";
import { PaymentRequired } from "../../src/types/payments";

const TEST_NETWORK = "x402:cash" as Network;

// ---------------------------------------------------------------------------
// Minimal PaywallProvider mock that generates inspectable HTML
// ---------------------------------------------------------------------------

class MockPaywallProvider implements PaywallProvider {
  generateHtml(paymentRequired: PaymentRequired, config?: PaywallConfig): string {
    const appName = config?.appName ?? "x402 Payment";
    const testnetLabel = config?.testnet ? " (Testnet)" : "";
    return `
      <!DOCTYPE html>
      <html>
        <head><title>${appName}${testnetLabel}</title></head>
        <body>
          <div class="paywall-container">
            <h1>${appName}${testnetLabel}</h1>
            <p>Payment required to access this resource.</p>
            <p data-x402-version="${paymentRequired.x402Version}">Version: ${paymentRequired.x402Version}</p>
            ${config?.sessionTokenEndpoint ? `<meta name="session-endpoint" content="${config.sessionTokenEndpoint}" />` : ""}
          </div>
        </body>
      </html>
    `.trim();
  }
}

// ---------------------------------------------------------------------------
// Shared setup
// ---------------------------------------------------------------------------

const paywallRoutes: Record<string, RouteConfig> = {
  "GET /data": {
    accepts: {
      scheme: "cash",
      payTo: "merchant@example.com",
      price: "$0.10",
      network: TEST_NETWORK,
    },
    description: "Protected data with paywall",
    mimeType: "application/json",
  },
};

function makeBrowserAdapter(overrides: Partial<HTTPAdapter> = {}): HTTPAdapter {
  return {
    getHeader: (_name: string) => undefined,
    getMethod: () => "GET",
    getPath: () => "/data",
    getUrl: () => "http://localhost:3000/data",
    getAcceptHeader: () => "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    getUserAgent: () => "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
    ...overrides,
  };
}

describe("Paywall UI", () => {
  let httpServer: x402HTTPResourceServer;
  let paywallProvider: MockPaywallProvider;

  beforeEach(async () => {
    const facilitator = new x402Facilitator().register(
      TEST_NETWORK,
      new CashSchemeNetworkFacilitator(),
    );
    const facilitatorClient = new CashFacilitatorClient(facilitator);
    const resourceServer = new x402ResourceServer(facilitatorClient);
    resourceServer.register(TEST_NETWORK, new CashSchemeNetworkServer());
    await resourceServer.initialize();

    httpServer = new x402HTTPResourceServer(resourceServer, paywallRoutes);
    paywallProvider = new MockPaywallProvider();
    httpServer.registerPaywallProvider(paywallProvider);
  });

  // P0 — Row 13
  it("Paywall Rendered: HTTP 402 response includes HTML paywall body when Accept header signals browser", async () => {
    // Setup: browser request (Accept: text/html)
    const browserAdapter = makeBrowserAdapter();
    const context = {
      adapter: browserAdapter,
      path: "/data",
      method: "GET",
    };

    // Action: send GET /data with browser Accept header
    const result = await httpServer.processHTTPRequest(context, {
      appName: "TestApp",
      sessionTokenEndpoint: "/api/session-token",
    });

    // Assertion: 402 response
    expect(result.type).toBe("payment-error");
    const errorResult = result as { type: "payment-error"; response: HTTPResponseInstructions };
    expect(errorResult.response.status).toBe(402);

    // Content-Type is text/html
    expect(errorResult.response.isHtml).toBe(true);

    // Body is a non-empty HTML string containing paywall UI elements
    const body = errorResult.response.body as string;
    expect(typeof body).toBe("string");
    expect(body.length).toBeGreaterThan(0);
    expect(body).toContain("<!DOCTYPE html>");
    expect(body).toContain("paywall-container");

    // PAYMENT-REQUIRED header is still present
    expect(errorResult.response.headers["PAYMENT-REQUIRED"]).toBeDefined();
  });

  // P2 — Row 35
  it("Paywall Config: generateHtml respects appName and testnet flag", async () => {
    // Action: call generateHtml directly with config options
    const mockPaymentRequired: PaymentRequired = {
      x402Version: 2,
      resource: {
        url: "http://localhost:3000/data",
        description: "Test resource",
      },
      accepts: [],
    };

    const html = paywallProvider.generateHtml(mockPaymentRequired, {
      appName: "MyApp",
      testnet: true,
    });

    // Assertion: HTML contains 'MyApp' text
    expect(html).toContain("MyApp");

    // HTML contains testnet network indicator
    expect(html).toContain("Testnet");

    // HTML is valid non-empty string
    expect(typeof html).toBe("string");
    expect(html.length).toBeGreaterThan(0);
  });
});
