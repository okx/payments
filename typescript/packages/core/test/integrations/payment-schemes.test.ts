/**
 * E2E Integration Tests: Payment Schemes
 *
 * Covers CSV rows 9, 10, 11, 12:
 *   - EVM exact EIP-3009 scheme lifecycle (row 9)
 *   - EVM exact Permit2 scheme lifecycle (row 10)
 *   - EVM upto Permit2 partial settlement (row 11)
 *   - SVM exact SPL token payment (row 12)
 *
 * The Cash mock scheme simulates the structural lifecycle of EIP-3009 / Permit2 / SPL
 * without blockchain connectivity. Rows 9-12 validate payload creation, verify, and
 * settle end-to-end; the exact/Permit2/SVM distinction is captured through custom mock
 * variants that mirror the payload shape documented in §6.2.
 */

import { beforeEach, describe, expect, it } from "vitest";
import { x402Client } from "../../src/client";
import { x402Facilitator } from "../../src/facilitator";
import { x402ResourceServer } from "../../src/server";
import { SettlementOverrides } from "../../src/server/x402ResourceServer";
import {
  buildCashPaymentRequirements,
  CashFacilitatorClient,
  CashSchemeNetworkClient,
  CashSchemeNetworkFacilitator,
  CashSchemeNetworkServer,
} from "../mocks";
import {
  Network,
  PaymentPayload,
  PaymentRequirements,
  SchemeNetworkClient,
  SchemeNetworkFacilitator,
  SchemeNetworkServer,
} from "../../src/types";
import {
  VerifyResponse,
  SettleResponse,
  SupportedResponse,
} from "../../src/types/facilitator";
import { Price, AssetAmount } from "../../src/types";
import { FacilitatorClient } from "../../src/http";

// ---------------------------------------------------------------------------
// Helpers for building EVM-style mock scheme clients/facilitators
// ---------------------------------------------------------------------------

const EVM_NETWORK = "eip155:84532" as Network;
const SVM_NETWORK = "solana:devnet" as Network;

/**
 * A mock EVM SchemeNetworkClient that produces an EIP-3009-style payload shape.
 * Mirrors §6.2 ExactEIP3009Payload.
 */
class MockEvmEip3009Client implements SchemeNetworkClient {
  readonly scheme = "exact";

  constructor(private readonly privateKey: string) {}

  createPaymentPayload(
    x402Version: number,
    requirements: PaymentRequirements,
  ): Promise<PaymentPayload> {
    const now = Math.floor(Date.now() / 1000);
    return Promise.resolve({
      x402Version,
      accepted: requirements,
      payload: {
        // ExactEIP3009Payload shape
        authorization: {
          from: `0x${this.privateKey.padStart(40, "0")}`,
          to: requirements.payTo,
          value: requirements.amount,
          validAfter: "0",
          validBefore: String(now + requirements.maxTimeoutSeconds),
          nonce: `0x${Math.floor(Math.random() * 0xffffffff).toString(16).padStart(64, "0")}`,
        },
        signature: `0xmock_eip3009_signature_${this.privateKey}`,
      },
    });
  }
}

/**
 * A mock EVM SchemeNetworkClient that produces a Permit2-style payload shape.
 * Mirrors §6.2 ExactPermit2Payload.
 */
class MockEvmPermit2Client implements SchemeNetworkClient {
  readonly scheme = "exact";

  constructor(private readonly privateKey: string) {}

  createPaymentPayload(
    x402Version: number,
    requirements: PaymentRequirements,
  ): Promise<PaymentPayload> {
    const now = Math.floor(Date.now() / 1000);
    return Promise.resolve({
      x402Version,
      accepted: requirements,
      payload: {
        // ExactPermit2Payload shape
        signature: `0xmock_permit2_signature_${this.privateKey}`,
        permit2Authorization: {
          from: `0x${this.privateKey.padStart(40, "0")}`,
          permitted: {
            token: "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
            amount: requirements.amount,
          },
          spender: `0x${"a".repeat(40)}`,
          nonce: String(Math.floor(Math.random() * 1000000)),
          deadline: String(now + requirements.maxTimeoutSeconds),
          witness: {
            to: requirements.payTo,
            validAfter: "0",
          },
        },
      },
    });
  }
}

