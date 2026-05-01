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
//! - [`charge`] — Charge intent (one-shot pay → settle): [`EvmChargeMethod`]
//!   (`impl mpp::protocol::traits::ChargeMethod`), [`EvmChargeChallenger`],
//!   and the shared `method="evm"` challenge builders.
//! - [`session_method`] — [`EvmSessionMethod`] (`impl
//!   mpp::protocol::traits::SessionMethod`). Merchant-driven lifecycle:
//!   the merchant calls [`EvmSessionMethod::settle_with_authorization`] /
//!   [`EvmSessionMethod::close_with_authorization`] explicitly. There is
//!   **no idle timer**; abandoned channels stay open until the merchant
//!   settles or the on-chain escrow timeout fires.
//! - [`store`] — Minimal session channel registry ([`SessionStore`] trait +
//!   [`InMemorySessionStore`] default). Name intentionally distinct from
//!   upstream `tempo::session_method::ChannelStore` which has a different model.
//! - [`types`] — Spec §8 data model: method details, EIP-3009 authorization,
//!   receipts, EIP-712 voucher domain constants, server accounting state.
//! - [`axum`] *(feature = "handlers")* — Drop-in Axum handlers for
//!   `/session/settle` and `/session/status`. Enable cargo feature `handlers`
//!   to include them; otherwise write your own.
//! - [`error`] — [`SaApiError`] with canonical RFC 9457 mapping for all 16
//!   documented SA API error codes.
//!
//! [`ChargeMethod`]: mpp::protocol::traits::ChargeMethod
//! [`SessionMethod`]: mpp::protocol::traits::SessionMethod

#[cfg(feature = "handlers")]
pub mod axum;
pub mod charge;
pub mod credential_ext;
pub mod eip712;
pub mod error;
pub mod nonce;
pub mod sa_client;
pub mod session_method;
pub mod store;
pub mod types;

pub use charge::{EvmChargeChallenger, EvmChargeChallengerConfig, EvmChargeMethod};
pub use credential_ext::CredentialExt;
pub use eip712::{
    build_domain, sign_close_authorization, sign_settle_authorization, verify_voucher,
    CloseAuthorization, DomainMeta, SettleAuthorization, SignedAuthorization, VerifyError, Voucher,
    VOUCHER_DOMAIN_NAME, VOUCHER_DOMAIN_VERSION,
};
pub use error::SaApiError;
pub use nonce::{NonceProvider, UuidNonceProvider};
pub use sa_client::{OkxSaApiClient, SaApiClient};
pub use session_method::EvmSessionMethod;
pub use store::{ChannelRecord, InMemorySessionStore, SessionStore};
pub use types::{
    ChargeMethodDetails, ChargeSplit, CloseRequestPayload, SettleRequestPayload, DEFAULT_CHAIN_ID,
};
