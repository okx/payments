# PRD: x402 Payments SDK — Feature Development

> **Note**: This PRD was synthesized from existing repository documentation (README.md, docs/, specs/, CONTRIBUTING.md) because no external PRD URL was provided. It captures the project's goals, architecture, and development priorities as found in the codebase.

---

## Overview

**x402** is an open standard for internet-native payments over HTTP. It leverages the HTTP `402 Payment Required` status code to enable programmatic, permissionless micro-payments between clients and resource servers — with first-class support for AI agents, crypto networks, and minimal integration effort.

- **GitHub**: https://github.com/coinbase/x402
- **License**: Apache-2.0
- **Sponsor**: Coinbase Developer Platform

---

## Problem Statement

Current payment systems on the internet require:
- Accounts and credential management
- Manual payment flows unsuited for machine-to-machine use
- Complex integration for developers
- Network and token lock-in

x402 solves this by embedding payment semantics directly in HTTP, enabling any web service to require payment before serving a response, with minimal code changes on both client and server.

---

## Goals & Principles

1. **Open standard** — freely accessible, no single-party reliance
2. **HTTP-native** — minimal additional requests beyond normal client/server flow
3. **Network, token, and currency agnostic** — supports EVM, Solana, Stellar, Aptos; extensible to fiat
4. **Backwards compatible** — no removal of existing network support unless critical security issue
5. **Trust-minimizing** — facilitator cannot move funds outside client intentions
6. **Easy to use** — abstracts gas, RPC, and crypto complexity away from client/server

---

## Technical Architecture

### Key Roles

| Role | Description |
|------|-------------|
| **Client** | Entity wanting to pay for a resource (human or AI agent) |
| **Resource Server** | HTTP server providing a paid API or resource |
| **Facilitator** | Server that verifies and settles payments across networks |

### Typical Payment Flow

1. Client makes HTTP request to resource server
2. Server responds with `402 Payment Required` + payment instructions
3. Client prepares and submits a payment payload
4. Server verifies/settles (optionally via facilitator's `/verify` + `/settle` endpoints)
5. Server delivers the requested resource

### TypeScript SDK Structure

```
typescript/
├── packages/
│   ├── core/           @x402/core       — Protocol types, HTTP client/server, facilitator primitives
│   ├── mechanisms/
│   │   ├── evm/        @x402/evm        — EVM (EIP-3009, Permit2)
│   │   ├── svm/        @x402/svm        — Solana SPL
│   │   ├── stellar/    @x402/stellar    — Stellar/Soroban
│   │   └── aptos/      @x402/aptos      — Aptos Fungible Assets
│   ├── http/
│   │   ├── express/    @x402/express    — Express middleware
│   │   ├── fetch/      @x402/fetch      — Fetch wrapper
│   │   ├── axios/      @x402/axios      — Axios interceptor
│   │   ├── hono/       @x402/hono       — Hono middleware
│   │   ├── next/       @x402/next       — Next.js integration
│   │   └── paywall/    @x402/paywall    — Browser paywall UI
│   └── extensions/     @x402/extensions — Bazaar, Sign-in-with-x, payment-identifier, offer-receipt
```

---

## Feature Scope

### Current SDK Features (v2.x)

| Category | Feature | Status |
|----------|---------|--------|
| Core | Server/Client/Facilitator | Complete |
| Networks | EVM, SVM, Stellar, Aptos | Complete (TS); EVM+SVM in Go/Python |
| Mechanisms | exact/evm (EIP-3009, Permit2), exact/svm (SPL), exact/stellar, exact/aptos | Complete |
| Extensions | bazaar, sign-in-with-x, payment-identifier, offer-receipt, eip2612/erc20 gas sponsoring | Complete |
| Client Hooks | onBefore/AfterPaymentCreation, onPaymentCreationFailure, onPaymentRequired | Complete |
| Server Hooks | onBefore/AfterVerify/Settle, onVerifyFailure, onSettleFailure, onProtectedRequest | Complete |
| HTTP Features | dynamicPayTo, dynamicPrice, paywall UI | Complete |

### Integration Points

- **TypeScript**: Express, Hono, Next.js, Fetch, Axios
- **Python**: FastAPI, Flask, httpx, requests
- **Go**: Gin, net/http, Echo

---

## Developer Experience Goals

- **1-line server integration**: `app.use(paymentMiddleware(...))`
- **1-function client integration**: wrap fetch/axios with payment logic
- **No accounts or sessions needed**
- **AI agent native**: designed for autonomous machine-to-machine payment flows

---

## Development Priorities (from PROJECT-IDEAS.md)

The x402 ecosystem is actively encouraging:

1. **AI Agent integrations** — unstoppable agents that provision inference via x402
2. **Financial agents** — trading bots, prediction market oracles, KYC/AML checkers
3. **Commerce agents** — crypto-native checkout, agentic commerce proofs-of-concept
4. **Knowledge services** — bounty hunters, code review marketplaces, consultant agents
5. **MCP server monetization** — dynamic endpoint discovery and payment

Micro-grants up to $3k available for live mainnet implementations.

---

## Acceptance Criteria for New Features

From CONTRIBUTING.md:
- Follow language-specific development guide
- Write tests for all new functionality
- Update documentation
- Pass CI (build, lint, test)
- Use changesets for version management (`pnpm changeset`)
- Contributions subject to x402 Foundation team review

---

## Out of Scope

- UI/UX redesign (this is a headless SDK)
- Fiat payment prioritization over onchain
- Proprietary lock-in of any kind

---

## References

- Specs: `specs/x402-specification-v1.md`, `specs/x402-specification-v2.md`
- Docs: `docs/introduction.md`, `docs/core-concepts/`, `docs/sdk-features.md`
- Contributing: `CONTRIBUTING.md`, `typescript/CONTRIBUTING.md`
- Project ideas: `PROJECT-IDEAS.md`
