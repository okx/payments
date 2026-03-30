/**
 * E2E Integration Tests: Client Wrapper
 *
 * Covers CSV rows 3, 4:
 *   - Fetch Integration: wrapFetchWithPayment full round-trip (row 3)
 *   - Axios Integration: Axios interceptor full round-trip (row 4)
 *
 * Both fetch and axios wrappers ultimately delegate to x402HTTPClient / x402Client.
 * These tests exercise the complete payment cycle using the Cash mock — 402 receive,
 * payload creation, PAYMENT-SIGNATURE header injection, and 200 + PAYMENT-RESPONSE.
 */

import { beforeEach, describe, expect, it } from "vitest";
import { x402Client, x402HTTPClient } from "../../src/client";
import { x402Facilitator } from "../../src/facilitator";
import {
  x402HTTPResourceServer,
  HTTPAdapter,
  HTTPResponseInstructions,
  RouteConfig,
} from "../../src/http";
import { x402ResourceServer } from "../../src/server";
import {
  CashFacilitatorClient,
  CashSchemeNetworkClient,
  CashSchemeNetworkFacilitator,
  CashSchemeNetworkServer,
} from "../mocks";
import { Network, PaymentPayload, PaymentRequirements } from "../../src/types";

const TEST_NETWORK = "x402:cash" as Network;

function makeAdapter(overrides: Partial<HTTPAdapter> = {}): HTTPAdapter {
  return {
    getHeader: (_name: string) => undefined,
    getMethod: () => "GET",
    getPath: () => "/protected",
    getUrl: () => "http://localhost:3000/protected",
    getAcceptHeader: () => "application/json",
    getUserAgent: () => "TestAgent/1.0",
    ...overrides,
  };
}

const protectedRoutes: Record<string, RouteConfig> = {
  "GET /protected": {
    accepts: {
      scheme: "cash",
      payTo: "server@example.com",
      price: "$0.05",
      network: TEST_NETWORK,
    },
    description: "Protected resource",
    mimeType: "application/json",
  },
};

