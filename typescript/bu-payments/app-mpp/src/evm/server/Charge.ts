import { Errors, Method, Receipt } from 'mppx'
import * as Methods from '../Methods.js'
import { toChallengeEcho } from '../sa/challenge.js'
import type { SaApiClient } from '../sa/SaApiClient.js'
import type {
  ChallengeEcho,
  ChargeVerifyHashRequest,
} from '../sa/types.js'

/**
 * Creates an EVM `charge` method for usage on the server.
 *
 * Uses SA API for settlement and verification. The verify flow dispatches
 * based on `payload.type`:
 *   - `"transaction"` → POST /charge/settle (server broadcasts on-chain transfer)
 *   - `"hash"`        → POST /charge/verifyHash (verify client-broadcast tx)
 *
 * @example
 * ```ts
 * import { Mppx } from 'mppx/server'
 * import { evm } from '@mpp/sdk/server'
 * import { SaApiClient } from '@mpp/sdk/sa'
 *
 * const saClient = new SaApiClient({
 *   apiKey: process.env.OKX_API_KEY!,
 *   secretKey: process.env.OKX_SECRET_KEY!,
 *   passphrase: process.env.OKX_PASSPHRASE!,
 * })
 *
 * const mppx = Mppx.create({
 *   methods: [evm.charge({ saClient })],
 * })
 *
 * export async function handler(request: Request) {
 *   const result = await mppx.charge({
 *     amount: '1000000',
 *     currency: '0xA8CE8aee21bC2A48a5EF670afCc9274C7bbbC035',
 *     recipient: '0x742d35Cc6634c0532925a3b844bC9e7595F8fE00',
 *     methodDetails: { chainId: 196, feePayer: true },
 *   })(request)
 *   if (result.status === 402) return result.challenge
 *   return result.withReceipt(Response.json({ data: '...' }))
 * }
 * ```
 */
export function charge(parameters: charge.Parameters) {
  const { saClient } = parameters

  return Method.toServer(Methods.charge, {
    defaults: {
      methodDetails: {
        chainId: 196,
      },
    },

    async request({ request }) {
      return request
    },

    async verify({ credential, request }) {
      const { payload } = credential
      const source = credential.source
      const challengeEcho = toChallengeEcho(
        credential.challenge as Record<string, unknown>,
        request as Record<string, unknown>,
      )

      if (payload.type === 'transaction') {
        return verifyTransaction(saClient, challengeEcho, payload, source)
      }

      if (payload.type === 'hash') {
        return verifyHash(saClient, challengeEcho, payload, source)
      }

      throw new Errors.InvalidPayloadError({ reason: 'unsupported payload type' })
    },
  })
}

export declare namespace charge {
  type Parameters = {
    /** SA API client instance (pre-configured with OKX credentials). */
    saClient: SaApiClient
  }
}

async function verifyTransaction(
  saClient: SaApiClient,
  challengeEcho: ChallengeEcho,
  payload: TransactionPayload,
  source: string | undefined,
): Promise<Receipt.Receipt> {
  const { authorization } = payload

  const settleRequest = {
    challenge: challengeEcho,
    payload: {
      type: 'transaction' as const,
      authorization,
    },
    source,
  }

  const receipt = await saClient.chargeSettle(settleRequest as any)

  return receipt as unknown as Receipt.Receipt
}

async function verifyHash(
  saClient: SaApiClient,
  challengeEcho: ChallengeEcho,
  payload: HashPayload,
  source: string | undefined,
): Promise<Receipt.Receipt> {
  if (!source) {
    throw new Errors.VerificationFailedError({ reason: 'source (payer DID) is required for hash mode credentials' })
  }

  const verifyRequest: ChargeVerifyHashRequest = {
    challenge: challengeEcho,
    payload: {
      type: 'hash',
      hash: payload.hash,
    },
    source,
  }

  const receipt = await saClient.chargeVerifyHash(verifyRequest)

  return receipt as unknown as Receipt.Receipt
}

/** Transaction-type payload — authorization is forwarded to SA API as-is. */
type TransactionPayload = {
  type: 'transaction'
  authorization: Record<string, unknown>
}

/** Hash-type payload. */
type HashPayload = {
  type: 'hash'
  hash: string
}