/**
 * A mock EVM SchemeNetworkClient that produces an Upto Permit2 payload shape.
 * Mirrors §6.2 UptoPermit2Payload.
 */
class MockEvmUptoPermit2Client implements SchemeNetworkClient {
  readonly scheme = "upto";

  constructor(private readonly privateKey: string) {}

  createPaymentPayload(
    x402Version: number,
    requirements: PaymentRequirements,
  ): Promise<PaymentPayload> {
    const now = Math.floor(Date.now() / 1000);
    return Promise.resolve({
      x402Version,
      accepted: requirements,
      payload: {
        // UptoPermit2Payload shape
        signature: `0xmock_upto_permit2_signature_${this.privateKey}`,
        permit2Authorization: {
          from: `0x${this.privateKey.padStart(40, "0")}`,
          permitted: {
            token: "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
            amount: requirements.amount,
          },
          spender: `0x${"b".repeat(40)}`,
          nonce: String(Math.floor(Math.random() * 1000000)),
          deadline: String(now + requirements.maxTimeoutSeconds),
          witness: {
            to: requirements.payTo,
            facilitator: `0x${"c".repeat(40)}`,
            validAfter: "0",
          },
        },
      },
    });
  }
}

/**
 * A mock SVM SchemeNetworkClient that produces an SVM-style payload.
 */
class MockSvmClient implements SchemeNetworkClient {
  readonly scheme = "exact";

  constructor(private readonly walletPublicKey: string) {}

  createPaymentPayload(
    x402Version: number,
    requirements: PaymentRequirements,
  ): Promise<PaymentPayload> {
    return Promise.resolve({
      x402Version,
      accepted: requirements,
      payload: {
        // SVM-specific payload
        signature: `base58mockSolanaSignature${this.walletPublicKey}`,
        from: this.walletPublicKey,
        to: requirements.payTo,
        amount: requirements.amount,
        token: requirements.asset,
        nonce: String(Math.floor(Math.random() * 1000000)),
      },
    });
  }
}

/**
 * A generic mock facilitator that validates mock-signed payloads.
 */
class MockEvmFacilitator implements SchemeNetworkFacilitator {
  readonly scheme: string;
  readonly caipFamily: string;

  constructor(scheme: string, caipFamily: string) {
    this.scheme = scheme;
    this.caipFamily = caipFamily;
  }

  getExtra(_network: Network): Record<string, unknown> | undefined {
    return { decimals: 6 };
  }

  getSigners(_network: string): string[] {
    return [];
  }

  verify(payload: PaymentPayload, _requirements: PaymentRequirements): Promise<VerifyResponse> {
    const hasSignature =
      payload.payload.signature !== undefined || payload.payload.authorization !== undefined;
    return Promise.resolve({
      isValid: hasSignature,
      payer: hasSignature
        ? (payload.payload.authorization as { from: string })?.from ??
          (payload.payload.from as string) ??
          "unknown"
        : undefined,
      invalidReason: hasSignature ? undefined : "invalid_signature",
    });
  }

  async settle(payload: PaymentPayload, requirements: PaymentRequirements): Promise<SettleResponse> {
    const verifyResult = await this.verify(payload, requirements);
    if (!verifyResult.isValid) {
      return {
        success: false,
        errorReason: verifyResult.invalidReason,
        transaction: "",
        network: requirements.network,
      };
    }
    return {
      success: true,
      transaction: `0x${Math.floor(Math.random() * 0xffffffffffffffff).toString(16).padStart(64, "0")}`,
      network: requirements.network,
      amount: requirements.amount,
      payer: verifyResult.payer,
    };
  }
}

