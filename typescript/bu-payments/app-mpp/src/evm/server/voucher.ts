/**
 * EIP-712 structures for the MPP Session method (voucher verification and
 * settle/close authorization construction).
 *
 * Shared domain:
 *   EIP712Domain(string name, string version, uint256 chainId, address verifyingContract)
 *   name="EVM Payment Channel", version="1",
 *   chainId=session.chainId, verifyingContract=escrowContract
 *
 * Primary types:
 *   Voucher(bytes32 channelId, uint128 cumulativeAmount)
 *   SettleAuthorization(bytes32 channelId, uint128 cumulativeAmount, uint256 nonce, uint256 deadline)
 *   CloseAuthorization(bytes32 channelId, uint128 cumulativeAmount, uint256 nonce, uint256 deadline)
 *
 * Vouchers are signed by the payer (or `authorizedSigner`) and verified
 * locally. `SettleAuthorization` / `CloseAuthorization` are signed by the
 * payee and submitted to SA API for on-chain settlement.
 */

import type { Hex, TypedDataDefinition } from 'viem'
import { verifyTypedData } from 'viem'

/** Default EIP-712 domain values; must match the deployed escrow contract. */
export const DEFAULT_DOMAIN_NAME = 'EVM Payment Channel'
export const DEFAULT_DOMAIN_VERSION = '1'

const VOUCHER_TYPES = {
  Voucher: [
    { name: 'channelId', type: 'bytes32' },
    { name: 'cumulativeAmount', type: 'uint128' },
  ],
} as const

const SETTLE_AUTH_TYPES = {
  SettleAuthorization: [
    { name: 'channelId', type: 'bytes32' },
    { name: 'cumulativeAmount', type: 'uint128' },
    { name: 'nonce', type: 'uint256' },
    { name: 'deadline', type: 'uint256' },
  ],
} as const

const CLOSE_AUTH_TYPES = {
  CloseAuthorization: [
    { name: 'channelId', type: 'bytes32' },
    { name: 'cumulativeAmount', type: 'uint128' },
    { name: 'nonce', type: 'uint256' },
    { name: 'deadline', type: 'uint256' },
  ],
} as const

/** Generate a cryptographically secure 256-bit random nonce as a decimal string. */
export function randomU256(): string {
  const bytes = new Uint8Array(32)
  crypto.getRandomValues(bytes)
  let n = 0n
  for (const b of bytes) n = (n << 8n) | BigInt(b)
  return n.toString()
}

/**
 * Generate a unix-seconds deadline for settle / close authorizations.
 * Defaults to one hour from now.
 */
export function unixDeadline(secondsFromNow = 3600): string {
  return Math.floor(Date.now() / 1000 + secondsFromNow).toString()
}

export interface VerifyVoucherParams {
  chainId: number
  escrowContract: Hex
  channelId: Hex
  cumulativeAmount: string | bigint
  signature: Hex
  expectedSigner: Hex
  /** EIP-712 `domain.name`. Defaults to `"EVM Payment Channel"`. */
  domainName?: string
  /** EIP-712 `domain.version`. Defaults to `"1"`. */
  domainVersion?: string
}

/** Verify that a voucher signature was produced by `expectedSigner`. */
export async function verifyVoucher(params: VerifyVoucherParams): Promise<boolean> {
  return verifyTypedData({
    domain: {
      name: params.domainName ?? DEFAULT_DOMAIN_NAME,
      version: params.domainVersion ?? DEFAULT_DOMAIN_VERSION,
      chainId: params.chainId,
      verifyingContract: params.escrowContract,
    },
    types: VOUCHER_TYPES,
    primaryType: 'Voucher' as const,
    message: {
      channelId: params.channelId,
      cumulativeAmount: BigInt(params.cumulativeAmount),
    },
    address: params.expectedSigner,
    signature: params.signature,
  })
}

export interface AuthMessageParams {
  chainId: number
  escrowContract: Hex
  channelId: Hex
  cumulativeAmount: string | bigint
  nonce: string | bigint
  deadline: string | bigint
  /** EIP-712 `domain.name`. Defaults to `"EVM Payment Channel"`. */
  domainName?: string
  /** EIP-712 `domain.version`. Defaults to `"1"`. */
  domainVersion?: string
}

/**
 * Build the TypedData for a `SettleAuthorization`. Pass the returned object to
 * `signer.signTypedData(...)` to obtain the signature.
 */
export function buildSettleAuth(p: AuthMessageParams): TypedDataDefinition<typeof SETTLE_AUTH_TYPES, 'SettleAuthorization'> {
  return {
    domain: {
      name: p.domainName ?? DEFAULT_DOMAIN_NAME,
      version: p.domainVersion ?? DEFAULT_DOMAIN_VERSION,
      chainId: p.chainId,
      verifyingContract: p.escrowContract,
    },
    types: SETTLE_AUTH_TYPES,
    primaryType: 'SettleAuthorization',
    message: {
      channelId: p.channelId,
      cumulativeAmount: BigInt(p.cumulativeAmount),
      nonce: BigInt(p.nonce),
      deadline: BigInt(p.deadline),
    },
  }
}

/**
 * Build the TypedData for a `CloseAuthorization`. Pass the returned object to
 * `signer.signTypedData(...)` to obtain the signature.
 */
export function buildCloseAuth(p: AuthMessageParams): TypedDataDefinition<typeof CLOSE_AUTH_TYPES, 'CloseAuthorization'> {
  return {
    domain: {
      name: p.domainName ?? DEFAULT_DOMAIN_NAME,
      version: p.domainVersion ?? DEFAULT_DOMAIN_VERSION,
      chainId: p.chainId,
      verifyingContract: p.escrowContract,
    },
    types: CLOSE_AUTH_TYPES,
    primaryType: 'CloseAuthorization',
    message: {
      channelId: p.channelId,
      cumulativeAmount: BigInt(p.cumulativeAmount),
      nonce: BigInt(p.nonce),
      deadline: BigInt(p.deadline),
    },
  }
}
