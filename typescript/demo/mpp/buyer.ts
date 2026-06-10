/**
 * Interactive Buyer — Web UI.
 *
 * Serves a single HTML page that walks through the Charge and Session
 * payment flows. All signatures (EIP-3009, EIP-712 Voucher) are produced
 * with viem `LocalAccount.signTypedData`.
 *
 * Key details:
 *  - Charge transaction mode: EIP-3009 `TransferWithAuthorization`, domain is
 *    taken from the token contract (`TOKEN_NAME` / `TOKEN_VERSION`,
 *    defaults `"USDC"` / `"1"`).
 *  - Charge hash mode: the client already broadcast on chain and posts the
 *    tx hash for SA to verify. This demo has no real tx and uses a
 *    placeholder hash (which production SA will reject).
 *  - Session voucher: EIP-712 `Voucher(channelId, cumulativeAmount)`, domain
 *    is taken from the escrow contract
 *    (`DEFAULT_DOMAIN_NAME` / `DEFAULT_DOMAIN_VERSION`).
 *  - `channelId` derivation: the client computes
 *    `keccak256(abi.encode(payer, payee, escrow, salt))`. If the SA API
 *    server uses a different formula, the SDK raises a
 *    "channel id mismatch" error.
 *
 * Port: 3002 (configurable via `BUYER_PORT`).
 */
import express from 'express'
import { Challenge, Credential } from '@okxweb3/app-mpp'
import {
  toHex,
  type Address,
  type Hex,
  type LocalAccount,
} from 'viem'
import { privateKeyToAccount } from 'viem/accounts'

const BUYER_PORT = Number.parseInt(process.env.BUYER_PORT ?? '3002', 10)
const SELLER_PORT = Number.parseInt(process.env.SERVER_PORT ?? '3000', 10)
const SELLER = `http://localhost:${SELLER_PORT}`

function requireEnv(key: string): string {
  const v = process.env[key]
  if (!v) {
    throw new Error(`Missing required env var ${key}. Did you copy .env.example to .env?`)
  }
  return v
}

const BUYER_PRIVATE_KEY = requireEnv('BUYER_PRIVATE_KEY') as Hex
const buyerAccount: LocalAccount = privateKeyToAccount(BUYER_PRIVATE_KEY)
const BUYER_ADDR = buyerAccount.address as Address
const SELLER_ADDR = requireEnv('SELLER_ADDRESS') as Address

// Must mirror the server.ts charge / session config; mismatched values cause
// challenge verification to fail.
const CHAIN_ID = Number.parseInt(process.env.CHAIN_ID ?? '196', 10) // X Layer
const TOKEN_ADDRESS = (process.env.TOKEN_ADDRESS ?? '0x779ded0c9e1022225f8e0630b35a9b54be713736') as Address
const TOKEN_NAME = process.env.TOKEN_NAME ?? 'USDC'
const TOKEN_VERSION = process.env.TOKEN_VERSION ?? '1'

// Session escrow values — must match the server-side defaults.
const ESCROW_CONTRACT = (process.env.ESCROW_CONTRACT ?? '0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b') as Address
const ESCROW_DOMAIN_NAME = process.env.ESCROW_DOMAIN_NAME ?? 'EVM Payment Channel'
const ESCROW_DOMAIN_VERSION = process.env.ESCROW_DOMAIN_VERSION ?? '1'

const TICK = 1_000_000n // Deduction per voucher tick (matches server CHARGE_CONFIG.amount).

// Placeholder hash for the hash-mode demo (SA will reject; for illustration only).
const PLACEHOLDER_TX_HASH = ('0x' + '99'.repeat(32)) as Hex

/** Random 32-byte hex (bytes32). */
function randomBytes32(): Hex {
  const buf = new Uint8Array(32)
  crypto.getRandomValues(buf)
  return toHex(buf)
}

/**
 * Sign EIP-3009 `TransferWithAuthorization` with the buyer's private key.
 * Returns both the signature and the full set of authorization fields so the
 * caller can embed them in the credential payload.
 */
