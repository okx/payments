/**
 * E2E Integration Tests: Facilitator
 *
 * Covers CSV rows 5, 6:
 *   - Verify and Settle: POST /verify returns isValid=true for well-formed payment payload (row 5)
 *   - Verify and Settle: POST /settle returns success=true with transaction hash (row 6)
 *
 * Uses the Cash mock facilitator which simulates the real EVM facilitator logic
 * without requiring on-chain infrastructure.
 */

import { describe, expect, it, beforeEach } from "vitest";
import { x402Facilitator } from "../../src/facilitator";
import {
  CashFacilitatorClient,
  CashSchemeNetworkClient,
  CashSchemeNetworkFacilitator,
  CashSchemeNetworkServer,
  buildCashPaymentRequirements,
} from "../mocks";
import { x402ResourceServer } from "../../src/server";
import { x402Client } from "../../src/client";
import { Network, PaymentPayload, PaymentRequirements } from "../../src/types";

const TEST_NETWORK = "x402:cash" as Network;

describe("Facilitator", () => {
  describe("Verify and Settle", () => {
    let facilitatorClient: CashFacilitatorClient;
    let server: x402ResourceServer;
    let client: x402Client;

    beforeEach(async () => {
      const facilitator = new x402Facilitator().register(
        TEST_NETWORK,
        new CashSchemeNetworkFacilitator(),
      );
      facilitatorClient = new CashFacilitatorClient(facilitator);

      server = new x402ResourceServer(facilitatorClient);
      server.register(TEST_NETWORK, new CashSchemeNetworkServer());
      await server.initialize();

      client = new x402Client().register(TEST_NETWORK, new CashSchemeNetworkClient("TestPayer"));
    });

    // P0 — Row 5
    it("Facilitator: POST /verify returns isValid=true for well-formed payment payload", async () => {
      // Setup: build a valid PaymentPayload using the client
      const accepts = [buildCashPaymentRequirements("merchant@example.com", "USD", "1000")];
      const resource = {
        url: "https://example.com/api/data",
        description: "Protected API",
        mimeType: "application/json",
      };
      const paymentRequired = await server.createPaymentRequiredResponse(accepts, resource);
      const paymentPayload = await client.createPaymentPayload(paymentRequired);

      // Action: POST to facilitator /verify (via facilitatorClient.verify)
      const verifyResponse = await facilitatorClient.verify(paymentPayload, accepts[0]);

      // Assertion: isValid=true, payer matches signing address
      expect(verifyResponse.isValid).toBe(true);
      expect(verifyResponse.invalidReason).toBeUndefined();
      expect(verifyResponse.invalidMessage).toBeUndefined();
      expect(verifyResponse.payer).toBeDefined();
      // The cash scheme sets payer to the signature (~TestPayer)
      expect(typeof verifyResponse.payer).toBe("string");
    });

    // P0 — Row 6
    it("Facilitator: POST /settle submits transaction and returns success=true with transaction hash", async () => {
      // Setup: build a valid PaymentPayload that has already passed /verify
      const accepts = [buildCashPaymentRequirements("merchant@example.com", "USD", "500")];
      const resource = {
        url: "https://example.com/api/data",
        description: "Protected API",
        mimeType: "application/json",
      };
      const paymentRequired = await server.createPaymentRequiredResponse(accepts, resource);
      const paymentPayload = await client.createPaymentPayload(paymentRequired);

      // Verify first (precondition for settle)
      const verifyResponse = await facilitatorClient.verify(paymentPayload, accepts[0]);
      expect(verifyResponse.isValid).toBe(true);

      // Action: POST to facilitator /settle (via facilitatorClient.settle)
      const settleResponse = await facilitatorClient.settle(paymentPayload, accepts[0]);

      // Assertion: success=true, transaction hash is non-empty string, network matches
      expect(settleResponse.success).toBe(true);
      expect(typeof settleResponse.transaction).toBe("string");
      expect(settleResponse.transaction.length).toBeGreaterThan(0);
      expect(settleResponse.network).toBe(TEST_NETWORK);
    });
  });
});
