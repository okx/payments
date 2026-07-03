# Changelog

All notable changes to `okxweb3-app-x402-evm` are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-07-03

### Added
- `subscription` module: `PermitSubscriptionScheme` (the `period` scheme), `SubscriptionPlan` (with `to_accept_config`), and AccessProof EIP-712 verification.

### Changed
- `PermitSubscriptionScheme` reads the facilitator EOA from `/supported` under `facilitatorAddress`, falling back to the legacy `facilitator` key (works with both old and new backends).

## [0.2.0] - 2026-05-28

First release on crates.io.
