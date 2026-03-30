/**
 * E2E Integration Tests: Server Middleware
 *
 * Covers CSV rows 1, 2, 7, 8:
 *   - Express Integration (rows 1, 2)
 *   - Hono Integration (row 7)
 *   - Next.js Integration (row 8)
 *
 * These tests use the Cash mock scheme/facilitator to simulate framework-agnostic
 * middleware behaviour (the x402HTTPResourceServer is the common core used by all
 * framework adapters). All tests are isolated and do not require a running server.
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
import { x402Client, x402HTTPClient } from "../../src/client";
import { x402Facilitator } from "../../src/facilitator";
import {
  CashFacilitatorClient,
  CashSchemeNetworkClient,
  CashSchemeNetworkFacilitator,
  CashSchemeNetworkServer,
} from "../mocks";
import { Network, PaymentPayload, PaymentRequirements } from "../../src/types";
import { decodePaymentRequiredHeader } from "../../src/http";

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

const TEST_NETWORK = "x402:cash" as Network;

function makeAdapter(overrides: Partial<HTTPAdapter> = {}): HTTPAdapter {
  return {
    getHeader: (_name: string) => undefined,
    getMethod: () => "GET",
    getPath: () => "/data",
    getUrl: () => "http://localhost:3000/data",
    getAcceptHeader: () => "application/json",
    getUserAgent: () => "TestAgent/1.0",
    ...overrides,
  };
}

async function buildServer(
  facilitatorClient?: CashFacilitatorClient,
): Promise<{ resourceServer: x402ResourceServer; facilitatorClient: CashFacilitatorClient }> {
  const facilitator = new x402Facilitator().register(TEST_NETWORK, new CashSchemeNetworkFacilitator());
  const fc = facilitatorClient ?? new CashFacilitatorClient(facilitator);
  const resourceServer = new x402ResourceServer(fc);
  resourceServer.register(TEST_NETWORK, new CashSchemeNetworkServer());
  await resourceServer.initialize();
  return { resourceServer, facilitatorClient: fc };
}

const baseRoutes: Record<string, RouteConfig> = {
  "GET /data": {
    accepts: {
      scheme: "cash",
      payTo: "merchant@example.com",
      price: "$0.10",
      network: TEST_NETWORK,
    },
    description: "Protected data endpoint",
    mimeType: "application/json",
  },
};

// ---------------------------------------------------------------------------
// Row 1: Initialize — middleware fetches supported kinds from facilitator on startup
// ---------------------------------------------------------------------------

describe("Server Middleware", () => {
  describe("Express Integration", () => {
    // P0
    it("Initialize: Express middleware fetches supported kinds from facilitator on startup", async () => {
      // Setup: build server with facilitator
      const facilitator = new x402Facilitator().register(
        TEST_NETWORK,
        new CashSchemeNetworkFacilitator(),
      );
      const facilitatorClient = new CashFacilitatorClient(facilitator);
      const resourceServer = new x402ResourceServer(facilitatorClient);
      resourceServer.register(TEST_NETWORK, new CashSchemeNetworkServer());

      // Action: initialize (equivalent to paymentMiddleware calling server.initialize())
      await resourceServer.initialize();

      // Assertion: supportedResponsesMap is populated (initialize did not throw)
      // Verify the server knows about the cash scheme after initialization
      expect(resourceServer.hasRegisteredScheme(TEST_NETWORK, "cash")).toBe(true);

      // Verify the supported kind is accessible (getSupportedKind returns defined value)
      const supportedKind = resourceServer.getSupportedKind(2, TEST_NETWORK, "cash");
      expect(supportedKind).toBeDefined();
      expect(supportedKind?.scheme).toBe("cash");
      expect(supportedKind?.network).toBe(TEST_NETWORK);
      expect(supportedKind?.x402Version).toBe(2);
    });

    // P0
    it("Payment Required: Express returns HTTP 402 with PAYMENT-REQUIRED header for unauthenticated request", async () => {
      // Setup
      const { resourceServer } = await buildServer();
      const httpServer = new x402HTTPResourceServer(resourceServer, baseRoutes);

      const adapter = makeAdapter();
      const context = { adapter, path: "/data", method: "GET" };

      // Action: send GET /data without a payment header
      const result = await httpServer.processHTTPRequest(context);

      // Assertion
      expect(result.type).toBe("payment-error");
      const errorResult = result as { type: "payment-error"; response: HTTPResponseInstructions };
      expect(errorResult.response.status).toBe(402);
      expect(errorResult.response.headers["PAYMENT-REQUIRED"]).toBeDefined();

      // Decode and validate PaymentRequired structure
      const decoded = decodePaymentRequiredHeader(errorResult.response.headers["PAYMENT-REQUIRED"]);
      expect(decoded.x402Version).toBe(2);
      expect(decoded.resource?.url).toBeDefined();
      expect(decoded.accepts).toBeInstanceOf(Array);
      expect(decoded.accepts.length).toBeGreaterThan(0);
    });
  });

  // --------------------------------------------------------------------------
  // Row 7: Hono Integration — full round-trip via x402HTTPResourceServer
  // --------------------------------------------------------------------------
  describe("Hono Integration", () => {
    let httpServer: x402HTTPResourceServer;
    let httpClient: x402HTTPClient;

    beforeEach(async () => {
      const { resourceServer } = await buildServer();
      httpServer = new x402HTTPResourceServer(resourceServer, baseRoutes);

      const paymentClient = new x402Client().register(TEST_NETWORK, new CashSchemeNetworkClient("HonoUser"));
      httpClient = new x402HTTPClient(paymentClient);
    });

    // P0
    it("Full Round-Trip: Hono paymentMiddleware completes payment cycle end-to-end", async () => {
      // Setup: simulate unauthenticated request
      const adapter = makeAdapter();
      const context = { adapter, path: "/data", method: "GET" };

      // Action step 1: unauthenticated request → 402
      const result402 = await httpServer.processHTTPRequest(context);
      expect(result402.type).toBe("payment-error");
      const errorResult = result402 as { type: "payment-error"; response: HTTPResponseInstructions };
      expect(errorResult.response.status).toBe(402);
      expect(errorResult.response.headers["PAYMENT-REQUIRED"]).toBeDefined();

      // Action step 2: client creates payment payload
      const paymentRequired = httpClient.getPaymentRequiredResponse(
        (name) => errorResult.response.headers[name],
        errorResult.response.body,
      );
      const paymentPayload = await httpClient.createPaymentPayload(paymentRequired);
      const requestHeaders = httpClient.encodePaymentSignatureHeader(paymentPayload);

      // Action step 3: retry with PAYMENT-SIGNATURE
      const authenticatedAdapter = makeAdapter({
        getHeader: (name: string) => {
          if (name === "PAYMENT-SIGNATURE") return requestHeaders["PAYMENT-SIGNATURE"];
          return undefined;
        },
      });
      const contextWithPayment = { adapter: authenticatedAdapter, path: "/data", method: "GET" };
      const result200 = await httpServer.processHTTPRequest(contextWithPayment);

      // Assertion: payment verified
      expect(result200.type).toBe("payment-verified");
      const verifiedResult = result200 as {
        type: "payment-verified";
        paymentPayload: PaymentPayload;
        paymentRequirements: PaymentRequirements;
      };

      // Settle the payment
      const settlementResult = await httpServer.processSettlement(
        verifiedResult.paymentPayload,
        verifiedResult.paymentRequirements,
      );
      expect(settlementResult.success).toBe(true);
      if (settlementResult.success) {
        // PAYMENT-RESPONSE header present
        expect(settlementResult.headers["PAYMENT-RESPONSE"]).toBeDefined();
      }
    });
  });

  // --------------------------------------------------------------------------
  // Row 8: Next.js Integration — same core, different route pattern
  // --------------------------------------------------------------------------
  describe("Next.js Integration", () => {
    let httpServer: x402HTTPResourceServer;
    let httpClient: x402HTTPClient;

    const nextRoutes: Record<string, RouteConfig> = {
      "GET /api/data": {
        accepts: {
          scheme: "cash",
          payTo: "merchant@next.example.com",
          price: "$0.25",
          network: TEST_NETWORK,
        },
        description: "Next.js API protected endpoint",
        mimeType: "application/json",
      },
    };

    beforeEach(async () => {
      const { resourceServer } = await buildServer();
      httpServer = new x402HTTPResourceServer(resourceServer, nextRoutes);

      const paymentClient = new x402Client().register(
        TEST_NETWORK,
        new CashSchemeNetworkClient("NextUser"),
      );
      httpClient = new x402HTTPClient(paymentClient);
    });

    // P0
    it("Full Round-Trip: Next.js API route protected by paymentMiddleware returns 200 after payment", async () => {
      // Setup: unauthenticated request to /api/data
      const adapter = makeAdapter({
        getPath: () => "/api/data",
        getUrl: () => "http://localhost:3000/api/data",
      });
      const context = { adapter, path: "/api/data", method: "GET" };

      // Action: first request — 402
      const result402 = await httpServer.processHTTPRequest(context);
      expect(result402.type).toBe("payment-error");
      const errorResult = result402 as { type: "payment-error"; response: HTTPResponseInstructions };
      expect(errorResult.response.status).toBe(402);
      expect(errorResult.response.headers["PAYMENT-REQUIRED"]).toBeDefined();

      // Decode PaymentRequired — verify it's a valid base64 JSON
      const paymentRequired = httpClient.getPaymentRequiredResponse(
        (name) => errorResult.response.headers[name],
        errorResult.response.body,
      );
      expect(paymentRequired.x402Version).toBe(2);

      // Action: create payment and retry
      const paymentPayload = await httpClient.createPaymentPayload(paymentRequired);
      const requestHeaders = httpClient.encodePaymentSignatureHeader(paymentPayload);

      const authenticatedAdapter = makeAdapter({
        getPath: () => "/api/data",
        getUrl: () => "http://localhost:3000/api/data",
        getHeader: (name: string) => {
          if (name === "PAYMENT-SIGNATURE") return requestHeaders["PAYMENT-SIGNATURE"];
          return undefined;
        },
      });

      const result200 = await httpServer.processHTTPRequest({
        adapter: authenticatedAdapter,
        path: "/api/data",
        method: "GET",
      });

      // Assertion: payment verified → 200 response
      expect(result200.type).toBe("payment-verified");
      const verifiedResult = result200 as {
        type: "payment-verified";
        paymentPayload: PaymentPayload;
        paymentRequirements: PaymentRequirements;
      };

      const settlementResult = await httpServer.processSettlement(
        verifiedResult.paymentPayload,
        verifiedResult.paymentRequirements,
      );
      expect(settlementResult.success).toBe(true);
      if (settlementResult.success) {
        // PAYMENT-RESPONSE is base64 JSON SettleResponse
        expect(settlementResult.headers["PAYMENT-RESPONSE"]).toBeDefined();
      }
    });
  });
});
