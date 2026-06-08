# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository structure

This is a sub-project within the `payments-sdk` monorepo at `typescript/mpp-typescript/`.

- **`sdk/`** — The `@okxweb3/app-mpp` npm package: an EVM payment method for the [MPP protocol](https://mpp.dev), using OKX SA API for on-chain settlement.

## Commands

### SDK (`sdk/`)
```bash
npm install
npx tsc --noEmit          # typecheck
```

## How the 402 payment flow works (EVM Charge)

1. Client sends a normal HTTP request.
2. Server calls `mppx.charge({ amount, currency, recipient, methodDetails })(request)`. If no valid credential is present, it returns a `402` with a `WWW-Authenticate: Payment` header.
3. Client signs an EIP-3009 authorization (or broadcasts a tx and gets a hash) and retries with an `Authorization: Payment <credential>` header.
4. Server forwards the credential to SA API:
   - `payload.type === "transaction"` → POST /charge/settle (SA API broadcasts on-chain)
   - `payload.type === "hash"` → POST /charge/verifyHash (SA API verifies client-broadcast tx)
5. Server returns the response with a `Payment-Receipt` header.

## SDK architecture

**`sdk/src/Methods.ts`** — Defines the EVM `charge` method schema using `mppx`'s `Method.from()`. Request schema: amount, currency, recipient, methodDetails (chainId, feePayer, splits). Credential payload: discriminated union on `type` (transaction vs hash), with authorization supporting EIP-3009, Permit2, and ERC-7710 delegation.

**`sdk/src/server/Charge.ts`** — Server-side `charge()`. Takes an `SaApiClient` instance. Implements `verify()` which dispatches based on `payload.type` to the appropriate SA API endpoint. Converts SA API receipts to mppx `Receipt.from()` format.

**`sdk/src/sa/SaApiClient.ts`** — HTTP client wrapping OKX SA API. Covers 7 endpoints: charge/settle, charge/verifyHash, session/open, session/topUp, session/settle, session/close, session/status. Voucher 验签已移到 SDK 本地（viem EIP-712），不调 SA API。Includes OKX API authentication (HMAC-SHA256) and structured error code parsing (70000–70014).

**`sdk/src/sa/types.ts`** — TypeScript type definitions for all SA API request/response structures.

## OKX API Authentication

All SA API calls are authenticated with:
- `OK-ACCESS-KEY` — API key
- `OK-ACCESS-SIGN` — HMAC-SHA256(secretKey, timestamp + method + path + body), base64-encoded
- `OK-ACCESS-TIMESTAMP` — ISO 8601 timestamp
- `OK-ACCESS-PASSPHRASE` — API passphrase
