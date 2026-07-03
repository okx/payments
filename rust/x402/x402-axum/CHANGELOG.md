# Changelog

All notable changes to `okxweb3-app-x402-axum` are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-07-03

### Added
- `SubscriptionSupport` (facilitator client + optional store + access hooks) with `due_subscriptions` / `charge_and_record` for seller-driven recurring charging.
- Subscription middleware branches dispatched as an explicit verify → settle pair (subscribe / change / cancel / cancel-pending / APP-Access).
- `on_before_access` merchant access-veto hook (`OnBeforeAccessHook` / `AccessContext` / `BeforeAccessResult`).
- `InMemorySubscriptionStore` plus `SubscriptionStore` / `SubscriptionRecord` re-exports.

## [0.2.0] - 2026-05-28

First release on crates.io.
