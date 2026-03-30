/**
 * E2E Integration Tests: Lifecycle Hooks
 *
 * Covers CSV rows 17, 18, 19:
 *   - beforeVerify Abort: hook returning { abort: true } prevents verification (row 17)
 *   - afterVerify: hook is called after successful verification with VerifyResponse (row 18)
 *   - onSettleFailure Hook Recovery: hook can recover from settlement error and prevent 500 (row 19)
 */

import { beforeEach, describe, expect, it } from "vitest";
import { x402Client } from "../../src/client";
import { x402Facilitator } from "../../src/facilitator";
import { x402ResourceServer } from "../../src/server";
import {
  VerifyContext,
  VerifyResultContext,
  SettleFailureContext,
} from "../../src/server/x402ResourceServer";
import {
  buildCashPaymentRequirements,
  CashFacilitatorClient,
  CashSchemeNetworkClient,
  CashSchemeNetworkFacilitator,
  CashSchemeNetworkServer,
} from "../mocks";
import { FacilitatorClient } from "../../src/http";
import { VerifyResponse, SettleResponse, SupportedResponse } from "../../src/types/facilitator";
import { Network, SettleError } from "../../src/types";

const TEST_NETWORK = "x402:cash" as Network;

async function buildTestServer(
  facilitatorClient?: FacilitatorClient,
): Promise<{ server: x402ResourceServer; client: x402Client }> {
  const facilitator = new x402Facilitator().register(
    TEST_NETWORK,
    new CashSchemeNetworkFacilitator(),
  );
  const fc = facilitatorClient ?? new CashFacilitatorClient(facilitator);
  const server = new x402ResourceServer(fc);
  server.register(TEST_NETWORK, new CashSchemeNetworkServer());
  await server.initialize();

  const client = new x402Client().register(TEST_NETWORK, new CashSchemeNetworkClient("HookPayer"));
  return { server, client };
}

async function buildPaymentPayloadForServer(
  server: x402ResourceServer,
  client: x402Client,
): Promise<{
  paymentPayload: ReturnType<typeof client.createPaymentPayload> extends Promise<infer T>
    ? T
    : never;
  accepts: ReturnType<typeof buildCashPaymentRequirements>[];
}> {
  const accepts = [buildCashPaymentRequirements("merchant@example.com", "USD", "100")];
  const paymentRequired = await server.createPaymentRequiredResponse(accepts, {
    url: "http://localhost:3000/resource",
  });
  const paymentPayload = await client.createPaymentPayload(paymentRequired);
  return { paymentPayload: paymentPayload as Awaited<ReturnType<typeof client.createPaymentPayload>>, accepts };
}

