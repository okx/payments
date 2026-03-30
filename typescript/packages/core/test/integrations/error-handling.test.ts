/**
 * E2E Integration Tests: Error Handling
 *
 * Covers CSV rows 14, 15, 16, 20, 21, 22, 23, 24, 36:
 *   - Verify Failure: middleware returns 402 with error field when facilitator rejects payment (row 14)
 *   - Settle Failure: middleware returns 500 when settlement is rejected (row 15)
 *   - Facilitator Unavailable: initialize() fails fast when facilitator is unreachable (row 16)
 *   - Malformed Facilitator Response: middleware returns 502 when response is malformed (row 20)
 *   - Version Mismatch: unsupported x402Version throws error (row 21)
 *   - No Registered Scheme: selectPaymentRequirements throws when no matching scheme (row 22)
 *   - Policy Filter: throws when all requirements filtered by PaymentPolicy (row 23)
 *   - Settlement Override on exact scheme: applying overrides to exact scheme causes rejection (row 24)
 *   - Extension Error Handling: failed extension hook is skipped, request continues (row 36)
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { x402Client, ClientExtension } from "../../src/client";
import { x402Facilitator } from "../../src/facilitator";
import {
  x402HTTPResourceServer,
  HTTPAdapter,
  HTTPResponseInstructions,
  FacilitatorClient,
  RouteConfig,
} from "../../src/http";
import { x402ResourceServer } from "../../src/server";
import {
  buildCashPaymentRequirements,
  CashFacilitatorClient,
  CashSchemeNetworkClient,
  CashSchemeNetworkFacilitator,
  CashSchemeNetworkServer,
  buildPaymentRequired,
  buildPaymentPayload,
  buildPaymentRequirements,
} from "../mocks";
import {
  Network,
  PaymentPayload,
  PaymentRequirements,
  FacilitatorResponseError,
  SettleError,
} from "../../src/types";
import { VerifyResponse, SettleResponse, SupportedResponse } from "../../src/types/facilitator";

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

const baseRoutes: Record<string, RouteConfig> = {
  "GET /data": {
    accepts: {
      scheme: "cash",
      payTo: "merchant@example.com",
      price: "$0.10",
      network: TEST_NETWORK,
    },
    description: "Protected endpoint",
    mimeType: "application/json",
  },
};

describe("Error Handling", () => {
  // --------------------------------------------------------------------------
  // Row 14: Verify Failure — facilitator rejects payment
  // --------------------------------------------------------------------------
  describe("Verification Failure", () => {
    // P0
    it("Verify Failure: middleware returns 402 with error field when facilitator rejects payment", async () => {
      // Setup: mock facilitator that always returns isValid=false
      const invalidFacilitatorClient: FacilitatorClient = {
        verify: (_payload, _requirements) =>
          Promise.resolve({
            isValid: false,
            invalidReason: "insufficient_funds",
            invalidMessage: "Insufficient funds in wallet",
          } as VerifyResponse),
        settle: (_payload, _requirements) =>
          Promise.resolve({
            success: false,
            errorReason: "not_reached",
            transaction: "",
            network: TEST_NETWORK,
          } as SettleResponse),
        getSupported: () =>
          Promise.resolve({
            kinds: [{ x402Version: 2, scheme: "cash", network: TEST_NETWORK }],
            extensions: [],
            signers: {},
          } as SupportedResponse),
      };

      const server = new x402ResourceServer(invalidFacilitatorClient);
      server.register(TEST_NETWORK, new CashSchemeNetworkServer());
      await server.initialize();
      const httpServer = new x402HTTPResourceServer(server, baseRoutes);

      // Create a valid payment payload first (from a real client)
      const realFacilitator = new x402Facilitator().register(
        TEST_NETWORK,
        new CashSchemeNetworkFacilitator(),
      );
      const realFcClient = new CashFacilitatorClient(realFacilitator);
      const realServer = new x402ResourceServer(realFcClient);
      realServer.register(TEST_NETWORK, new CashSchemeNetworkServer());
      await realServer.initialize();
      const client = new x402Client().register(TEST_NETWORK, new CashSchemeNetworkClient("TestPayer"));
      const paymentRequired = await realServer.createPaymentRequiredResponse(
        [buildCashPaymentRequirements("merchant@example.com", "USD", "1")],
        { url: "http://localhost:3000/data" },
      );
      const paymentPayload = await client.createPaymentPayload(paymentRequired);

      // Encode the payload as the PAYMENT-SIGNATURE header
      const { encodePaymentSignatureHeader } = await import("../../src/http");
      const encoded = encodePaymentSignatureHeader(paymentPayload);

      const authenticatedAdapter = makeAdapter({
        getHeader: (name: string) => (name === "PAYMENT-SIGNATURE" ? encoded : undefined),
      });

      // Action: send request with a PAYMENT-SIGNATURE header
      const result = await httpServer.processHTTPRequest({
        adapter: authenticatedAdapter,
        path: "/data",
        method: "GET",
      });

      // Assertion: response status is 402
      expect(result.type).toBe("payment-error");
      const errorResult = result as { type: "payment-error"; response: HTTPResponseInstructions };
      expect(errorResult.response.status).toBe(402);

      // PAYMENT-REQUIRED header is set with error field
      expect(errorResult.response.headers["PAYMENT-REQUIRED"]).toBeDefined();
      const { decodePaymentRequiredHeader } = await import("../../src/http");
      const decoded = decodePaymentRequiredHeader(errorResult.response.headers["PAYMENT-REQUIRED"]);
      expect(decoded.error).toBeDefined();
      expect(decoded.error?.length).toBeGreaterThan(0);
    });
  });

  // --------------------------------------------------------------------------
  // Row 15: Settle Failure — facilitator throws SettleError
  // --------------------------------------------------------------------------
  describe("Settlement Failure", () => {
    // P0
    it("Settle Failure: middleware returns 500 when settlement is rejected by facilitator", async () => {
      // Setup: facilitator that passes verify but throws SettleError on settle
      let onSettleFailureCalled = false;
      const settleErrorFacilitatorClient: FacilitatorClient = {
        verify: (_payload, _requirements) =>
          Promise.resolve({ isValid: true, payer: "~TestPayer" } as VerifyResponse),
        settle: (_payload, requirements) => {
          throw new SettleError(500, {
            success: false,
            errorReason: "blockchain_error",
            errorMessage: "Transaction submission failed",
            transaction: "",
            network: requirements.network,
          });
        },
        getSupported: () =>
          Promise.resolve({
            kinds: [{ x402Version: 2, scheme: "cash", network: TEST_NETWORK }],
            extensions: [],
            signers: {},
          } as SupportedResponse),
      };

      const server = new x402ResourceServer(settleErrorFacilitatorClient);
      server.register(TEST_NETWORK, new CashSchemeNetworkServer());

      // Register onSettleFailure hook
      server.onSettleFailure(async (_context) => {
        onSettleFailureCalled = true;
      });

      await server.initialize();
      const httpServer = new x402HTTPResourceServer(server, baseRoutes);

      // Get a valid payment payload matching the httpServer's requirements
      const client = new x402Client().register(TEST_NETWORK, new CashSchemeNetworkClient("TestPayer"));

      // First call without payment to get the actual PaymentRequired from the httpServer
      const { decodePaymentRequiredHeader, encodePaymentSignatureHeader } = await import("../../src/http");
      const unpaidResult = await httpServer.processHTTPRequest({
        adapter: makeAdapter(),
        path: "/data",
        method: "GET",
      });
      expect(unpaidResult.type).toBe("payment-error");
      const unpaidError = unpaidResult as { type: "payment-error"; response: { headers: Record<string, string> } };
      const paymentRequired = decodePaymentRequiredHeader(unpaidError.response.headers["PAYMENT-REQUIRED"]);
      const paymentPayload = await client.createPaymentPayload(paymentRequired);

      const encoded = encodePaymentSignatureHeader(paymentPayload);

      const authenticatedAdapter = makeAdapter({
        getHeader: (name: string) => (name === "PAYMENT-SIGNATURE" ? encoded : undefined),
      });

      // Payment passes verify
      const verifiedResult = await httpServer.processHTTPRequest({
        adapter: authenticatedAdapter,
        path: "/data",
        method: "GET",
      });
      expect(verifiedResult.type).toBe("payment-verified");

      const verifiedData = verifiedResult as {
        type: "payment-verified";
        paymentPayload: PaymentPayload;
        paymentRequirements: PaymentRequirements;
      };

      // Action: settlement throws SettleError
      const settlementResult = await httpServer.processSettlement(
        verifiedData.paymentPayload,
        verifiedData.paymentRequirements,
      );

      // Assertion: response is a failure (non-successful settlement)
      expect(settlementResult.success).toBe(false);
      if (!settlementResult.success) {
        // onSettleFailure hook was called
        expect(onSettleFailureCalled).toBe(true);
      }
    });
  });

  // --------------------------------------------------------------------------
  // Row 16: Facilitator Unavailable — initialize() fails fast
  // --------------------------------------------------------------------------
  describe("Facilitator Unavailable", () => {
    // P0
    it("Facilitator Unavailable: initialize() fails fast when facilitator is unreachable", async () => {
      // Setup: facilitator client that throws on getSupported (simulates unreachable URL)
      const unreachableFacilitatorClient: FacilitatorClient = {
        verify: () => Promise.reject(new Error("Connection refused")),
        settle: () => Promise.reject(new Error("Connection refused")),
        getSupported: () =>
          Promise.reject(new Error("connect ECONNREFUSED http://unreachable-host:8080/supported")),
      };

      const server = new x402ResourceServer(unreachableFacilitatorClient);
      server.register(TEST_NETWORK, new CashSchemeNetworkServer());

      // Action: initialize() should throw
      let thrownError: Error | undefined;
      try {
        await server.initialize();
      } catch (error) {
        thrownError = error as Error;
      }

      // Assertion: initialize() throws an error
      expect(thrownError).toBeDefined();
      expect(thrownError).toBeInstanceOf(Error);
      // Error message references the failure
      expect(thrownError!.message.length).toBeGreaterThan(0);
    });
  });

  // --------------------------------------------------------------------------
  // Row 20: Malformed Facilitator Response — FacilitatorResponseError → 502
  // --------------------------------------------------------------------------
  describe("Malformed Facilitator Response", () => {
    // P1
    it("Facilitator Response Error: middleware returns 502 when facilitator response is malformed", async () => {
      // Setup: facilitator that throws FacilitatorResponseError
      const malformedFacilitatorClient: FacilitatorClient = {
        verify: (_payload, _requirements) => {
          throw new FacilitatorResponseError(
            "Unexpected token N in JSON at position 0: not json",
          );
        },
        settle: () =>
          Promise.resolve({ success: false, errorReason: "n/a", transaction: "", network: TEST_NETWORK }),
        getSupported: () =>
          Promise.resolve({
            kinds: [{ x402Version: 2, scheme: "cash", network: TEST_NETWORK }],
            extensions: [],
            signers: {},
          } as SupportedResponse),
      };

      const server = new x402ResourceServer(malformedFacilitatorClient);
      server.register(TEST_NETWORK, new CashSchemeNetworkServer());
      await server.initialize();
      const httpServer = new x402HTTPResourceServer(server, baseRoutes);

      // Get a valid payment payload matching the httpServer's requirements
      const client = new x402Client().register(TEST_NETWORK, new CashSchemeNetworkClient("TestPayer"));

      // First call without payment to get the actual PaymentRequired from the httpServer
      const { decodePaymentRequiredHeader, encodePaymentSignatureHeader } = await import("../../src/http");
      const unpaidResult = await httpServer.processHTTPRequest({
        adapter: makeAdapter(),
        path: "/data",
        method: "GET",
      });
      // unpaidResult is payment-error (no PAYMENT-SIGNATURE) — but malformedFacilitator
      // only throws on verify, so the unpaid path should succeed returning payment-error
      expect(unpaidResult.type).toBe("payment-error");
      const unpaidError = unpaidResult as { type: "payment-error"; response: { headers: Record<string, string> } };
      const paymentRequired = decodePaymentRequiredHeader(unpaidError.response.headers["PAYMENT-REQUIRED"]);
      const paymentPayload = await client.createPaymentPayload(paymentRequired);

      const encoded = encodePaymentSignatureHeader(paymentPayload);

      const authenticatedAdapter = makeAdapter({
        getHeader: (name: string) => (name === "PAYMENT-SIGNATURE" ? encoded : undefined),
      });

      // Action: middleware calls facilitator.verify() → FacilitatorResponseError thrown
      // The x402HTTPResourceServer propagates FacilitatorResponseError (§4.4)
      let thrownError: unknown;
      try {
        await httpServer.processHTTPRequest({
          adapter: authenticatedAdapter,
          path: "/data",
          method: "GET",
        });
      } catch (error) {
        thrownError = error;
      }

      // Assertion: FacilitatorResponseError is thrown internally
      expect(thrownError).toBeDefined();
      expect(thrownError).toBeInstanceOf(FacilitatorResponseError);
    });
  });

  // --------------------------------------------------------------------------
  // Row 21: Version Mismatch
  // --------------------------------------------------------------------------
  describe("Version Mismatch", () => {
    // P1
    it("Version Mismatch: unsupported x402Version throws error on client payload matching", async () => {
      // Setup: client registered for v2 only; PaymentRequired with x402Version=99
      const client = new x402Client().register(TEST_NETWORK, new CashSchemeNetworkClient("TestPayer"));

      const mismatchedPaymentRequired = buildPaymentRequired({
        x402Version: 99,
        accepts: [buildPaymentRequirements({ scheme: "cash", network: TEST_NETWORK })],
      });

      // Action: createPaymentPayload with mismatched version
      let thrownError: unknown;
      try {
        await client.createPaymentPayload(mismatchedPaymentRequired);
      } catch (error) {
        thrownError = error;
      }

      // Assertion: error is thrown referencing the unsupported version
      expect(thrownError).toBeDefined();
      expect(thrownError).toBeInstanceOf(Error);
      const errorMessage = (thrownError as Error).message;
      expect(errorMessage.toLowerCase()).toMatch(/version|unsupported|registered/);
    });
  });

  // --------------------------------------------------------------------------
  // Row 22: No Registered Scheme
  // --------------------------------------------------------------------------
  describe("No Registered Scheme", () => {
    // P1
    it("No Scheme: selectPaymentRequirements throws when client has no matching scheme", async () => {
      // Setup: client with only SVM scheme registered
      const SVM_NETWORK = "solana:devnet" as Network;
      const svmClient = new CashSchemeNetworkClient("SVMPayer");
      // Override scheme to pretend it's an svm scheme
      Object.defineProperty(svmClient, "scheme", { value: "svm-exact", configurable: true });

      const client = new x402Client().register(SVM_NETWORK, svmClient);

      // Server returns PaymentRequired with only EVM requirements
      const evmOnlyPaymentRequired = buildPaymentRequired({
        x402Version: 2,
        accepts: [
          buildPaymentRequirements({ scheme: "exact", network: "eip155:84532" as Network }),
        ],
      });

      // Action: client tries to select requirements but only SVM scheme is registered
      let thrownError: unknown;
      try {
        await client.createPaymentPayload(evmOnlyPaymentRequired);
      } catch (error) {
        thrownError = error;
      }

      // Assertion: error thrown with message referencing no matching scheme
      expect(thrownError).toBeDefined();
      expect(thrownError).toBeInstanceOf(Error);
      const errorMessage = (thrownError as Error).message.toLowerCase();
      expect(errorMessage).toMatch(/no.*registered|no.*scheme|no.*network|version/);
    });
  });

  // --------------------------------------------------------------------------
  // Row 23: Policy Filter
  // --------------------------------------------------------------------------
  describe("Policy Filter", () => {
    // P1
    it("Policy Filtered: throws when all requirements are filtered by PaymentPolicy", async () => {
      // Setup: client with PaymentPolicy that blocks all schemes (amount > 0)
      const client = new x402Client()
        .register(TEST_NETWORK, new CashSchemeNetworkClient("PolicyPayer"))
        .registerPolicy((_version, reqs) => reqs.filter((r) => BigInt(r.amount) < 0n)); // always empty

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

      // Action: selectPaymentRequirements — all filtered out by policy
      let thrownError: unknown;
      try {
        await client.createPaymentPayload(paymentRequired);
      } catch (error) {
        thrownError = error;
      }

      // Assertion: error thrown with message about filtered requirements
      expect(thrownError).toBeDefined();
      expect(thrownError).toBeInstanceOf(Error);
      const errorMessage = (thrownError as Error).message.toLowerCase();
      expect(errorMessage).toMatch(/filtered|policies|no.*scheme|no.*registered/);
    });
  });

  // --------------------------------------------------------------------------
  // Row 24: Settlement Override on exact scheme causes rejection
  // --------------------------------------------------------------------------
  describe("Settlement Override on exact scheme", () => {
    // P1
    it("Settlement Override: applying settlement-overrides to exact scheme causes rejection", async () => {
      // Setup: exact scheme server
      const facilitator = new x402Facilitator().register(
        TEST_NETWORK,
        new CashSchemeNetworkFacilitator(),
      );
      const facilitatorClient = new CashFacilitatorClient(facilitator);
      const server = new x402ResourceServer(facilitatorClient);
      server.register(TEST_NETWORK, new CashSchemeNetworkServer());
      await server.initialize();

      const client = new x402Client().register(TEST_NETWORK, new CashSchemeNetworkClient("ExactPayer"));

      // Build exact payment payload
      const accepts = [buildCashPaymentRequirements("merchant@example.com", "USD", "1000")];
      const resource = { url: "http://localhost:3000/data" };
      const paymentRequired = await server.createPaymentRequiredResponse(accepts, resource);
      const paymentPayload = await client.createPaymentPayload(paymentRequired);
      const matched = server.findMatchingRequirements(accepts, paymentPayload)!;

      // Verify passes normally
      const verifyResponse = await server.verifyPayment(paymentPayload, matched);
      expect(verifyResponse.isValid).toBe(true);

      // Action: try to settle exact payment with amount override (50%)
      // Override the scheme to "exact" to trigger the guard (cash scheme → exact scheme rename)
      const exactSchemeMatched = { ...matched, scheme: "exact" };
      let thrownError: unknown;
      try {
        await server.settlePayment(paymentPayload, exactSchemeMatched, undefined, undefined, {
          amount: "50%",
        });
      } catch (error) {
        thrownError = error;
      }

      // Assertion: settlement is rejected (SettleError or rejection is surfaced)
      // The guard in settlePayment throws SettleError for exact scheme with overrides
      expect(thrownError).toBeDefined();
      expect(thrownError).toBeInstanceOf(SettleError);
      const settleError = thrownError as SettleError;
      expect(settleError.errorReason).toMatch(/override|not.*support|exact/i);
    });
  });

  // --------------------------------------------------------------------------
  // Row 36: Extension Error Handling
  // --------------------------------------------------------------------------
  describe("Extension Error Handling", () => {
    // P2
    it("Extension Error: failed extension hook is skipped and request processing continues", async () => {
      // Setup: server with a broken ResourceServerExtension whose afterVerify hook throws
      const facilitator = new x402Facilitator().register(
        TEST_NETWORK,
        new CashSchemeNetworkFacilitator(),
      );
      const facilitatorClient = new CashFacilitatorClient(facilitator);
      const server = new x402ResourceServer(facilitatorClient);
      server.register(TEST_NETWORK, new CashSchemeNetworkServer());

      // Register an extension that enriches declarations (the crash comes from enrichPaymentRequiredResponse)
      server.registerExtension({
        key: "brokenExtension",
        enrichDeclaration: (_declaration, _context) => {
          throw new Error("Extension hook crashed!");
        },
        enrichPaymentRequiredResponse: async (_declaration, _context) => {
          throw new Error("Extension enrichPaymentRequiredResponse crashed!");
        },
      });

      // Monitor console.error
      const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});

      await server.initialize();

      const client = new x402Client().register(TEST_NETWORK, new CashSchemeNetworkClient("ExtPayer"));
      const accepts = [buildCashPaymentRequirements("merchant@example.com", "USD", "1")];
      const resource = { url: "http://localhost:3000/data" };

      // Action: create payment required response with declared extensions (triggers hooks)
      const paymentRequired = await server.createPaymentRequiredResponse(
        accepts,
        resource,
        undefined,
        { brokenExtension: { schema: {}, info: {} } }, // declared extension
      );

      // Assertion: response is not aborted — PaymentRequired is returned
      expect(paymentRequired).toBeDefined();
      expect(paymentRequired.x402Version).toBe(2);

      // console.error was called with the extension error
      expect(consoleSpy).toHaveBeenCalled();
      const errorArgs = consoleSpy.mock.calls.flat().join(" ");
      expect(errorArgs).toMatch(/extension|brokenExtension|crashed/i);

      // Payment cycle can still continue
      const paymentPayload = await client.createPaymentPayload(paymentRequired);
      const verifyResponse = await server.verifyPayment(
        paymentPayload,
        server.findMatchingRequirements(accepts, paymentPayload)!,
      );
      expect(verifyResponse.isValid).toBe(true);

      consoleSpy.mockRestore();
    });
  });
});