async function signEip3009Authorization(params: {
  to: Address
  value: string
  validBeforeSeconds?: number
}): Promise<{
  type: 'eip-3009'
  from: Address
  to: Address
  value: string
  validAfter: string
  validBefore: string
  nonce: Hex
  signature: Hex
}> {
  const validAfter = '0'
  const validBefore = String(Math.floor(Date.now() / 1000) + (params.validBeforeSeconds ?? 3600))
  const nonce = randomBytes32()
  const signature = await buyerAccount.signTypedData({
    domain: {
      name: TOKEN_NAME,
      version: TOKEN_VERSION,
      chainId: CHAIN_ID,
      verifyingContract: TOKEN_ADDRESS,
    },
    types: {
      TransferWithAuthorization: [
        { name: 'from', type: 'address' },
        { name: 'to', type: 'address' },
        { name: 'value', type: 'uint256' },
        { name: 'validAfter', type: 'uint256' },
        { name: 'validBefore', type: 'uint256' },
        { name: 'nonce', type: 'bytes32' },
      ],
    },
    primaryType: 'TransferWithAuthorization',
    message: {
      from: BUYER_ADDR,
      to: params.to,
      value: BigInt(params.value),
      validAfter: BigInt(validAfter),
      validBefore: BigInt(validBefore),
      nonce,
    },
  })
  return {
    type: 'eip-3009',
    from: BUYER_ADDR,
    to: params.to,
    value: params.value,
    validAfter,
    validBefore,
    nonce,
    signature,
  }
}

/** Sign an EIP-712 Voucher with the buyer's private key. */
async function signVoucher(params: { channelId: Hex; cumulativeAmount: bigint }): Promise<Hex> {
  return buyerAccount.signTypedData({
    domain: {
      name: ESCROW_DOMAIN_NAME,
      version: ESCROW_DOMAIN_VERSION,
      chainId: CHAIN_ID,
      verifyingContract: ESCROW_CONTRACT,
    },
    types: {
      Voucher: [
        { name: 'channelId', type: 'bytes32' },
        { name: 'cumulativeAmount', type: 'uint128' },
      ],
    },
    primaryType: 'Voucher',
    message: {
      channelId: params.channelId,
      cumulativeAmount: params.cumulativeAmount,
    },
  })
}

let chargeChallenge: Challenge.Challenge | null = null
let sessionChallenge: Challenge.Challenge | null = null
// Production code should index session state by channelId; this demo only
// runs one channel at a time.
let sessionState: {
  salt: Hex
  channelId: Hex
  cumulativeAmount: bigint
} | null = null

function decodeReceipt(headers: Headers) {
  const raw = headers.get('payment-receipt')
  if (!raw) return null
  try { return JSON.parse(Buffer.from(raw, 'base64url').toString()) } catch { return null }
}

async function readBody(r: Response) {
  if (r.status === 204) return null
  const text = await r.text()
  if (!text) return null
  try { return JSON.parse(text) } catch { return text }
}

const app = express()
app.use(express.json())

app.get('/', (_req, res) => res.type('html').send(HTML))

// ===== Charge API =====

app.post('/api/charge/challenge', async (_req, res) => {
  const r = await fetch(`${SELLER}/charge/weather`)
  chargeChallenge = Challenge.fromResponse(r)
  const reqObj = chargeChallenge.request as Record<string, unknown>
  res.json({
    status: r.status,
    challenge: {
      id: chargeChallenge.id,
      realm: chargeChallenge.realm,
      method: chargeChallenge.method,
      intent: chargeChallenge.intent,
      expires: chargeChallenge.expires,
      amount: reqObj.amount,
      currency: reqObj.currency,
      recipient: reqObj.recipient,
    },
  })
})

app.post('/api/charge/transaction', async (_req, res) => {
  if (!chargeChallenge) return res.status(400).json({ error: 'No challenge. Get challenge first.' })
  const reqObj = chargeChallenge.request as Record<string, unknown>
  const authorization = await signEip3009Authorization({
    to: SELLER_ADDR,
    value: reqObj.amount as string,
  })
  const auth = Credential.serialize({
    challenge: chargeChallenge,
    payload: { type: 'transaction', authorization },
    source: `did:pkh:eip155:${CHAIN_ID}:${BUYER_ADDR}`,
  })
  const r = await fetch(`${SELLER}/charge/weather`, { headers: { Authorization: auth } })
  const body = await readBody(r)
  res.json({ status: r.status, body, receipt: decodeReceipt(r.headers) })
})

