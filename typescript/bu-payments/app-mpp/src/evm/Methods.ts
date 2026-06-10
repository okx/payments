// ---------------------------------------------------------------------------
// EVM payment method schemas — aligned with draft-evm-charge-00
// ---------------------------------------------------------------------------
import { Method, z } from 'mppx'

// ---- Shared sub-schemas ----

const splitSchema = z.object({
  amount: z.string(),
  recipient: z.string(),
  memo: z.optional(z.string()),
})

const methodDetailsSchema = z.object({
  /** EVM chain ID (e.g. 196 for X Layer). */
  chainId: z.number(),
  /** true = server pays gas, client must use transaction mode. */
  feePayer: z.optional(z.boolean()),
  /** Uniswap Permit2 contract address. */
  permit2Address: z.optional(z.string()),
  /** Split payment recipients (max 10). */
  splits: z.optional(z.array(splitSchema)),
})

// ---- EIP-3009 authorization ----

const eip3009SplitAuthSchema = z.object({
  from: z.string(),
  to: z.string(),
  value: z.string(),
  validAfter: z.string(),
  validBefore: z.string(),
  nonce: z.string(),
  signature: z.string(),
})

const eip3009AuthorizationSchema = z.object({
  type: z.literal('eip-3009'),
  from: z.string(),
  to: z.string(),
  value: z.string(),
  validAfter: z.string(),
  validBefore: z.string(),
  nonce: z.string(),
  signature: z.string(),
  splits: z.optional(z.array(eip3009SplitAuthSchema)),
})

// ---- Permit2 authorization ----

const permit2AuthorizationSchema = z.object({
  type: z.literal('permit2'),
  permit: z.object({
    permitted: z.array(z.object({
      token: z.string(),
      amount: z.string(),
    })),
    nonce: z.string(),
    deadline: z.string(),
  }),
  transferDetails: z.array(z.object({
    to: z.string(),
    requestedAmount: z.string(),
  })),
  witness: z.object({
    challengeHash: z.string(),
  }),
  signature: z.string(),
})

// ---- ERC-7710 delegation authorization ----

const delegationAuthorizationSchema = z.object({
  type: z.literal('delegation'),
  delegationManager: z.string(),
  permissionContexts: z.array(z.string()),
  mode: z.string(),
})

// ---- Credential payload ----

const transactionPayloadSchema = z.object({
  type: z.literal('transaction'),
  authorization: z.union([
    eip3009AuthorizationSchema,
    permit2AuthorizationSchema,
    delegationAuthorizationSchema,
  ]),
})

const hashPayloadSchema = z.object({
  type: z.literal('hash'),
  hash: z.string(),
})

// ---- EVM Charge Method ----

/**
 * EVM charge method — shared schema used by both server and client.
 *
 * The challenge request carries payment parameters (amount, currency, recipient,
 * chain details). The credential payload is either:
 *   - `type: "transaction"` — off-chain authorization (EIP-3009 / Permit2 / delegation),
 *     server submits on-chain transfer
 *   - `type: "hash"` — client already broadcast the transaction, presents the tx hash
 *     for server to verify on-chain
 */
export const charge = Method.from({
  intent: 'charge',
  name: 'evm',
  schema: {
    credential: {
      payload: z.discriminatedUnion('type', [
        transactionPayloadSchema,
        hashPayloadSchema,
      ]),
    },
    request: z.object({
      /** Payment amount in base units (stringified integer). */
      amount: z.string(),
      /** ERC-20 token contract address (EIP-55 checksummed). */
      currency: z.string(),
      /** Recipient address (EIP-55). */
      recipient: z.string(),
      /** Human-readable payment description. */
      description: z.optional(z.string()),
      /** Merchant order reference, echoed in receipt. */
      externalId: z.optional(z.string()),
      /** EVM method-specific parameters. */
      methodDetails: methodDetailsSchema,
    }),
  },
})

// ---- Session method details ----

const sessionSplitSchema = z.object({
  recipient: z.string(),
  bps: z.number(),
  memo: z.optional(z.string()),
})

