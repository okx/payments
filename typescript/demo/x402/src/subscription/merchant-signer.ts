/**
 * Merchant-initiated cancel signing helper. Produces a `CancelAuth` whose
 * `initiator=1 (merchant)`, signed by `MERCHANT_PRIVATE_KEY`. Used by the
 * admin "cancel" button per row.
 */
import type { Hex } from "viem";

import {
  buildCancelAuthTypedData,
  type CancelAuth,
} from "@okxweb3/app-x402-core/subscription";
import type { PermitSubscriptionScheme } from "@okxweb3/app-x402-evm/subscription";

import { merchantAccount } from "./shared";

/* eslint-disable @typescript-eslint/no-explicit-any */

/**
 * Build a merchant-signed CancelAuthorization (EIP-712, `by=MERCHANT`). The
 * deadline is set to "now + 5min", nonce is a random 32-byte hex.
 */
export async function signMerchantCancelAuth(
  subId: string,
  scheme: PermitSubscriptionScheme,
): Promise<CancelAuth> {
  const deadline = Math.floor(Date.now() / 1000) + 300;
  const nonce =
    "0x" +
    Array.from({ length: 32 }, () =>
      Math.floor(Math.random() * 256)
        .toString(16)
        .padStart(2, "0"),
    ).join("");
  const td = buildCancelAuthTypedData({
    subId: subId as Hex,
    initiator: "merchant",
    deadline,
    nonce: nonce as Hex,
    domain: scheme.getDomain(),
  });
  if (!merchantAccount) {
    throw new Error("MERCHANT_PRIVATE_KEY not set; cannot sign CancelAuth");
  }
  const signature = (await merchantAccount.signTypedData({
    domain: td.domain,
    types: td.types as any,
    primaryType: td.primaryType,
    message: td.message as any,
  })) as Hex;
  return {
    action: 0,
    subId,
    initiator: 1,
    nonce,
    deadline,
    signature,
  };
}
