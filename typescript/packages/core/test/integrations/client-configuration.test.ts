/**
 * E2E Integration Tests: Client Configuration
 *
 * Covers CSV rows 32, 33, 34:
 *   - Max Amount Policy: client refuses to pay when amount exceeds policy ceiling (row 32)
 *   - Dynamic Pricing: server uses DynamicPrice function to compute per-request price (row 33)
 *   - Dynamic PayTo: server uses DynamicPayTo function to compute recipient per request (row 34)
 */

import { beforeEach, describe, expect, it } from "vitest";
import { x402Client } from "../../src/client";
import { x402Facilitator } from "../../src/facilitator";
import {
  x402HTTPResourceServer,
  HTTPAdapter,
  HTTPResponseInstructions,
  RouteConfig,
  DynamicPrice,
  DynamicPayTo,
  decodePaymentRequiredHeader,
} from "../../src/http";
import { x402ResourceServer } from "../../src/server";
import {
  CashFacilitatorClient,
  CashSchemeNetworkClient,
  CashSchemeNetworkFacilitator,
  CashSchemeNetworkServer,
  buildPaymentRequired,
  buildPaymentRequirements,
} from "../mocks";
import { Network } from "../../src/types";

const TEST_NETWORK = "x402:cash" as Network;

function makeAdapter(overrides: Partial<HTTPAdapter> = {}): HTTPAdapter {
  return {
    getHeader: (_name: string) => undefined,
    getMethod: () => "GET",
    getPath: () => "/resource",
    getUrl: () => "http://localhost:3000/resource",
    getAcceptHeader: () => "application/json",
    getUserAgent: () => "TestAgent/1.0",
    ...overrides,
  };
}

async function buildResourceServer(): Promise<x402ResourceServer> {
  const facilitator = new x402Facilitator().register(
    TEST_NETWORK,
    new CashSchemeNetworkFacilitator(),
  );
  const facilitatorClient = new CashFacilitatorClient(facilitator);
  const server = new x402ResourceServer(facilitatorClient);
  server.register(TEST_NETWORK, new CashSchemeNetworkServer());
  await server.initialize();
  return server;
}

