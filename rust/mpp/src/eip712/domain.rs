//! EIP-712 domain constants for OKX EvmPaymentChannel.
//!
//! The contract exposes `domainSeparator()`, so the on-chain value can be
//! read directly. At SDK startup it's recommended to compare the on-chain
//! value against the locally computed one and refuse to start on
//! mismatch — otherwise every subsequent signature will be invalid.
//!
//! ## How the 4 domain fields are configured
//!
//! - `name` / `version`: come from [`DomainMeta`]; defaults are
//!   `"EVM Payment Channel"` / `"1"`. Fork the contract? Override via
//!   `EvmSessionMethod::with_domain_meta(...)`.
//! - `chainId` / `verifying_contract`: passed as arguments on every
//!   [`build_domain`] call.

use std::borrow::Cow;

use alloy_primitives::{Address, U256};
use alloy_sol_types::Eip712Domain;

/// Default value for the EIP-712 domain `name` field. Sourced from the
/// OKX EvmPaymentChannel contract; **must match byte-for-byte** —
/// capitalization, whitespace, and punctuation cannot drift.
pub const VOUCHER_DOMAIN_NAME: &str = "EVM Payment Channel";

/// Default value for the EIP-712 domain `version` field.
pub const VOUCHER_DOMAIN_VERSION: &str = "1";

/// Configurable EIP-712 domain metadata (`name` and `version` only).
///
/// `chainId` / `verifying_contract` are always supplied per call, so
/// they're not part of this struct. Construct a non-default value only
/// when forking the contract with a different `name` / `version`.
///
/// `Default::default()` returns the canonical OKX EvmPaymentChannel
/// domain (`"EVM Payment Channel"` / `"1"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainMeta {
    pub name: Cow<'static, str>,
    pub version: Cow<'static, str>,
}

impl Default for DomainMeta {
    fn default() -> Self {
        Self {
            name: Cow::Borrowed(VOUCHER_DOMAIN_NAME),
            version: Cow::Borrowed(VOUCHER_DOMAIN_VERSION),
        }
    }
}

impl DomainMeta {
    /// Construct a custom `DomainMeta`. `name` / `version` must match the
    /// deployed contract's EIP-712 domain byte-for-byte; otherwise signature
    /// verification will fail.
    pub fn new(name: impl Into<Cow<'static, str>>, version: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// Build the EIP-712 domain used to sign / verify the three typed
/// messages (Voucher, SettleAuthorization, CloseAuthorization).
///
/// `meta` controls `name` / `version` (defaults are the OKX canonical
/// values — see [`DomainMeta::default`]); `chain_id` and
/// `escrow_contract` are required.
///
/// Note: we **cannot** use the `eip712_domain!` macro here — it requires
/// `name` / `version` to be compile-time `&'static str` literals, while
/// [`DomainMeta`] holds a runtime `Cow<'static, str>` (`Cow::Owned(String)`
/// satisfies `'static` but the macro doesn't accept runtime values).
/// We therefore construct `Eip712Domain` directly and move the `Cow`s in.
pub fn build_domain(meta: &DomainMeta, chain_id: u64, escrow_contract: Address) -> Eip712Domain {
    Eip712Domain {
        name: Some(meta.name.clone()),
        version: Some(meta.version.clone()),
        chain_id: Some(U256::from(chain_id)),
        verifying_contract: Some(escrow_contract),
        salt: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn default_meta_uses_constants() {
        let m = DomainMeta::default();
        assert_eq!(m.name.as_ref(), VOUCHER_DOMAIN_NAME);
        assert_eq!(m.version.as_ref(), VOUCHER_DOMAIN_VERSION);
    }

    #[test]
    fn build_domain_is_deterministic() {
        let escrow = address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b");
        let m = DomainMeta::default();
        let a = build_domain(&m, 196, escrow);
        let b = build_domain(&m, 196, escrow);
        assert_eq!(a.separator(), b.separator());
    }

    #[test]
    fn different_chain_id_yields_different_separator() {
        let escrow = address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b");
        let m = DomainMeta::default();
        let a = build_domain(&m, 196, escrow).separator();
        let b = build_domain(&m, 1, escrow).separator();
        assert_ne!(a, b);
    }

    #[test]
    fn different_escrow_yields_different_separator() {
        let chain_id = 196u64;
        let m = DomainMeta::default();
        let a = build_domain(&m, chain_id, Address::ZERO).separator();
        let b = build_domain(
            &m,
            chain_id,
            address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b"),
        )
        .separator();
        assert_ne!(a, b);
    }

    #[test]
    fn different_meta_yields_different_separator() {
        let escrow = address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b");
        let default = DomainMeta::default();
        let custom = DomainMeta::new("Forked Channel", "2");
        let a = build_domain(&default, 196, escrow).separator();
        let b = build_domain(&custom, 196, escrow).separator();
        assert_ne!(a, b);
    }

    #[test]
    fn custom_meta_with_default_values_matches_default() {
        // Explicitly passing the Default values → separators must be equal.
        let escrow = address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b");
        let default = DomainMeta::default();
        let custom = DomainMeta::new(VOUCHER_DOMAIN_NAME, VOUCHER_DOMAIN_VERSION);
        assert_eq!(
            build_domain(&default, 196, escrow).separator(),
            build_domain(&custom, 196, escrow).separator(),
        );
    }
}
