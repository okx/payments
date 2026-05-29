import { x402Facilitator } from "@okxweb3/x402-core/facilitator";
import { Network } from "@okxweb3/x402-core/types";
import { FacilitatorEvmSigner } from "../../signer";
import { ExactEvmScheme } from "./scheme";

/**
 * Configuration options for registering EVM schemes to an x402Facilitator
 */
export interface EvmFacilitatorConfig {
  /**
   * The EVM signer for facilitator operations (verify and settle)
   */
  signer: FacilitatorEvmSigner;

  /**
   * Networks to register (single network or array of networks)
   * Example: "eip155:196"
   */
  networks: Network | Network[];

  /**
   * If enabled, the facilitator will deploy ERC-4337 smart wallets
   * via EIP-6492 when encountering undeployed contract signatures.
   *
   * @default false
   */
  deployERC4337WithEIP6492?: boolean;

  /**
   * If enabled, reruns on-chain simulation during settle's re-verify.
   *
   * @default false
   */
  simulateInSettle?: boolean;
}

/**
 * Registers EVM exact payment schemes to an x402Facilitator instance.
 *
 * This function registers:
 * - Specified networks with ExactEvmScheme
 *
 * @param facilitator - The x402Facilitator instance to register schemes to
 * @param config - Configuration for EVM facilitator registration
 * @returns The facilitator instance for chaining
 *
 * @example
 * ```typescript
 * import { registerExactEvmScheme } from "@okxweb3/x402-evm/exact/facilitator/register";
 * import { x402Facilitator } from "@okxweb3/x402-core/facilitator";
 * import { createPublicClient, createWalletClient } from "viem";
 *
 * const facilitator = new x402Facilitator();
 *
 * registerExactEvmScheme(facilitator, {
 *   signer: combinedClient,
 *   networks: "eip155:196"  // XLayer mainnet
 * });
 * ```
 */
export function registerExactEvmScheme(
  facilitator: x402Facilitator,
  config: EvmFacilitatorConfig,
): x402Facilitator {
  // Register V2 scheme with specified networks
  facilitator.register(
    config.networks,
    new ExactEvmScheme(config.signer, {
      deployERC4337WithEIP6492: config.deployERC4337WithEIP6492,
      simulateInSettle: config.simulateInSettle,
    }),
  );

  return facilitator;
}
