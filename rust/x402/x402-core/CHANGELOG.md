# Changelog

All notable changes to `okxweb3-app-x402-core` are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.1] - 2026-07-10

### Added
- `FacilitatorClient::verify_signature_only` (default: unsupported) plus the `OkxHttpFacilitatorClient` implementation calling `POST /api/v6/pay/x402/verify-signature` — signature-only verify (no balance / nonce / blacklist checks).
- `X402ResourceServer::recover_payer` — recovers the payer via signature-only verify, gated on `isValid`. Internal plumbing for the review-wallet exemption (`#[doc(hidden)]`).

## [0.3.0] - 2026-07-03

### Added
- `subscription` module for the `period` scheme: `SubscriptionTerms` / `PermitSingle` / `CancelAuth` / `PendingChangeCancelAuth` / `AccessProof` types, facilitator request/response types, the `codec` (payload ↔ facilitator request), period math, and the `subscription_state` / `change_effective_at` / `cancel_initiator` / `PERIOD_MODE_*` constants.
- `SubscriptionFacilitatorClient` and `SubscriptionStore` traits (`OkxHttpFacilitatorClient` implements the former).
- `SubscriptionOperation` enum and the `RoutePaymentConfig.operation` field for change / cancel / cancel-pending routes.
- `SettlementMode` on `SchemeNetworkServer` (`Post` / `Pre`) so the middleware can settle-before-serve for `period`.

## [0.2.0] - 2026-05-28

First release on crates.io.