app.post('/api/charge/hash', async (_req, res) => {
  if (!chargeChallenge) return res.status(400).json({ error: 'No challenge. Get challenge first.' })
  // This demo cannot produce a real on-chain transaction; hash mode is for
  // illustration only and production SA will reject the placeholder tx.
  const auth = Credential.serialize({
    challenge: chargeChallenge,
    payload: { type: 'hash', hash: PLACEHOLDER_TX_HASH },
    source: `did:pkh:eip155:${CHAIN_ID}:${BUYER_ADDR}`,
  })
  const r = await fetch(`${SELLER}/charge/weather`, { headers: { Authorization: auth } })
  const body = await readBody(r)
  res.json({ status: r.status, body, receipt: decodeReceipt(r.headers) })
})

// ===== Session API =====

app.post('/api/session/challenge', async (_req, res) => {
  const r = await fetch(`${SELLER}/session/weather`)
  sessionChallenge = Challenge.fromResponse(r)
  sessionState = null
  const reqObj = sessionChallenge.request as Record<string, unknown>
  res.json({
    status: r.status,
    challenge: {
      id: sessionChallenge.id,
      method: sessionChallenge.method,
      intent: sessionChallenge.intent,
      amount: reqObj.amount,
      currency: reqObj.currency,
      recipient: reqObj.recipient,
      unitType: reqObj.unitType,
      suggestedDeposit: reqObj.suggestedDeposit,
    },
  })
})

// open: the client signs the EIP-3009 deposit and a random salt; it does not
// send a channelId and does not sign an initial voucher. The SA backend
// derives the channelId and returns it in the receipt; the buyer stores it
// in sessionState for later phases.
app.post('/api/session/open', async (req, res) => {
  if (!sessionChallenge) return res.status(400).json({ error: 'No challenge.' })
  const reqObj = sessionChallenge.request as Record<string, unknown>
  const city = (req.body?.city as string) ?? 'default'

  const salt = randomBytes32()
  const deposit = (reqObj.suggestedDeposit as string) ?? '5000000'

  // EIP-3009 authorizes the deposit to the escrow contract.
  const authorization = await signEip3009Authorization({ to: ESCROW_CONTRACT, value: deposit })

  const auth = Credential.serialize({
    challenge: sessionChallenge,
    payload: {
      action: 'open',
      type: 'transaction',
      salt,
      authorization: {
        type: authorization.type,
        from: authorization.from,
        to: authorization.to,
        value: authorization.value,
        validAfter: authorization.validAfter,
        validBefore: authorization.validBefore,
        nonce: authorization.nonce,
      },
      signature: authorization.signature,
    },
    source: `did:pkh:eip155:${CHAIN_ID}:${BUYER_ADDR}`,
  })
  const r = await fetch(`${SELLER}/session/weather?city=${city}`, { headers: { Authorization: auth } })
  const body = await readBody(r)
  const receipt = decodeReceipt(r.headers) as { channelId?: string } | null
  if (receipt?.channelId) {
    sessionState = {
      salt,
      channelId: receipt.channelId as Hex,
      cumulativeAmount: 0n,
    }
  }
  res.json({
    status: r.status,
    body,
    receipt,
    channelId: sessionState?.channelId,
  })
})

// Subsequent resource requests: cumulative += TICK; sign a new voucher.
app.post('/api/session/resource', async (req, res) => {
  if (!sessionChallenge || !sessionState) return res.status(400).json({ error: 'Open channel first.' })
  const city = (req.body?.city as string) ?? 'default'
  const next = sessionState.cumulativeAmount + TICK
  const signature = await signVoucher({ channelId: sessionState.channelId, cumulativeAmount: next })

  const auth = Credential.serialize({
    challenge: sessionChallenge,
    payload: {
      action: 'voucher',
      channelId: sessionState.channelId,
      cumulativeAmount: next.toString(),
      signature,
    },
    source: `did:pkh:eip155:${CHAIN_ID}:${BUYER_ADDR}`,
  })
  const r = await fetch(`${SELLER}/session/weather?city=${city}`, { headers: { Authorization: auth } })
  const body = await readBody(r)
  if (r.status === 200) sessionState.cumulativeAmount = next
  res.json({
    status: r.status,
    body,
    receipt: decodeReceipt(r.headers),
    cumulativeAmount: sessionState.cumulativeAmount.toString(),
  })
})