const sessionMethodDetailsSchema = z.object({
  chainId: z.number(),
  escrowContract: z.string(),
  channelId: z.optional(z.string()),
  minVoucherDelta: z.optional(z.string()),
  feePayer: z.optional(z.boolean()),
  splits: z.optional(z.array(sessionSplitSchema)),
})

// ---- Session credential payload (discriminated on action) ----

const depositMergeSchema = z.object({
  action: z.string(),
  authorization: z.object({
    type: z.string(),
    from: z.string(),
    to: z.string(),
    value: z.string(),
    validAfter: z.string(),
    validBefore: z.string(),
    nonce: z.string(),
  }),
  signature: z.string(),
  salt: z.optional(z.string()),
  authorizedSigner: z.optional(z.string()),
})

const sessionOpenPayloadSchema = z.object({
  action: z.literal('open'),
  type: z.string(),
  channelId: z.string(),
  salt: z.string(),
  cumulativeAmount: z.string(),
  authorization: z.optional(z.object({
    type: z.string(),
    from: z.string(),
    to: z.string(),
    value: z.string(),
    validAfter: z.string(),
    validBefore: z.string(),
    nonce: z.string(),
  })),
  signature: z.optional(z.string()),
  voucherSignature: z.optional(z.string()),
  hash: z.optional(z.string()),
  authorizedSigner: z.optional(z.string()),
})

const sessionVoucherPayloadSchema = z.object({
  action: z.literal('voucher'),
  channelId: z.string(),
  cumulativeAmount: z.string(),
  signature: z.string(),
  deposit: z.optional(depositMergeSchema),
})

const sessionTopUpPayloadSchema = z.object({
  action: z.literal('topUp'),
  type: z.string(),
  channelId: z.string(),
  additionalDeposit: z.string(),
  authorization: z.optional(z.object({
    type: z.string(),
    from: z.string(),
    to: z.string(),
    value: z.string(),
    validAfter: z.string(),
    validBefore: z.string(),
    nonce: z.string(),
  })),
  signature: z.optional(z.string()),
  hash: z.optional(z.string()),
})

const sessionClosePayloadSchema = z.object({
  action: z.literal('close'),
  channelId: z.string(),
  cumulativeAmount: z.string(),
  signature: z.string(),
})

// ---- EVM Session Method ----

/**
 * EVM session method — shared schema used by both server and client.
 *
 * Session enables pay-as-you-go streaming payments via on-chain Escrow + off-chain
 * EIP-712 vouchers. The credential payload is a discriminated union on `action`:
 *   - `open`    — create a payment channel (transaction or hash mode)
 *   - `voucher` — submit a cumulative voucher (high frequency, off-chain)
 *   - `topUp`   — add deposit to an existing channel
 *   - `close`   — close the channel with final settlement
 */
export const session = Method.from({
  intent: 'session',
  name: 'evm',
  schema: {
    credential: {
      payload: z.discriminatedUnion('action', [
        sessionOpenPayloadSchema,
        sessionVoucherPayloadSchema,
        sessionTopUpPayloadSchema,
        sessionClosePayloadSchema,
      ]),
    },
    request: z.object({
      /** Price per unit of service in base units. */
      amount: z.string(),
      /** ERC-20 token contract address. */
      currency: z.string(),
      /** Recipient / payee address. */
      recipient: z.string(),
      /** Human-readable payment description. */
      description: z.optional(z.string()),
      /** Merchant order reference. */
      externalId: z.optional(z.string()),
      /** Unit being priced (e.g., "llm_token", "byte", "request"). */
      unitType: z.optional(z.string()),
      /** Suggested deposit amount in base units. */
      suggestedDeposit: z.optional(z.string()),
      /** Session method-specific parameters. */
      methodDetails: sessionMethodDetailsSchema,
    }),
  },
})

// Re-export sub-schemas for external use
export {
  eip3009AuthorizationSchema,
  permit2AuthorizationSchema,
  delegationAuthorizationSchema,
  methodDetailsSchema,
  splitSchema,
  sessionMethodDetailsSchema,
  sessionSplitSchema,
}