describe("Lifecycle Hooks", () => {
  // --------------------------------------------------------------------------
  // Row 17: beforeVerify Abort
  // --------------------------------------------------------------------------
  describe("beforeVerify Hook", () => {
    // P1
    it("beforeVerify Abort: hook can block verification and return 402", async () => {
      // Setup: server with beforeVerify hook that aborts
      const { server, client } = await buildTestServer();

      let facilitatorVerifyCalled = false;
      // Wrap the facilitator to track calls
      const originalFacilitator = new x402Facilitator().register(
        TEST_NETWORK,
        new CashSchemeNetworkFacilitator(),
      );
      const wrappedFacilitatorClient: FacilitatorClient = {
        verify: async (payload, requirements) => {
          facilitatorVerifyCalled = true;
          return originalFacilitator.verify(payload, requirements);
        },
        settle: (payload, requirements) => originalFacilitator.settle(payload, requirements),
        getSupported: () =>
          Promise.resolve({
            kinds: [{ x402Version: 2, scheme: "cash", network: TEST_NETWORK }],
            extensions: [],
            signers: {},
          } as SupportedResponse),
      };

      const serverWithHook = new x402ResourceServer(wrappedFacilitatorClient);
      serverWithHook.register(TEST_NETWORK, new CashSchemeNetworkServer());

      // Register beforeVerify hook that aborts
      serverWithHook.onBeforeVerify(async (_context: VerifyContext) => {
        return { abort: true as const, reason: "blocked", message: "Access blocked by policy" };
      });

      await serverWithHook.initialize();

      const { paymentPayload, accepts } = await buildPaymentPayloadForServer(server, client);
      const matched = server.findMatchingRequirements(accepts, paymentPayload)!;

      // Action: verify with the hook-equipped server
      const verifyResult = await serverWithHook.verifyPayment(paymentPayload, matched);

      // Assertion: isValid=false, facilitator /verify is never called
      expect(verifyResult.isValid).toBe(false);
      expect(facilitatorVerifyCalled).toBe(false);
      // The abort reason is reflected in invalidReason
      expect(verifyResult.invalidReason).toBe("blocked");
    });
  });

  // --------------------------------------------------------------------------
  // Row 18: afterVerify Hook
  // --------------------------------------------------------------------------
  describe("afterVerify Hook", () => {
    // P1
    it("afterVerify: hook is called after successful verification with VerifyResponse", async () => {
      // Setup: server with afterVerify hook that records payer
      const { server, client } = await buildTestServer();

      let capturedPayer: string | undefined;
      let afterVerifyCallCount = 0;

      const facilitator = new x402Facilitator().register(
        TEST_NETWORK,
        new CashSchemeNetworkFacilitator(),
      );
      const facilitatorClient = new CashFacilitatorClient(facilitator);
      const serverWithHook = new x402ResourceServer(facilitatorClient);
      serverWithHook.register(TEST_NETWORK, new CashSchemeNetworkServer());

      // Register afterVerify hook that saves payer
      serverWithHook.onAfterVerify(async (context: VerifyResultContext) => {
        afterVerifyCallCount++;
        capturedPayer = context.result.payer;
      });

      await serverWithHook.initialize();

      const { paymentPayload, accepts } = await buildPaymentPayloadForServer(server, client);
      const matched = server.findMatchingRequirements(accepts, paymentPayload)!;

      // Action: verify with the hook-equipped server
      const verifyResult = await serverWithHook.verifyPayment(paymentPayload, matched);

      // Assertion: afterVerify hook is called once with VerifyResponse
      expect(verifyResult.isValid).toBe(true);
      expect(afterVerifyCallCount).toBe(1);

      // Hook receives VerifyResponse with payer field
      expect(capturedPayer).toBeDefined();
      // The cash scheme payer is the signature (~HookPayer)
      expect(typeof capturedPayer).toBe("string");
    });
  });

  // --------------------------------------------------------------------------
  // Row 19: onSettleFailure Hook Recovery
  // --------------------------------------------------------------------------
  describe("onSettleFailure Hook Recovery", () => {
    // P1
    it("onSettleFailure: hook can recover from settlement error and prevent 500", async () => {
      // Setup: mock facilitator that throws SettleError; recovery hook returning { recovered: true }
      const settleErrorFacilitatorClient: FacilitatorClient = {
        verify: (_payload, _requirements) =>
          Promise.resolve({ isValid: true, payer: "~HookPayer" } as VerifyResponse),
        settle: (_payload, requirements) => {
          throw new SettleError(500, {
            success: false,
            errorReason: "blockchain_error",
            errorMessage: "Transaction failed",
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

      const serverWithRecovery = new x402ResourceServer(settleErrorFacilitatorClient);
      serverWithRecovery.register(TEST_NETWORK, new CashSchemeNetworkServer());

      // Mock settle response for recovery
      const mockSettleResponse: SettleResponse = {
        success: true,
        transaction: "0xRecoveryTx",
        network: TEST_NETWORK,
        payer: "~HookPayer",
      };

      let recoveryHookCalled = false;

      // Register recovery hook
      serverWithRecovery.onSettleFailure(async (_context: SettleFailureContext) => {
        recoveryHookCalled = true;
        return { recovered: true as const, result: mockSettleResponse };
      });

      await serverWithRecovery.initialize();

      // Build a client and payment payload using a clean server
      const { server: cleanServer, client } = await buildTestServer();
      const { paymentPayload, accepts } = await buildPaymentPayloadForServer(cleanServer, client);
      const matched = cleanServer.findMatchingRequirements(accepts, paymentPayload)!;

      // Action: settle triggers SettleError, recovery hook returns { recovered: true, result }
      const settleResult = await serverWithRecovery.settlePayment(paymentPayload, matched);

      // Assertion: response status is 200 equivalent (success=true from recovered result)
      expect(settleResult.success).toBe(true);
      expect(settleResult.transaction).toBe("0xRecoveryTx");

      // Recovery hook was called
      expect(recoveryHookCalled).toBe(true);
    });
  });
});
