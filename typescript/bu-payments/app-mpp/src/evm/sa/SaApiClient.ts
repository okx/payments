import { Errors } from 'mppx'
import type {
  SaApiResponse,
  ChargeSettleRequest,
  ChargeVerifyHashRequest,
  ChargeReceipt,
  SessionOpenRequest,
  SessionOpenReceipt,
  SessionTopUpRequest,
  SessionTopUpReceipt,
  SessionSettleRequest,
  SessionSettleReceipt,
  SessionCloseRequest,
  SessionCloseReceipt,
  ChannelStatus,
} from './types.js'

const DEFAULT_BASE_URL = 'https://web3.okx.com'
const API_PREFIX = '/api/v6/pay/mpp'

/** Payload passed to the SA API error callback. */
export interface SaApiErrorInfo {
  /** HTTP method. */
  method: 'GET' | 'POST'
  /** Request path (including query). */
  path: string
  /** Serialized request body (present for POST). */
  requestBody?: string
  /** HTTP status code. */
  httpStatus: number
  /** SA API business error code (undefined when the envelope failed to parse). */
  code?: number
  /** SA API error description. */
  msg?: string
  /** Raw response body text (included on parse failure / non-2xx where possible). */
  responseBody?: string
}

export interface SaApiClientConfig {
  /** OKX API key. */
  apiKey: string
  /** HMAC-SHA256 signing secret. */
  secretKey: string
  /** OKX API passphrase. */
  passphrase: string
  /** Base URL (without trailing slash). Defaults to "https://web3.okx.com". */
  baseUrl?: string
  /**
   * Optional callback fired on SA API errors (non-2xx HTTP or non-zero
   * business code). The callback is wrapped in try/catch and never affects
   * the main flow.
   */
  onError?: (info: SaApiErrorInfo) => void
}

/** Map an SA API error code to the corresponding mppx `PaymentError`. */
function saCodeToPaymentError(code: number, msg?: string): Errors.PaymentError {
  const reason = msg ?? `SA API error ${code}`

  switch (code) {
    case 8000:
    case 70000:
    case 70001:
    case 70002:
    case 70003:
    case 70007:
      return new Errors.VerificationFailedError({ reason })

    case 70004:
      return new Errors.InvalidSignatureError({ reason })

    case 70005:
    case 70006:
    case 70011:
      return new Errors.InvalidPayloadError({ reason })

    case 70009:
      return new Errors.InvalidChallengeError({ reason })

    case 70008:
    case 70014:
      return new Errors.ChannelClosedError({ reason })

    case 70010:
      return new Errors.ChannelNotFoundError({ reason })

    case 70012:
      return new Errors.AmountExceedsDepositError({ reason })

    case 70013:
      return new Errors.DeltaTooSmallError({ reason })

    default:
      return new Errors.VerificationFailedError({ reason })
  }
}

export class SaApiClient {
  private readonly baseUrl: string
  private readonly apiKey: string
  private readonly secretKey: string
  private readonly passphrase: string
  private readonly onError?: (info: SaApiErrorInfo) => void

  constructor(config: SaApiClientConfig) {
    this.baseUrl = (config.baseUrl ?? DEFAULT_BASE_URL).replace(/\/+$/, '')
    this.apiKey = config.apiKey
    this.secretKey = config.secretKey
    this.passphrase = config.passphrase
    this.onError = config.onError
  }

  /** Invoke the `onError` callback if configured; swallow errors from the callback. */
  private emitError(info: SaApiErrorInfo): void {
    if (!this.onError) return
    try { this.onError(info) } catch { /* swallow */ }
  }

  /** POST /charge/settle — server-side settle via EIP-3009 authorization. */
  async chargeSettle(credential: ChargeSettleRequest): Promise<ChargeReceipt> {
    return this.post<ChargeReceipt>(`${API_PREFIX}/charge/settle`, credential)
  }

  /** POST /charge/verifyHash — verify a client-broadcast tx hash. */
  async chargeVerifyHash(credential: ChargeVerifyHashRequest): Promise<ChargeReceipt> {
    return this.post<ChargeReceipt>(`${API_PREFIX}/charge/verifyHash`, credential)
  }

  /** POST /session/open — open a payment channel. */
  async sessionOpen(credential: SessionOpenRequest): Promise<SessionOpenReceipt> {
    return this.post<SessionOpenReceipt>(`${API_PREFIX}/session/open`, credential)
  }

