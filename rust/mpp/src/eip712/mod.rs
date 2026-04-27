//! EIP-712 typed-data primitives shared by signing and verification paths.
//!
//! Mirrors mpp-rs `protocol/methods/tempo/voucher.rs` layout: a single source
//! of truth for the typed structs and the domain. Both the on-chain Voucher
//! signature (verified locally by `voucher::verify_voucher`) and the payee-side
//! SettleAuthorization / CloseAuthorization signatures (produced locally by
//! `authorization::sign_*`) live here so the wire format cannot drift between
//! the two paths.

pub mod authorization;
pub mod domain;
pub mod voucher;

pub use authorization::{
    sign_close_authorization, sign_settle_authorization, CloseAuthorization, SettleAuthorization,
    SignedAuthorization,
};
pub use domain::{build_domain, VOUCHER_DOMAIN_NAME, VOUCHER_DOMAIN_VERSION};
pub use voucher::{verify_voucher, VerifyError, Voucher};