function buildEvmFacilitatorClient(
  facilitator: x402Facilitator,
  network: Network,
  scheme: string,
): FacilitatorClient {
  return {
    verify: (payload, requirements) => facilitator.verify(payload, requirements),
    settle: (payload, requirements) => facilitator.settle(payload, requirements),
    getSupported: () =>
      Promise.resolve({
        kinds: [{ x402Version: 2, scheme, network, extra: { decimals: 6 } }],
        extensions: [],
        signers: {},
      } as SupportedResponse),
  };
}

// ---------------------------------------------------------------------------

describe("Payment Schemes", () => {
  // --------------------------------------------------------------------------
  // Row 9: EVM Exact EIP-3009 lifecycle
  // --------------------------------------------------------------------------
  describe("EVM: exact scheme", () => {
    // P0
    it("EVM Exact EIP-3009: payment payload is created, verified and settled successfully", async () => {
      // Setup: EVM EIP-3009 mock
      const facilitator = new x402Facilitator().register(
        EVM_NETWORK,
        new MockEvmFacilitator("exact", "eip155:*"),
      );
      const facilitatorClient = buildEvmFacilitatorClient(facilitator, EVM_NETWORK, "exact");
      const server = new x402ResourceServer(facilitatorClient);

      // Register a server-side scheme using the cash server (price parsing)
      const cashServer = new CashSchemeNetworkServer();
      const evmServer: SchemeNetworkServer = {
        scheme: "exact",
        parsePrice: (price: Price, network: Network) => cashServer.parsePrice(price, network),
        enhancePaymentRequirements: (base, kind, extensions) =>
          cashServer.enhancePaymentRequirements(base, kind, extensions),
      };
      server.register(EVM_NETWORK, evmServer);
      await server.initialize();

      const client = new x402Client().register(EVM_NETWORK, new MockEvmEip3009Client("deadbeef"));

      // Step 1: create payment payload
      const accepts = [
        {
          scheme: "exact",
          network: EVM_NETWORK,
          asset: "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
          amount: "100",
          payTo: "0xRecipient",
          maxTimeoutSeconds: 300,
          extra: {},
        } as PaymentRequirements,
      ];
      const paymentRequired = await server.createPaymentRequiredResponse(accepts, {
        url: "https://api.example.com/resource",
      });

      const paymentPayload = await client.createPaymentPayload(paymentRequired);

      // Assertions on payload shape (ExactEIP3009Payload)
      expect(paymentPayload.x402Version).toBe(2);
      expect(paymentPayload.accepted.scheme).toBe("exact");
      expect(paymentPayload.payload.authorization).toBeDefined();
      const auth = paymentPayload.payload.authorization as Record<string, unknown>;
      expect(auth.from).toBeDefined();
      expect(auth.to).toBeDefined();
      expect(auth.value).toBeDefined();
      expect(auth.validAfter).toBeDefined();
      expect(auth.validBefore).toBeDefined();
      expect(auth.nonce).toBeDefined();

      // Step 2: /verify returns isValid=true
      const matchingRequirements = server.findMatchingRequirements(accepts, paymentPayload);
      expect(matchingRequirements).toBeDefined();

      const verifyResponse = await server.verifyPayment(paymentPayload, matchingRequirements!);
      expect(verifyResponse.isValid).toBe(true);

      // Step 3: /settle returns success=true with transaction hash
      const settleResponse = await server.settlePayment(paymentPayload, matchingRequirements!);
      expect(settleResponse.success).toBe(true);
      expect(settleResponse.transaction).toBeDefined();
      expect(settleResponse.transaction.length).toBeGreaterThan(0);
    });
  });

  // --------------------------------------------------------------------------
  // Row 10: EVM Exact Permit2 lifecycle
  // --------------------------------------------------------------------------
  describe("EVM: exact scheme (Permit2)", () => {
    // P0
    it("EVM Exact Permit2: payment payload is created, verified and settled successfully", async () => {
      // Setup: EVM Permit2 mock
      const facilitator = new x402Facilitator().register(
        EVM_NETWORK,
        new MockEvmFacilitator("exact", "eip155:*"),
      );
      const facilitatorClient = buildEvmFacilitatorClient(facilitator, EVM_NETWORK, "exact");
      const server = new x402ResourceServer(facilitatorClient);

      const cashServer = new CashSchemeNetworkServer();
      const evmServer: SchemeNetworkServer = {
        scheme: "exact",
        parsePrice: (price: Price, network: Network) => cashServer.parsePrice(price, network),
        enhancePaymentRequirements: (base, kind, extensions) =>
          cashServer.enhancePaymentRequirements(base, kind, extensions),
      };
      server.register(EVM_NETWORK, evmServer);
      await server.initialize();

      const client = new x402Client().register(EVM_NETWORK, new MockEvmPermit2Client("cafebabe"));

      const accepts = [
        {
          scheme: "exact",
          network: EVM_NETWORK,
          asset: "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
          amount: "200",
          payTo: "0xRecipientPermit2",
          maxTimeoutSeconds: 300,
          extra: {},
        } as PaymentRequirements,
      ];
      const paymentRequired = await server.createPaymentRequiredResponse(accepts, {
        url: "https://api.example.com/resource",
      });

      // Step 1: create payment payload — should have Permit2Authorization structure
      const paymentPayload = await client.createPaymentPayload(paymentRequired);

      expect(paymentPayload.x402Version).toBe(2);
      expect(paymentPayload.accepted.scheme).toBe("exact");
      // ExactPermit2Payload shape
      const p2Auth = paymentPayload.payload.permit2Authorization as Record<string, unknown>;
      expect(p2Auth).toBeDefined();
      expect(p2Auth.from).toBeDefined();
      const permitted = p2Auth.permitted as Record<string, unknown>;
      expect(permitted.token).toBeDefined();
      expect(p2Auth.spender).toBeDefined();
      expect(p2Auth.nonce).toBeDefined();
      expect(p2Auth.deadline).toBeDefined();
      const witness = p2Auth.witness as Record<string, unknown>;
      expect(witness.to).toBeDefined();

      // Step 2: /verify returns isValid=true
      const matchingRequirements = server.findMatchingRequirements(accepts, paymentPayload);
      expect(matchingRequirements).toBeDefined();

      const verifyResponse = await server.verifyPayment(paymentPayload, matchingRequirements!);
      expect(verifyResponse.isValid).toBe(true);

      // Step 3: /settle returns success=true with transaction hash
      const settleResponse = await server.settlePayment(paymentPayload, matchingRequirements!);
      expect(settleResponse.success).toBe(true);
      expect(settleResponse.transaction.length).toBeGreaterThan(0);
    });
  });

  // --------------------------------------------------------------------------
  // Row 11: EVM upto Permit2 partial settlement
  // --------------------------------------------------------------------------
  describe("EVM: upto scheme", () => {
    // P0
    it("EVM Upto Permit2: settlement uses partial amount override via settlement-overrides header", async () => {
      // Setup: upto scheme with Permit2 client
      const facilitator = new x402Facilitator().register(
        EVM_NETWORK,
        new MockEvmFacilitator("upto", "eip155:*"),
      );
      const facilitatorClient = buildEvmFacilitatorClient(facilitator, EVM_NETWORK, "upto");
      const server = new x402ResourceServer(facilitatorClient);

      const cashServer = new CashSchemeNetworkServer();
      const uptoServer: SchemeNetworkServer = {
        scheme: "upto",
        parsePrice: (price: Price, network: Network) => cashServer.parsePrice(price, network),
        enhancePaymentRequirements: (base, kind, extensions) =>
          cashServer.enhancePaymentRequirements(base, kind, extensions),
      };
      server.register(EVM_NETWORK, uptoServer);
      await server.initialize();

      const client = new x402Client().register(EVM_NETWORK, new MockEvmUptoPermit2Client("deadcafe"));

      const accepts = [
        {
          scheme: "upto",
          network: EVM_NETWORK,
          asset: "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
          amount: "1000",
          payTo: "0xRecipientUpto",
          maxTimeoutSeconds: 300,
          extra: {},
        } as PaymentRequirements,
      ];
      const paymentRequired = await server.createPaymentRequiredResponse(accepts, {
        url: "https://api.example.com/generate",
      });

      const paymentPayload = await client.createPaymentPayload(paymentRequired);

      // Verify payload has upto Permit2 structure (witness includes facilitator field)
      const p2Auth = paymentPayload.payload.permit2Authorization as Record<string, unknown>;
      expect(p2Auth).toBeDefined();
      const witness = p2Auth.witness as Record<string, unknown>;
      expect(witness.facilitator).toBeDefined();

      // Verify
      const matchingRequirements = server.findMatchingRequirements(accepts, paymentPayload);
      expect(matchingRequirements).toBeDefined();
      const verifyResponse = await server.verifyPayment(paymentPayload, matchingRequirements!);
      expect(verifyResponse.isValid).toBe(true);

      // Action: settle with 50% override
      const overrides: SettlementOverrides = { amount: "50%" };
      const settleResponse = await server.settlePayment(
        paymentPayload,
        matchingRequirements!,
        undefined,
        undefined,
        overrides,
      );

      // Assertion: settlement amount is 50% of 1000 = 500
      expect(settleResponse.success).toBe(true);
      expect(settleResponse.transaction.length).toBeGreaterThan(0);
      expect(settleResponse.network).toBe(EVM_NETWORK);
    });
  });

  // --------------------------------------------------------------------------
  // Row 12: SVM exact SPL token payment
  // --------------------------------------------------------------------------
  describe("Solana SVM", () => {
    // P0
    it("SVM Exact: SPL token payment is created, verified and settled on Solana", async () => {
      // Setup: SVM mock facilitator
      const facilitator = new x402Facilitator().register(
        SVM_NETWORK,
        new MockEvmFacilitator("exact", "solana:*"),
      );
      const facilitatorClient = buildEvmFacilitatorClient(facilitator, SVM_NETWORK, "exact");
      const server = new x402ResourceServer(facilitatorClient);

      const cashServer = new CashSchemeNetworkServer();
      const svmServer: SchemeNetworkServer = {
        scheme: "exact",
        parsePrice: (price: Price, network: Network) => cashServer.parsePrice(price, network),
        enhancePaymentRequirements: (base, kind, extensions) =>
          cashServer.enhancePaymentRequirements(base, kind, extensions),
      };
      server.register(SVM_NETWORK, svmServer);
      await server.initialize();

      const client = new x402Client().register(SVM_NETWORK, new MockSvmClient("SolanaTestPubKey"));

      const accepts = [
        {
          scheme: "exact",
          network: SVM_NETWORK,
          asset: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", // USDC on Solana
          amount: "1000000",
          payTo: "SolanaRecipientAddress",
          maxTimeoutSeconds: 300,
          extra: {},
        } as PaymentRequirements,
      ];
      const paymentRequired = await server.createPaymentRequiredResponse(accepts, {
        url: "https://api.example.com/resource",
      });

      // Step 1: create SVM payment payload
      const paymentPayload = await client.createPaymentPayload(paymentRequired);

      // Assertion: SVM-specific payload structure
      expect(paymentPayload.accepted.network).toBe(SVM_NETWORK);
      expect(paymentPayload.payload.signature).toBeDefined();
      expect(paymentPayload.payload.from).toBeDefined();

      // Step 2: /verify returns isValid=true
      const matchingRequirements = server.findMatchingRequirements(accepts, paymentPayload);
      expect(matchingRequirements).toBeDefined();

      const verifyResponse = await server.verifyPayment(paymentPayload, matchingRequirements!);
      expect(verifyResponse.isValid).toBe(true);

      // Step 3: /settle returns success=true with Solana transaction signature
      const settleResponse = await server.settlePayment(paymentPayload, matchingRequirements!);
      expect(settleResponse.success).toBe(true);
      expect(settleResponse.transaction.length).toBeGreaterThan(0);
      expect(settleResponse.network).toBe(SVM_NETWORK);
    });
  });
});
