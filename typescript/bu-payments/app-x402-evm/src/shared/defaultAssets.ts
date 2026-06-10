import type { Network } from "@okxweb3/app-x402-core/types";

/**
 * Base stablecoin asset configuration shared across all EVM payment schemes.
 * Contains the core fields needed to identify and convert tokens.
 */
export type DefaultAssetInfo = {
  /** Token contract address */
  address: string;
  /** EIP-712 domain name (must match the token's domain separator) */
  name: string;
  /** EIP-712 domain version (must match the token's domain separator) */
  version: string;
  /** Token decimal places (typically 6 for USDC) */
  decimals: number;
};

/**
 * Extended asset configuration for the exact scheme.
 * Includes transfer method hints that control client-side behaviour.
 */
export type ExactDefaultAssetInfo = DefaultAssetInfo & {
  /**
   * Transfer method override: `"permit2"` for tokens that don't support EIP-3009.
   * Omit for EIP-3009 tokens (default behaviour).
   */
  assetTransferMethod?: string;
  /**
   * Set to `true` for permit2 tokens that implement EIP-2612 `permit()`.
   * Controls whether name/version are included in `extra` so the client can
   * sign a gasless EIP-2612 permit for Permit2 approval.
   */
  supportsEip2612?: boolean;
};

/**
 * Default stablecoins indexed by CAIP-2 network identifier.
 *
 * Each network has the right to determine its own default stablecoin that can
 * be expressed as a USD string by calling servers. See DEFAULT_ASSET.md in
 * exact/server/ for how to add new chains.
 */
export const DEFAULT_STABLECOINS: Record<string, ExactDefaultAssetInfo> = {
  "eip155:196": {
    address: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
    name: "USD₮0",
    version: "1",
    decimals: 6,
  }, // X Layer mainnet USDT0 (EIP-3009)
  "eip155:1952": {
    address: "0x9e29b3aada05bf2d2c827af80bd28dc0b9b4fb0c",
    name: "USD₮0",
    version: "1",
    decimals: 6,
  },
};

/**
 * Look up the default stablecoin for a network.
 *
 * @param network - CAIP-2 network identifier (e.g. "eip155:196")
 * @returns The default asset info
 * @throws If no default asset is configured for the network
 */
export function getDefaultAsset(network: Network): ExactDefaultAssetInfo {
  const info = DEFAULT_STABLECOINS[network];
  if (!info) {
    throw new Error(`No default asset configured for network ${network}`);
  }
  return info;
}