// topUp: add deposit, no charge; signs an EIP-3009 to the escrow contract.
app.post('/api/session/topup', async (_req, res) => {
  if (!sessionChallenge || !sessionState) return res.status(400).json({ error: 'Open channel first.' })
  const additionalDeposit = '3000000'
  const authorization = await signEip3009Authorization({ to: ESCROW_CONTRACT, value: additionalDeposit })

  const auth = Credential.serialize({
    challenge: sessionChallenge,
    payload: {
      action: 'topUp',
      type: 'transaction',
      channelId: sessionState.channelId,
      additionalDeposit,
      authorization: {
        type: authorization.type,
        from: authorization.from,
        to: authorization.to,
        value: authorization.value,
        validAfter: authorization.validAfter,
        validBefore: authorization.validBefore,
        nonce: authorization.nonce,
      },
      signature: authorization.signature,
    },
    source: `did:pkh:eip155:${CHAIN_ID}:${BUYER_ADDR}`,
  })
  const r = await fetch(`${SELLER}/session/weather`, { headers: { Authorization: auth } })
  const body = await readBody(r)
  res.json({
    status: r.status,
    body,
    receipt: decodeReceipt(r.headers),
    additionalDeposit,
  })
})

// close: sign the final voucher using the highest cumulative amount.
app.post('/api/session/close', async (_req, res) => {
  if (!sessionChallenge || !sessionState) return res.status(400).json({ error: 'Open channel first.' })
  const signature = await signVoucher({
    channelId: sessionState.channelId,
    cumulativeAmount: sessionState.cumulativeAmount,
  })
  const auth = Credential.serialize({
    challenge: sessionChallenge,
    payload: {
      action: 'close',
      channelId: sessionState.channelId,
      cumulativeAmount: sessionState.cumulativeAmount.toString(),
      signature,
    },
    source: `did:pkh:eip155:${CHAIN_ID}:${BUYER_ADDR}`,
  })
  const r = await fetch(`${SELLER}/session/weather`, { headers: { Authorization: auth } })
  const body = await readBody(r)
  res.json({
    status: r.status,
    body,
    receipt: decodeReceipt(r.headers),
    finalCumulative: sessionState.cumulativeAmount.toString(),
  })
})

app.listen(BUYER_PORT, () => {
  console.log(`[Buyer UI] http://localhost:${BUYER_PORT}`)
  console.log(`[Buyer UI] Seller -> ${SELLER}`)
  console.log(`[Buyer UI] Buyer  = ${BUYER_ADDR}`)
})

