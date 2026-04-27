//! MPP EVM Seller SDK for OKX Payments.
//!
//! Integrates the Machine Payments Protocol (MPP) `evm` method into the
//! OKX Payments SDK. Implements mpp-rs's [`ChargeMethod`] and [`SessionMethod`]
//! traits backed by the OKX SA API, so sellers don't operate an RPC node or
//! manage on-chain state themselves.
//!
//! Target chain: **X Layer (chainId 196)**. ERC-20 tokens with EIP-3009
//! `transferWithAuthorization` support.
//!
//! See [`README.md`](https://github.com/okx/payments/blob/main/rust/mpp/README.md)
//! for quick-start examples.
//!
//! # Module map
//!
//! - [`sa_client`] — SA API HTTP client ([`OkxSaApiClient`]) + pluggable
//!   [`SaApiClient`] trait for injecting mocks or alternative backends.
//! - [`charge_method`] — [`EvmChargeMethod`] (`impl mpp::protocol::traits::ChargeMethod`).
//! - [`session_method`] — [`EvmSessionMethod`] with 5-minute idle-timeout
//!   auto-settle (calls `/session/settle`, not `/close`, so no client
//!   signature is needed on timeout).
//! - [`store`] — Minimal session channel registry ([`SessionStore`] trait +
//!   [`InMemorySessionStore`] default). Name intentionally distinct from
//!   upstream `tempo::session_method::ChannelStore` which has a different model.
//! - [`types`] — Spec §8 data model: method details, EIP-3009 authorization,
//!   receipts, EIP-712 voucher domain constants, server accounting state.
//! - [`challenge`] — `method="evm"` challenge builders for charge + session
//!   (upstream `Mpp::charge()` is gated on the `tempo` feature).
//! - [`handlers`] *(feature = "handlers")* — Drop-in Axum handlers for
//!   `/session/settle` and `/session/status`. Enable cargo feature `handlers`
//!   to include them; otherwise write your own.
//! - [`mock`] *(feature = "mock")* — [`MockSaApiClient`] for local dev /
//!   example flow verification. Returns fixed success data, no signature /
//!   chain validation. **DO NOT enable in production dep chains.**
//! - [`error`] — [`SaApiError`] with canonical RFC 9457 mapping for all 16
//!   documented SA API error codes.
//!
//! [`ChargeMethod`]: mpp::protocol::traits::ChargeMethod
//! [`SessionMethod`]: mpp::protocol::traits::SessionMethod

pub mod challenge;
pub mod challenger;
pub mod charge_method;
pub mod eip712;
pub mod error;
#[cfg(feature = "handlers")]
pub mod handlers;
#[cfg(feature = "mock")]
pub mod mock;
pub mod nonce;
pub mod sa_client;
pub mod session_method;
pub mod store;
pub mod types;

pub use challenger::{EvmChargeChallenger, EvmChargeChallengerConfig};
pub use charge_method::EvmChargeMethod;
pub use eip712::{
    build_domain, sign_close_authorization, sign_settle_authorization, verify_voucher,
    CloseAuthorization, SettleAuthorization, SignedAuthorization, VerifyError, Voucher,
    VOUCHER_DOMAIN_NAME, VOUCHER_DOMAIN_VERSION,
};
pub use error::SaApiError;
pub use nonce::{NonceProvider, UuidNonceProvider};
pub use types::{
    ChargeMethodDetails, ChargeSplit, CloseRequestPayload, SettleRequestPayload, DEFAULT_CHAIN_ID,
};
#[cfg(feature = "mock")]
pub use mock::MockSaApiClient;
pub use sa_client::{OkxSaApiClient, SaApiClient};
pub use session_method::EvmSessionMethod;
pub use store::{ChannelRecord, InMemorySessionStore, SessionStore};
