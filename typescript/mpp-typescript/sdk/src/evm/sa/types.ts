// SA API type definitions — aligned with the [Pay] MPP EVM API spec.

/** Challenge object echoed back in every credential request. */
export interface ChallengeEcho {
  id: string
  realm: string
  method: string   // "evm"
  intent: string   // "charge" | "session"
  request: string  // base64url-encoded request params
  expires: string  // ISO 8601
}

/** EIP-3009 authorization object (shared by Charge and Session). */
export interface Eip3009Authorization {
  type: string          // "eip-3009" | "delegation"
  from: string
  to: string
  value: string
  validAfter: string    // unix timestamp string
  validBefore: string   // unix timestamp string
  nonce: string         // random bytes32
  signature: string     // 65-byte EIP-712 signature (r||s||v)
}

/** Split authorization (Charge only, max 10 per request). */
export interface SplitAuth extends Eip3009Authorization {
  // Each split carries its own to/value/nonce/signature; from/validAfter/
  // validBefore are inherited from Eip3009Authorization.
}

export interface SaApiResponse<T> {
  code: number   // 0 = SUCCESS
  data: T
  msg?: string
}

/** POST /charge/settle — transaction mode credential. */
export interface ChargeSettleRequest {
  challenge: ChallengeEcho
  payload: {
    type: 'transaction'
    authorization: Eip3009Authorization & {
      splits?: SplitAuth[]
    }
  }
  source?: string  // DID, e.g. "did:pkh:eip155:196:0x..."
}

/** POST /charge/verifyHash — hash mode credential. */
export interface ChargeVerifyHashRequest {
  challenge: ChallengeEcho
  payload: {
    type: 'hash'
    hash: string   // on-chain tx hash
  }
  source: string   // DID, required for hash mode
}

/** Receipt returned by charge/settle and charge/verifyHash. */
export interface ChargeReceipt {
  method: string        // "evm"
  reference: string     // tx hash (0x-prefixed)
  status: string        // "success"
  timestamp: string     // RFC 3339
  chainId: number
  challengeId?: string
  externalId?: string
}

/**
 * Receipt shape shared by all session/* endpoints (per spec).
 * Fields: method, intent, status, timestamp, channelId, chainId, reference?, deposit.
 */
export interface SessionReceipt {
  method: string             // "evm"
  intent: string             // "session"
  status: string             // "success"
  timestamp: string          // RFC 3339
  channelId: string          // bytes32 hex
  chainId: number
  reference?: string         // tx hash (transaction mode)
  /** Current on-chain escrow deposit; used for local voucher capacity checks. */
  deposit: string
}

export type SessionOpenReceipt = SessionReceipt
export type SessionTopUpReceipt = SessionReceipt
export type SessionSettleReceipt = SessionReceipt
export type SessionCloseReceipt = SessionReceipt

/** POST /session/open — open a payment channel. */
export interface SessionOpenRequest {
  challenge: ChallengeEcho
  payload: SessionOpenTransactionPayload | SessionOpenHashPayload
  source?: string
}

export interface SessionOpenTransactionPayload {
  action: 'open'
  type: 'transaction'
  channelId: string
  authorization: Eip3009Authorization
  signature: string                 // EIP-3009 signature (65 bytes)
  cumulativeAmount?: string         // Optional initial voucher amount.
  voucherSignature?: string         // Optional initial Voucher EIP-712 signature.
  salt: string                      // Random bytes32 used in channelId derivation.
  authorizedSigner?: string         // Delegated signer address (0x0 disables AA).
}

export interface SessionOpenHashPayload {
  action: 'open'
  type: 'hash'
  channelId: string
  hash: string                      // On-chain open transaction hash.
  cumulativeAmount?: string         // Optional initial voucher amount.
  signature?: string                // Optional initial Voucher EIP-712 signature (hash mode skips EIP-3009).
  salt: string
  authorizedSigner?: string
}