describe("Client Configuration", () => {
  // --------------------------------------------------------------------------
  // Row 32: Max Amount Policy
  // --------------------------------------------------------------------------
  describe("Payment Policy", () => {
    // P1
    it("Max Amount Policy: client refuses to pay when required amount exceeds policy ceiling", async () => {
      // Setup: client with maxAmount=100 policy; server requires 1000
      const client = new x402Client()
        .register(TEST_NETWORK, new CashSchemeNetworkClient("PolicyPayer"))
        .registerPolicy((_version, reqs) =>
          // Filter out any requirement with amount > 100
          reqs.filter((r) => BigInt(r.amount) <= 100n),
        );

      // Server returns PaymentRequired with amount=1000 (exceeds policy)
      const paymentRequired = buildPaymentRequired({
        x402Version: 2,
        accepts: [
          buildPaymentRequirements({
            scheme: "cash",
            network: TEST_NETWORK,
            amount: "1000",
          }),
        ],
      });

      // Action: selectPaymentRequirements filters out 1000 > 100
      let thrownError: unknown;
      try {
        await client.createPaymentPayload(paymentRequired);
      } catch (error) {
        thrownError = error;
      }

      // Assertion: error thrown because all requirements filtered by policy
      expect(thrownError).toBeDefined();
      expect(thrownError).toBeInstanceOf(Error);

      // No PAYMENT-SIGNATURE header is sent (payment creation failed before encoding)
      const errorMessage = (thrownError as Error).message.toLowerCase();
      expect(errorMessage).toMatch(/filtered|policies|no.*scheme|no.*registered/);
    });
  });

  // --------------------------------------------------------------------------
  // Row 33: Dynamic Pricing
  // --------------------------------------------------------------------------
  describe("Dynamic Pricing", () => {
    // P2
    it("Dynamic Price: server uses DynamicPrice function to compute per-request price", async () => {
      // Setup: two routes with dynamic prices based on path
      const dynamicPriceRoutes: Record<string, RouteConfig> = {
        "GET /cheap-endpoint": {
          accepts: {
            scheme: "cash",
            payTo: "merchant@example.com",
            // DynamicPrice: returns $0.01 for cheap path
            price: ((_context) => "$0.01") as DynamicPrice,
            network: TEST_NETWORK,
          },
          description: "Cheap endpoint",
        },
        "GET /expensive-endpoint": {
          accepts: {
            scheme: "cash",
            payTo: "merchant@example.com",
            // DynamicPrice: returns $1.00 for expensive path
            price: ((_context) => "$1.00") as DynamicPrice,
            network: TEST_NETWORK,
          },
          description: "Expensive endpoint",
        },
      };

      const resourceServer = await buildResourceServer();
      const httpServer = new x402HTTPResourceServer(resourceServer, dynamicPriceRoutes);

      // Action: send GET /cheap-endpoint
      const cheapAdapter = makeAdapter({
        getPath: () => "/cheap-endpoint",
        getUrl: () => "http://localhost:3000/cheap-endpoint",
      });
      const cheapResult = await httpServer.processHTTPRequest({
        adapter: cheapAdapter,
        path: "/cheap-endpoint",
        method: "GET",
      });

      expect(cheapResult.type).toBe("payment-error");
      const cheapError = cheapResult as { type: "payment-error"; response: HTTPResponseInstructions };
      expect(cheapError.response.status).toBe(402);
      const cheapPR = decodePaymentRequiredHeader(cheapError.response.headers["PAYMENT-REQUIRED"]);
      const cheapAmount = cheapPR.accepts[0].amount;

      // Action: send GET /expensive-endpoint
      const expensiveAdapter = makeAdapter({
        getPath: () => "/expensive-endpoint",
        getUrl: () => "http://localhost:3000/expensive-endpoint",
      });
      const expensiveResult = await httpServer.processHTTPRequest({
        adapter: expensiveAdapter,
        path: "/expensive-endpoint",
        method: "GET",
      });

      expect(expensiveResult.type).toBe("payment-error");
      const expensiveError = expensiveResult as {
        type: "payment-error";
        response: HTTPResponseInstructions;
      };
      const expensivePR = decodePaymentRequiredHeader(
        expensiveError.response.headers["PAYMENT-REQUIRED"],
      );
      const expensiveAmount = expensivePR.accepts[0].amount;

      // Assertion: /cheap-endpoint has lower amount than /expensive-endpoint
      expect(parseFloat(cheapAmount)).toBeLessThan(parseFloat(expensiveAmount));
    });
  });

  // --------------------------------------------------------------------------
  // Row 34: Dynamic PayTo
  // --------------------------------------------------------------------------
  describe("Dynamic PayTo", () => {
    // P2
    it("Dynamic PayTo: server uses DynamicPayTo function to compute recipient per request", async () => {
      // Setup: route with DynamicPayTo that varies by user query param
      const dynamicPayToRoutes: Record<string, RouteConfig> = {
        "GET /resource": {
          accepts: {
            scheme: "cash",
            // DynamicPayTo: returns different address based on user-agent (simulating user context)
            payTo: ((context) => {
              const userAgent = context.adapter.getUserAgent();
              // Route affiliate payments: user X → address A, user Y → address B
              if (userAgent.includes("UserX")) {
                return "address-a@example.com";
              }
              return "address-b@example.com";
            }) as DynamicPayTo,
            price: "$0.10",
            network: TEST_NETWORK,
          },
          description: "Resource with dynamic payment routing",
        },
      };

      const resourceServer = await buildResourceServer();
      const httpServer = new x402HTTPResourceServer(resourceServer, dynamicPayToRoutes);

      // Action: send request from user X context
      const userXAdapter = makeAdapter({
        getUserAgent: () => "TestClient/1.0 UserX",
      });
      const userXResult = await httpServer.processHTTPRequest({
        adapter: userXAdapter,
        path: "/resource",
        method: "GET",
      });

      expect(userXResult.type).toBe("payment-error");
      const userXError = userXResult as { type: "payment-error"; response: HTTPResponseInstructions };
      const userXPR = decodePaymentRequiredHeader(userXError.response.headers["PAYMENT-REQUIRED"]);
      const userXPayTo = userXPR.accepts[0].payTo;

      // Action: send request from user Y context
      const userYAdapter = makeAdapter({
        getUserAgent: () => "TestClient/1.0 UserY",
      });
      const userYResult = await httpServer.processHTTPRequest({
        adapter: userYAdapter,
        path: "/resource",
        method: "GET",
      });

      expect(userYResult.type).toBe("payment-error");
      const userYError = userYResult as { type: "payment-error"; response: HTTPResponseInstructions };
      const userYPR = decodePaymentRequiredHeader(userYError.response.headers["PAYMENT-REQUIRED"]);
      const userYPayTo = userYPR.accepts[0].payTo;

      // Assertion: payTo addresses differ between user X and user Y
      expect(userXPayTo).toBe("address-a@example.com");
      expect(userYPayTo).toBe("address-b@example.com");
      expect(userXPayTo).not.toBe(userYPayTo);
    });
  });
});
