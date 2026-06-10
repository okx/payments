# @okxweb3/app-mpp

OKX EVM payment method SDK for the [MPP (Machine Payment Protocol)](https://mpp.dev). Implements on-chain ERC-20 settlement via the OKX SA (Settlement API), supporting both one-shot **charge** and streaming **session** payment patterns.

## Installation

```bash
npm install @okxweb3/app-mpp
```

## Quick Start

### Charge (one-shot payment)

```ts
import { Mppx } from '@okxweb3/app-mpp'
import { charge } from '@okxweb3/app-mpp/evm/server'
import { SaApiClient } from '@okxweb3/app-mpp/evm'

const saClient = new SaApiClient({
  apiKey: process.env.OKX_API_KEY!,
  secretKey: process.env.OKX_SECRET_KEY!,
  passphrase: process.env.OKX_PASSPHRASE!,
})

const mppx = Mppx.create({
  methods: [charge({ saClient })],
})

export async function handler(request: Request) {
  const result = await mppx.charge({
    amount: '1000000',           // 1 USDC (6 decimals)
    currency: '0xA8CE8aee21bC2A48a5EF670afCc9274C7bbbC035', // USDC on X Layer
    recipient: '0x742d35Cc6634c0532925a3b844bC9e7595F8fE00',
    methodDetails: { chainId: 196, feePayer: true },
  })(request)

  if (result.status === 402) return result.challenge
  return result.withReceipt(Response.json({ data: 'success' }))
}
```

### Session (streaming / pay-per-use)

```ts
import { Mppx } from '@okxweb3/app-mpp'
import { session } from '@okxweb3/app-mpp/evm/server'
import { SaApiClient } from '@okxweb3/app-mpp/evm'

const saClient = new SaApiClient({ /* ... */ })

const mppx = Mppx.create({
  methods: [session({ saClient, signer: serverWallet })],
})

export async function handler(request: Request) {
  const result = await mppx.session({
    amount: '100',               // price per request unit
    currency: '0x...',
    recipient: '0x...',
    unitType: 'request',
    suggestedDeposit: '10000000',
    methodDetails: {
      chainId: 196,
      escrowContract: '0x...',
    },
  })(request)

  if (result.status === 402) return result.challenge
  return result.withReceipt(Response.json({ data: 'response' }))
}
```

## API Reference

### `SaApiClient`

OKX Settlement API client. All settlement and verification calls go through this client.

```ts
import { SaApiClient } from '@okxweb3/app-mpp/evm'

const saClient = new SaApiClient({
  apiKey: string,
  secretKey: string,
  passphrase: string,
  baseUrl?: string,   // default: OKX SA API production endpoint
})
```

### `charge(parameters)`

Creates a server-side EVM charge method. Handles two settlement modes:

| `payload.type` | Behavior |
|---|---|
| `"transaction"` | Client provides EIP-3009 / Permit2 / delegation authorization; server broadcasts on-chain transfer via SA API |
| `"hash"` | Client already broadcast the tx; server verifies the on-chain hash via SA API |

**Parameters:**

| Field | Type | Description |
|---|---|---|
| `saClient` | `SaApiClient` | SA API client instance |

**Challenge request fields:**

| Field | Type | Description |
|---|---|---|
| `amount` | `string` | Payment amount in token base units |
| `currency` | `string` | ERC-20 token contract address (EIP-55) |
| `recipient` | `string` | Payee address (EIP-55) |
| `description` | `string?` | Human-readable description |
| `externalId` | `string?` | Merchant order reference |
| `methodDetails.chainId` | `number` | EVM chain ID (default: `196` for X Layer) |
| `methodDetails.feePayer` | `boolean?` | If `true`, server pays gas (requires transaction mode) |
| `methodDetails.permit2Address` | `string?` | Custom Permit2 contract address |
| `methodDetails.splits` | `Split[]?` | Split payment recipients (max 10) |

### `session(parameters)`

Creates a server-side EVM session method for pay-as-you-go streaming payments via on-chain escrow and off-chain EIP-712 vouchers.

**Parameters:**

| Field | Type | Description |
|---|---|---|
| `saClient` | `SaApiClient` | SA API client instance |
| `signer` | `SessionSigner` | Server wallet for EIP-712 close/settle signing |
| `store` | `SessionStore?` | Channel state store (default: in-memory) |

**Session actions (credential `payload.action`):**

| Action | Description |
|---|---|
| `open` | Open a payment channel (transaction or hash mode) |
| `voucher` | Submit a cumulative off-chain voucher (high-frequency) |
| `topUp` | Add deposit to an existing channel |
| `close` | Close the channel with final on-chain settlement |

**Challenge request fields:**

| Field | Type | Description |
|---|---|---|
| `amount` | `string` | Price per unit in token base units |
| `currency` | `string` | ERC-20 token contract address |
| `recipient` | `string` | Payee address |
| `unitType` | `string?` | Unit being priced (e.g. `"request"`, `"llm_token"`, `"byte"`) |
| `suggestedDeposit` | `string?` | Suggested initial deposit amount |
| `methodDetails.chainId` | `number` | EVM chain ID |
| `methodDetails.escrowContract` | `string` | On-chain escrow contract address |

### `SessionStore`

Interface for channel state persistence. Implement to back channels with Redis, Postgres, Cloudflare KV, etc.

```ts
interface SessionStore {
  get(channelId: string): Promise<ChannelState | null>
  set(channelId: string, state: ChannelState): Promise<void>
  delete(channelId: string): Promise<void>
  update(channelId: string, mutator: ChannelMutator): Promise<ChannelState | null>
}
```

## Exports

| Entry point | Contents |
|---|---|
| `@okxweb3/app-mpp` | Full `mppx` namespace re-export + server entities (`Mppx`, `NodeListener`, `Request`, `Response`, `Transport`) |
| `@okxweb3/app-mpp/evm` | `SaApiClient`, EVM method schemas, authorization type schemas |
| `@okxweb3/app-mpp/evm/server` | `charge()`, `session()` server factory functions |

## Supported Networks

| Network | Chain ID | Default |
|---|---|---|
| X Layer Mainnet | 196 | ✅ |
| Ethereum Mainnet | 1 | via `methodDetails.chainId` |
| Other EVM chains | any | via `methodDetails.chainId` |

## License

MIT © OKX