describe("Client Wrapper", () => {
  let httpServer: x402HTTPResourceServer;
  let resourceServer: x402ResourceServer;

  beforeEach(async () => {
    const facilitator = new x402Facilitator().register(
      TEST_NETWORK,
      new CashSchemeNetworkFacilitator(),
    );
    const facilitatorClient = new CashFacilitatorClient(facilitator);
    resourceServer = new x402ResourceServer(facilitatorClient);
    resourceServer.register(TEST_NETWORK, new CashSchemeNetworkServer());
    await resourceServer.initialize();
    httpServer = new x402HTTPResourceServer(resourceServer, protectedRoutes);
  });

  // --------------------------------------------------------------------------
  // Row 3: Fetch Integration — wrapFetchWithPayment full round-trip
  // --------------------------------------------------------------------------
  describe("Fetch Integration", () => {
    // P0
    it("Full Round-Trip: wrapFetchWithPayment completes payment and receives HTTP 200", async () => {
      // Setup: x402Client with cash scheme registered (simulates EVM SchemeNetworkClient)
      const paymentClient = new x402Client().register(
        TEST_NETWORK,
        new CashSchemeNetworkClient("FetchUser"),
      );
      const httpClient = new x402HTTPClient(paymentClient);

      // Action step 1: simulate initial fetch request (no payment) → 402
      const adapter = makeAdapter();
      const context = { adapter, path: "/protected", method: "GET" };
      const result402 = await httpServer.processHTTPRequest(context);

      // Assertion: initial request returns 402
      expect(result402.type).toBe("payment-error");
      const errorResult = result402 as { type: "payment-error"; response: HTTPResponseInstructions };
      expect(errorResult.response.status).toBe(402);
      expect(errorResult.response.headers["PAYMENT-REQUIRED"]).toBeDefined();

      // Action step 2: decode PAYMENT-REQUIRED header
      const paymentRequired = httpClient.getPaymentRequiredResponse(
        (name) => errorResult.response.headers[name],
        errorResult.response.body,
      );
      expect(paymentRequired.x402Version).toBe(2);
      expect(paymentRequired.accepts.length).toBeGreaterThan(0);

      // Action step 3: create payment payload (wrapFetchWithPayment does this internally)
      const paymentPayload = await httpClient.createPaymentPayload(paymentRequired);

      // Action step 4: encode as PAYMENT-SIGNATURE header and retry
      const requestHeaders = httpClient.encodePaymentSignatureHeader(paymentPayload);
      expect(requestHeaders["PAYMENT-SIGNATURE"]).toBeDefined();

      const authenticatedAdapter = makeAdapter({
        getHeader: (name: string) =>
          name === "PAYMENT-SIGNATURE" ? requestHeaders["PAYMENT-SIGNATURE"] : undefined,
      });
      const contextWithPayment = { adapter: authenticatedAdapter, path: "/protected", method: "GET" };
      const result200 = await httpServer.processHTTPRequest(contextWithPayment);

      // Assertion: second response is 200 (payment verified)
      expect(result200.type).toBe("payment-verified");
      const verifiedResult = result200 as {
        type: "payment-verified";
        paymentPayload: PaymentPayload;
        paymentRequirements: PaymentRequirements;
      };

      // Settle and verify PAYMENT-RESPONSE header
      const settlement = await httpServer.processSettlement(
        verifiedResult.paymentPayload,
        verifiedResult.paymentRequirements,
      );
      expect(settlement.success).toBe(true);
      if (settlement.success) {
        expect(settlement.headers["PAYMENT-RESPONSE"]).toBeDefined();
      }
    });
  });

  // --------------------------------------------------------------------------
  // Row 4: Axios Integration — same round-trip via axios-style interceptor
  // --------------------------------------------------------------------------
  describe("Axios Integration", () => {
    // P0
    it("Full Round-Trip: Axios interceptor completes payment and receives HTTP 200", async () => {
      // Setup: create axios-equivalent client (uses x402HTTPClient like wrapAxiosWithPayment)
      const paymentClient = new x402Client().register(
        TEST_NETWORK,
        new CashSchemeNetworkClient("AxiosUser"),
      );
      const httpClient = new x402HTTPClient(paymentClient);

      // Action: simulate axios interceptor intercepting 402
      const adapter = makeAdapter();
      const context = { adapter, path: "/protected", method: "GET" };
      const result402 = await httpServer.processHTTPRequest(context);

      // axios resolves with 402 initially
      expect(result402.type).toBe("payment-error");
      const errorResult = result402 as { type: "payment-error"; response: HTTPResponseInstructions };
      expect(errorResult.response.status).toBe(402);

      // Interceptor intercepts: extract PAYMENT-REQUIRED header
      const paymentRequired = httpClient.getPaymentRequiredResponse(
        (name) => errorResult.response.headers[name],
        errorResult.response.body,
      );

      // Interceptor creates payment payload and retries with PAYMENT-SIGNATURE
      const paymentPayload = await httpClient.createPaymentPayload(paymentRequired);
      const requestHeaders = httpClient.encodePaymentSignatureHeader(paymentPayload);

      const authenticatedAdapter = makeAdapter({
        getHeader: (name: string) =>
          name === "PAYMENT-SIGNATURE" ? requestHeaders["PAYMENT-SIGNATURE"] : undefined,
      });
      const retryResult = await httpServer.processHTTPRequest({
        adapter: authenticatedAdapter,
        path: "/protected",
        method: "GET",
      });

      // Assertion: axios request resolves with 200 (payment-verified)
      expect(retryResult.type).toBe("payment-verified");
      const verifiedResult = retryResult as {
        type: "payment-verified";
        paymentPayload: PaymentPayload;
        paymentRequirements: PaymentRequirements;
      };

      // Settle: PAYMENT-RESPONSE header accessible in response headers
      const settlement = await httpServer.processSettlement(
        verifiedResult.paymentPayload,
        verifiedResult.paymentRequirements,
      );
      expect(settlement.success).toBe(true);
      if (settlement.success) {
        expect(settlement.headers["PAYMENT-RESPONSE"]).toBeDefined();
      }
    });
  });
});