const HTML = /* html */ `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>MPP EVM SDK — Interactive Demo</title>
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body { font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace; background: #0d1117; color: #c9d1d9; min-height: 100vh; }
  header { background: #161b22; border-bottom: 1px solid #30363d; padding: 16px 24px; display: flex; align-items: center; gap: 16px; }
  header h1 { font-size: 16px; color: #58a6ff; }
  header span { font-size: 12px; color: #8b949e; }
  .container { display: grid; grid-template-columns: 1fr 1fr; gap: 1px; background: #30363d; margin-top: 1px; }
  .panel { background: #0d1117; padding: 20px; min-height: calc(100vh - 53px); }
  .panel h2 { font-size: 14px; color: #58a6ff; margin-bottom: 16px; padding-bottom: 8px; border-bottom: 1px solid #21262d; }
  .step { margin-bottom: 12px; }
  .step-header { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; }
  .step-num { background: #21262d; color: #8b949e; font-size: 11px; padding: 2px 8px; border-radius: 10px; }
  .step-label { font-size: 13px; color: #c9d1d9; }
  .step-hint { font-size: 11px; color: #8b949e; margin-top: 2px; }
  .btn { background: #21262d; color: #c9d1d9; border: 1px solid #30363d; padding: 6px 14px; border-radius: 6px;
         font-size: 12px; font-family: inherit; cursor: pointer; transition: all 0.15s; }
  .btn:hover:not(:disabled) { background: #30363d; border-color: #58a6ff; }
  .btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .btn-primary { background: #238636; border-color: #2ea043; color: #fff; }
  .btn-primary:hover:not(:disabled) { background: #2ea043; }
  .btn-warn { background: #9e6a03; border-color: #bb8009; color: #fff; }
  .btn-warn:hover:not(:disabled) { background: #bb8009; }
  .btn-info { background: #1f6feb; border-color: #388bfd; color: #fff; }
  .btn-info:hover:not(:disabled) { background: #388bfd; }
  .btn-danger { background: #da3633; border-color: #f85149; color: #fff; }
  .btn-danger:hover:not(:disabled) { background: #f85149; }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 4px; }
  .input-row { display: flex; gap: 8px; align-items: center; margin-bottom: 4px; flex-wrap: wrap; }
  .input-row select { background: #0d1117; border: 1px solid #30363d; color: #c9d1d9; padding: 5px 10px;
                      border-radius: 4px; font-size: 12px; font-family: inherit; width: 120px; }
  .input-row label { font-size: 11px; color: #8b949e; }
  .balance { font-size: 12px; padding: 6px 12px; margin: 8px 0; background: #21262d; border-radius: 4px;
             color: #7ee787; display: none; }
  .log { background: #161b22; border: 1px solid #21262d; border-radius: 6px; padding: 12px; margin-top: 8px;
         max-height: 550px; overflow-y: auto; font-size: 12px; line-height: 1.6; }
  .log:empty { display: none; }
  .log-entry { margin-bottom: 8px; padding-bottom: 8px; border-bottom: 1px solid #21262d; }
  .log-entry:last-child { border-bottom: none; margin-bottom: 0; padding-bottom: 0; }
  .tag { display: inline-block; font-size: 10px; padding: 1px 6px; border-radius: 3px; font-weight: bold; margin-right: 6px; }
  .tag-402 { background: #9e6a03; color: #fff; }
  .tag-200 { background: #238636; color: #fff; }
  .tag-204 { background: #6e40c9; color: #fff; }
  .tag-err { background: #da3633; color: #fff; }
  .tag-req { background: #1f6feb; color: #fff; }
  pre { margin: 4px 0 0 0; font-size: 11px; color: #8b949e; white-space: pre-wrap; word-break: break-all; }
  @media (max-width: 900px) { .container { grid-template-columns: 1fr; } }
</style>
</head>
<body>
<header>
  <h1>MPP EVM SDK</h1>
  <span>Seller: ${SELLER} &nbsp;|&nbsp; Buyer: ${BUYER_ADDR}</span>
</header>
<div class="container">

  <!-- ===== CHARGE ===== -->
  <div class="panel">
    <h2>CHARGE MODE — one-time payment</h2>

    <div class="step">
      <div class="step-header"><span class="step-num">1</span><span class="step-label">Get Challenge (402)</span></div>
      <div class="actions">
        <button class="btn btn-primary" onclick="chargeGetChallenge()">Get Challenge</button>
      </div>
    </div>

    <div class="step">
      <div class="step-header"><span class="step-num">2</span><span class="step-label">Pay</span></div>
      <div class="actions">
        <button class="btn btn-primary" id="btn-charge-tx" disabled onclick="chargePay('transaction')">Pay (Transaction)</button>
        <button class="btn btn-warn" id="btn-charge-hash" disabled onclick="chargePay('hash')">Pay (Hash)</button>
      </div>
    </div>

    <div class="log" id="charge-log"></div>
  </div>

  <!-- ===== SESSION ===== -->
  <div class="panel">
    <h2>SESSION MODE — pay-per-request</h2>

    <div class="step">
      <div class="step-header"><span class="step-num">1</span><span class="step-label">Get Challenge (402)</span></div>
      <div class="actions">
        <button class="btn btn-primary" onclick="sessionGetChallenge()">Get Challenge</button>
      </div>
    </div>

    <div class="step">
      <div class="step-header"><span class="step-num">2</span><span class="step-label">Open + first request</span></div>
      <div class="input-row">
        <label>City:</label>
        <select id="open-city">
          <option value="xlayer">X Layer</option>
          <option value="ethereum">Ethereum</option>
          <option value="default">Default</option>
        </select>
        <button class="btn btn-primary" id="btn-session-open" disabled onclick="sessionOpen()">Open & Get Weather</button>
      </div>
      <div class="step-hint">Random salt derives the channelId; client signs an EIP-3009 deposit to the escrow and the initial Voucher (cumulative=1M). The SDK does not deduct during open — it just creates the channel and stores the initial voucher. Server replies 204.</div>
    </div>

    <div class="step">
      <div class="step-header"><span class="step-num">3</span><span class="step-label">Request more resources</span></div>
      <div class="input-row">
        <label>City:</label>
        <select id="resource-city">
          <option value="xlayer">X Layer</option>
          <option value="ethereum">Ethereum</option>
          <option value="default">Default</option>
        </select>
        <button class="btn btn-warn" id="btn-session-resource" disabled onclick="sessionResource()">Get Weather</button>
      </div>
      <div class="step-hint">Each click signs a new voucher (cumulative +1M); the server verifies locally, deducts, and returns content.</div>
    </div>

    <div class="step">
      <div class="step-header"><span class="step-num">4</span><span class="step-label">Top Up (optional)</span></div>
      <div class="actions">
        <button class="btn btn-info" id="btn-session-topup" disabled onclick="sessionTopUp()">Top Up +3M</button>
      </div>
      <div class="step-hint">Add 3M to the deposit with a fresh EIP-3009 signature. Server returns 204 (no body); no deduction.</div>
    </div>

    <div class="step">
      <div class="step-header"><span class="step-num">5</span><span class="step-label">Close Channel</span></div>
      <div class="actions">
        <button class="btn btn-danger" id="btn-session-close" disabled onclick="sessionClose()">Close Channel</button>
      </div>
      <div class="step-hint">Sign a voucher with the final cumulative amount; the server submits the on-chain close. Returns 204.</div>
    </div>

    <div class="balance" id="session-balance"></div>
    <div class="log" id="session-log"></div>
  </div>
</div>

<script>
const TICK = 1000000;
const DEPOSIT = 5000000;
let cumulative = 0;
let depositTotal = DEPOSIT;
let requestCount = 0;

function logEntry(panel, tag, cls, title, data) {
  const el = document.getElementById(panel + '-log');
  const pre = data ? '<pre>' + JSON.stringify(data, null, 2) + '</pre>' : '';
  el.innerHTML += '<div class="log-entry"><span class="tag ' + cls + '">' + tag + '</span>' + title + pre + '</div>';
  el.scrollTop = el.scrollHeight;
}

function tagForStatus(status) {
  if (status === 200) return ['200', 'tag-200'];
  if (status === 204) return ['204', 'tag-204'];
  if (status === 402) return ['402', 'tag-402'];
  return ['ERR', 'tag-err'];
}

function updateBalance() {
  const el = document.getElementById('session-balance');
  const avail = depositTotal - cumulative;
  el.style.display = 'block';
  el.textContent = 'Deposit: ' + depositTotal.toLocaleString()
    + '  |  Cumulative: ' + cumulative.toLocaleString()
    + '  |  Available: ' + avail.toLocaleString()
    + '  |  Requests: ' + requestCount;
  if (avail < TICK) {
    el.style.color = '#f85149';
    document.getElementById('btn-session-resource').disabled = true;
  } else {
    el.style.color = '#7ee787';
  }
}

async function api(path, body) {
  const r = await fetch(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  });
  return r.json();
}

// ===== Charge =====

async function chargeGetChallenge() {
  logEntry('charge', 'REQ', 'tag-req', 'GET /charge/weather (no credential)');
  const d = await api('/api/charge/challenge');
  logEntry('charge', '402', 'tag-402', 'Challenge received', d.challenge);
  document.getElementById('btn-charge-tx').disabled = false;
  document.getElementById('btn-charge-hash').disabled = false;
}

async function chargePay(mode) {
  logEntry('charge', 'REQ', 'tag-req', 'Pay with type=' + mode);
  const d = await api(mode === 'transaction' ? '/api/charge/transaction' : '/api/charge/hash');
  const [tag, cls] = tagForStatus(d.status);
  if (d.status === 200) {
    logEntry('charge', tag, cls, 'Payment verified', { receipt: d.receipt, content: d.body });
  } else {
    logEntry('charge', tag, cls, 'Failed', d);
  }
  document.getElementById('btn-charge-tx').disabled = true;
  document.getElementById('btn-charge-hash').disabled = true;
}

// ===== Session =====

async function sessionGetChallenge() {
  logEntry('session', 'REQ', 'tag-req', 'GET /session/weather (no credential)');
  const d = await api('/api/session/challenge');
  logEntry('session', '402', 'tag-402', 'Session challenge', d.challenge);
  document.getElementById('btn-session-open').disabled = false;
  cumulative = 0;
  depositTotal = DEPOSIT;
  requestCount = 0;
}

async function sessionOpen() {
  const city = document.getElementById('open-city').value;
  const initialVoucher = TICK;
  logEntry('session', 'REQ', 'tag-req',
    'action=open (deposit ' + DEPOSIT.toLocaleString() + ', initial voucher=' + initialVoucher.toLocaleString() + ') city=' + city);
  const d = await api('/api/session/open', { city });
  const [tag, cls] = tagForStatus(d.status);
  if (d.status === 204) {
    logEntry('session', tag, cls, 'Channel opened', { channelId: d.channelId, receipt: d.receipt });
    document.getElementById('btn-session-open').disabled = true;
    document.getElementById('btn-session-resource').disabled = false;
    document.getElementById('btn-session-topup').disabled = false;
    document.getElementById('btn-session-close').disabled = false;
    updateBalance();
  } else {
    logEntry('session', tag, cls, 'Open failed', d);
  }
}

async function sessionResource() {
  const city = document.getElementById('resource-city').value;
  const nextCumulative = cumulative + TICK;
  logEntry('session', 'REQ', 'tag-req',
    '#' + (requestCount + 1) + ' voucher(cumulative=' + nextCumulative.toLocaleString() + ') city=' + city);
  const d = await api('/api/session/resource', { city });
  const [tag, cls] = tagForStatus(d.status);
  if (d.status === 200) {
    cumulative = nextCumulative;
    requestCount++;
    logEntry('session', tag, cls, 'Content received', { body: d.body, receipt: d.receipt });
    updateBalance();
  } else {
    logEntry('session', tag, cls, 'Request failed (balance exhausted?)', d);
  }
}

async function sessionTopUp() {
  logEntry('session', 'REQ', 'tag-req', 'action=topUp (+3M deposit)');
  const d = await api('/api/session/topup');
  const [tag, cls] = tagForStatus(d.status);
  if (d.status === 204 || d.status === 200) {
    depositTotal += 3000000;
    logEntry('session', tag, cls, 'Top-up accepted (no content)', { receipt: d.receipt });
    if (depositTotal - cumulative >= TICK) {
      document.getElementById('btn-session-resource').disabled = false;
    }
    updateBalance();
  } else {
    logEntry('session', tag, cls, 'Top-up failed', d);
  }
}

async function sessionClose() {
  logEntry('session', 'REQ', 'tag-req', 'action=close (finalCumulative=' + cumulative.toLocaleString() + ')');
  const d = await api('/api/session/close');
  const [tag, cls] = tagForStatus(d.status);
  if (d.status === 204 || d.status === 200) {
    logEntry('session', tag, cls, 'Channel closed — settled ' + cumulative.toLocaleString() + ' for ' + requestCount + ' requests', { receipt: d.receipt });
    document.getElementById('btn-session-resource').disabled = true;
    document.getElementById('btn-session-topup').disabled = true;
    document.getElementById('btn-session-close').disabled = true;
  } else {
    logEntry('session', tag, cls, 'Close failed', d);
  }
}
</script>
</body>
</html>`