  /** POST /session/topUp — add deposit to an existing channel. */
  async sessionTopUp(credential: SessionTopUpRequest): Promise<SessionTopUpReceipt> {
    return this.post<SessionTopUpReceipt>(`${API_PREFIX}/session/topUp`, credential)
  }

  /** POST /session/settle — server-initiated mid-session on-chain settlement. */
  async sessionSettle(request: SessionSettleRequest): Promise<SessionSettleReceipt> {
    return this.post<SessionSettleReceipt>(`${API_PREFIX}/session/settle`, request)
  }

  /** POST /session/close — close channel with final voucher settlement. */
  async sessionClose(credential: SessionCloseRequest): Promise<SessionCloseReceipt> {
    return this.post<SessionCloseReceipt>(`${API_PREFIX}/session/close`, credential)
  }

  /** GET /session/status — read-only channel state query. */
  async sessionStatus(channelId: string): Promise<ChannelStatus> {
    const query = `channelId=${encodeURIComponent(channelId)}`
    return this.get<ChannelStatus>(`${API_PREFIX}/session/status?${query}`)
  }

  /**
   * Build OKX API authentication headers.
   *
   * Signature = `Base64(HMAC-SHA256(secretKey, timestamp + method + requestPath + body))`.
   * Uses the Web Crypto API (`globalThis.crypto.subtle`), portable across
   * Node / browser / Edge runtimes.
   */
  private async createHeaders(method: string, requestPath: string, body?: string): Promise<Record<string, string>> {
    const timestamp = new Date().toISOString()
    const prehash = timestamp + method + requestPath + (body ?? '')
    const enc = new TextEncoder()
    const key = await globalThis.crypto.subtle.importKey(
      'raw',
      enc.encode(this.secretKey),
      { name: 'HMAC', hash: 'SHA-256' },
      false,
      ['sign'],
    )
    const sigBuf = await globalThis.crypto.subtle.sign('HMAC', key, enc.encode(prehash))
    const sig = bytesToBase64(new Uint8Array(sigBuf))

    return {
      'OK-ACCESS-KEY': this.apiKey,
      'OK-ACCESS-SIGN': sig,
      'OK-ACCESS-TIMESTAMP': timestamp,
      'OK-ACCESS-PASSPHRASE': this.passphrase,
      'Content-Type': 'application/json',
    }
  }

  private async post<T>(path: string, body: unknown): Promise<T> {
    const bodyStr = JSON.stringify(body)
    const url = this.baseUrl + path
    const res = await fetch(url, {
      method: 'POST',
      headers: await this.createHeaders('POST', path, bodyStr),
      body: bodyStr,
    })
    return this.handleResponse<T>('POST', path, res, bodyStr)
  }

  private async get<T>(path: string): Promise<T> {
    const url = this.baseUrl + path
    const res = await fetch(url, {
      method: 'GET',
      headers: await this.createHeaders('GET', path),
    })
    return this.handleResponse<T>('GET', path, res)
  }

  /**
   * Unified SA API response handler.
   *  - Non-2xx HTTP → `emitError` + throw `VerificationFailedError`.
   *  - 2xx HTTP, non-zero business code → `emitError` + throw the mapped `PaymentError`.
   *  - 2xx HTTP, business code 0 → return `data`.
   */
  private async handleResponse<T>(
    method: 'GET' | 'POST',
    path: string,
    res: Response,
    requestBody?: string,
  ): Promise<T> {
    const text = await res.text()
    if (!res.ok) {
      this.emitError({ method, path, requestBody, httpStatus: res.status, responseBody: text })
      throw new Errors.VerificationFailedError({ reason: `${method} ${path} failed: ${res.status} ${res.statusText}` })
    }
    let json: SaApiResponse<T>
    try {
      json = JSON.parse(text) as SaApiResponse<T>
    } catch {
      this.emitError({ method, path, requestBody, httpStatus: res.status, responseBody: text })
      throw new Errors.VerificationFailedError({ reason: `${method} ${path}: invalid JSON response` })
    }
    if (json.code === 0) return json.data
    this.emitError({ method, path, requestBody, httpStatus: res.status, code: json.code, msg: json.msg, responseBody: text })
    throw saCodeToPaymentError(json.code, json.msg)
  }
}

/** Uint8Array → base64 via `globalThis.btoa` (portable across runtimes). */
function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  for (const b of bytes) binary += String.fromCodePoint(b)
  return globalThis.btoa(binary)
}
