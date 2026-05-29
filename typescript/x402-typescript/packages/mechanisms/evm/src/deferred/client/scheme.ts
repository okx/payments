import {
  SchemeNetworkClient,
  PaymentRequirements,
  PaymentPayloadResult,
  PaymentPayloadContext,
} from "@okxweb3/x402-core/types";
import { authorizationTypes } from "../../constants";
import { createNonce, getEvmChainId } from "../../utils";
import { getAddress } from "viem";

export interface SessionKeySigner {
  address: `0x${string}`;
  signTypedData(params: {
    domain: object;
    types: object;
    primaryType: string;
    message: object;
  }): Promise<`0x${string}`>;
}

/**
 * EVM client implementation for the aggr_deferred payment scheme.
 *
 * Uses a session key to sign EIP-3009 TransferWithAuthorization directly
 * (without signConvert/TEE). The OKX Facilitator receives the session key
 * signature and sessionCert, writes to DB, and batches on-chain later.
 * Settlement returns immediately with status=success.
 */
export class AggrDeferredEvmScheme implements SchemeNetworkClient {
  readonly scheme = "aggr_deferred";

  constructor(
    private readonly sessionKeySigner: SessionKeySigner,
    private readonly sessionCert: string,
  ) {}

  async createPaymentPayload(
    x402Version: number,
    paymentRequirements: PaymentRequirements,
    _context?: PaymentPayloadContext,
  ): Promise<PaymentPayloadResult> {
    const nonce = createNonce();
    const now = Math.floor(Date.now() / 1000);
    const chainId = getEvmChainId(paymentRequirements.network);

    const authorization = {
      from: this.sessionKeySigner.address,
      to: getAddress(paymentRequirements.payTo),
      value: paymentRequirements.amount,
      validAfter: (now - 5).toString(),
      validBefore: (now + paymentRequirements.maxTimeoutSeconds).toString(),
      nonce,
    };

    if (!paymentRequirements.extra?.name || !paymentRequirements.extra?.version) {
      throw new Error(
        "EIP-712 domain parameters (name, version) required in payment requirements",
      );
    }

    const domain = {
      name: paymentRequirements.extra.name,
      version: paymentRequirements.extra.version,
      chainId,
      verifyingContract: getAddress(paymentRequirements.asset),
    };

    const signature = await this.sessionKeySigner.signTypedData({
      domain,
      types: authorizationTypes,
      primaryType: "TransferWithAuthorization",
      message: {
        from: getAddress(authorization.from),
        to: getAddress(authorization.to),
        value: BigInt(authorization.value),
        validAfter: BigInt(authorization.validAfter),
        validBefore: BigInt(authorization.validBefore),
        nonce: authorization.nonce,
      },
    });

    // Per OKX API spec: sessionCert flows through paymentPayload.accepted.extra.sessionCert
    // (Buyer fills it, Seller passes through, Facilitator extracts for TEE verification).
    // acceptedExtraOverrides is merged into accepted.extra by x402Client.createPaymentPayload.
    return {
      x402Version,
      payload: { authorization, signature },
      extensions: {
        scheme: "aggr_deferred",
      },
      acceptedExtraOverrides: {
        sessionCert: this.sessionCert,
      },
    };
  }
}
