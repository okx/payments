// EVM payment method shared exports (used by both client and server).
// Server-side factories (charge / session) live under '@okxweb3/mpp/evm/server'.

// EIP-712 voucher utilities
export {
  buildSettleAuth,
  buildCloseAuth,
  verifyVoucher,
  randomU256,
  unixDeadline,
  DEFAULT_DOMAIN_NAME,
  DEFAULT_DOMAIN_VERSION,
} from './server/voucher.js'
export type {
  VerifyVoucherParams,
  AuthMessageParams,
} from './server/voucher.js'

// SA API client
export { SaApiClient } from './sa/SaApiClient.js'
export type { SaApiClientConfig, SaApiErrorInfo } from './sa/SaApiClient.js'
export { toChallengeEcho } from './sa/challenge.js'
export type {
  ChallengeEcho,
  Eip3009Authorization,
  SplitAuth,
  SaApiResponse,

  // Charge
  ChargeSettleRequest,
  ChargeVerifyHashRequest,
  ChargeReceipt,

  // Session
  SessionReceipt,
  SessionOpenRequest,
  SessionOpenTransactionPayload,
  SessionOpenHashPayload,
  SessionOpenReceipt,
  SessionTopUpRequest,
  SessionTopUpTransactionPayload,
  SessionTopUpHashPayload,
  SessionTopUpReceipt,
  SessionSettleRequest,
  SessionSettleReceipt,
  SessionCloseRequest,
  SessionCloseReceipt,
  ChannelStatus,

  SaErrorCode,
} from './sa/types.js'
export { SA_ERROR_CODES } from './sa/types.js'

// Method schemas (Method.from outputs, shared by client and server)
export { charge as chargeSchema, session as sessionSchema } from './Methods.js'