/** POST /session/topUp — add deposit to a channel. */
export interface SessionTopUpRequest {
  challenge: ChallengeEcho
  payload: SessionTopUpTransactionPayload | SessionTopUpHashPayload
  source?: string
}

export interface SessionTopUpTransactionPayload {
  action: 'topUp'
  type: 'transaction'
  channelId: string
  authorization: Eip3009Authorization
  signature: string          // EIP-3009 signature
  additionalDeposit: string  // Additional deposit in base units.
  topUpSalt?: string
}

export interface SessionTopUpHashPayload {
  action: 'topUp'
  type: 'hash'
  channelId: string
  hash: string
  additionalDeposit: string
  topUpSalt?: string
}

/**
 * POST /session/settle — relayer-submitted on-chain settlement.
 *
 * The merchant signs the EIP-712 `SettleAuthorization` off-chain:
 *   SettleAuthorization(bytes32 channelId, uint128 cumulativeAmount, uint256 nonce, uint256 deadline)
 * The voucher signature (signed by payer or authorizedSigner) is uploaded
 * with the request; the server does not pull vouchers from a database.
 */
export interface SessionSettleRequest {
  challenge: ChallengeEcho
  payload: {
    action?: 'settle'
    channelId: string
    /** Cumulative amount to settle (equals the highest accepted voucher amount). */
    cumulativeAmount: string
    /** EIP-712 Voucher signature (65 bytes); signer = authorizedSigner ?? payer. */
    voucherSignature: string
    /** EIP-712 SettleAuthorization signature (65 bytes); signer = payee. */
    payeeSignature: string
    /** uint256 decimal string. `(payee, channelId, nonce)` must be unique on chain. */
    nonce: string
    /** Signature expiry (unix seconds); server rejects if current time > deadline. */
    deadline: string
  }
}

/**
 * POST /session/close — close the channel with final settlement.
 *
 * Uses `max(client amount, server-held highest voucher)` to protect merchant
 * revenue. Remaining deposit is refunded to the payer by the escrow contract.
 * The payee must sign the EIP-712 `CloseAuthorization`.
 */
export interface SessionCloseRequest {
  challenge: ChallengeEcho
  payload: {
    action?: 'close'
    channelId: string
    /** Final cumulative amount (uint128 decimal string). */
    cumulativeAmount: string
    /** EIP-712 Voucher signature; required unless waiver branch (cumulativeAmount <= settledOnChain). */
    voucherSignature: string
    /** EIP-712 CloseAuthorization signature; signer = payee. */
    payeeSignature: string
    /** uint256 decimal random nonce; the `(payee, channelId, nonce)` used-set is shared with SettleAuthorization. */
    nonce: string
    /** Signature expiry (unix seconds). */
    deadline: string
  }
}

/** GET /session/status — read-only channel state. */
export interface ChannelStatus {
  channelId: string
  payer: string
  payee: string
  token: string
  deposit: string
  /** On-chain settled amount (only updated by `settle`). */
  settledOnChain: string
  sessionStatus: 'OPEN' | 'CLOSING' | 'CLOSED'
  /** Remaining available balance = `deposit - cumulativeAmount`. */
  remainingBalance: string
}

// Error codes (70000–70014 + 8000).

export const SA_ERROR_CODES = {
  8000: 'SERVICE_ERROR',
  70000: 'invalid_params',
  70001: 'unsupported_chain',
  70002: 'payer_blocked',
  70003: 'invalid_credential',
  70004: 'invalid_signature',
  70005: 'split_sum_exceeds_total',
  70006: 'split_count_exceeded',
  70007: 'tx_not_confirmed',
  70008: 'channel_close',
  70009: 'challenge_invalid',
  70010: 'channel_not_found',
  70011: 'grace_period_too_short',
  70012: 'amount_exceeds_deposit',
  70013: 'voucher_delta_too_small',
  70014: 'channel_closing',
} as const satisfies Record<number, string>

export type SaErrorCode = keyof typeof SA_ERROR_CODES
