//! EIP-712 domain constants for OKX EvmPaymentChannel.
//!
//! `domainSeparator` 由合约暴露 `domainSeparator()` 方法可直接读链上取，
//! SDK 启动时建议做一次链上 vs 本地 `eip712_domain!` 算出的值的相等性校验
//! —— 不一致直接拒绝启动，避免后续所有签名都无效。
//!
//! ## 4 个 domain 字段如何配置
//!
//! - `name` / `version`:走 [`DomainMeta`],默认 `"EVM Payment Channel"` / `"1"`,
//!   开发者 fork 合约时可通过 `EvmSessionMethod::with_domain_meta(...)` 改
//! - `chainId` / `verifying_contract`:每次 [`build_domain`] 调用按入参传入

use std::borrow::Cow;

use alloy_primitives::{Address, U256};
use alloy_sol_types::Eip712Domain;

/// EIP-712 domain `name` 字段默认值。值来自 OKX EvmPaymentChannel 合约 source code,
/// 必须**逐字节**和合约一致:大小写、空格、标点不能错。
pub const VOUCHER_DOMAIN_NAME: &str = "EVM Payment Channel";

/// EIP-712 domain `version` 字段默认值。
pub const VOUCHER_DOMAIN_VERSION: &str = "1";

/// EIP-712 domain 的可配置元数据(`name` / `version` 两个字段)。
///
/// `chainId` 和 `verifying_contract` 从来都是按场景动态传入,所以不在这里。
/// 只有当开发者 fork 合约改了 `name` / `version` 时才需要构造非默认值。
///
/// `Default::default()` 返回标准 OKX EvmPaymentChannel domain
/// (`"EVM Payment Channel"` / `"1"`)。
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
    /// 构造自定义 DomainMeta。`name` / `version` 必须跟合约部署时的 EIP-712 domain
    /// 完全一致(逐字节),否则签名验证一定挂。
    pub fn new(
        name: impl Into<Cow<'static, str>>,
        version: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// 构造 EIP-712 domain,用于 Voucher / SettleAuthorization / CloseAuthorization
/// 三种 typed message 的签名与验签。
///
/// `meta` 决定 `name` / `version`(默认走 OKX 标准值,见 [`DomainMeta::default`]);
/// `chain_id` / `escrow_contract` 必填。
///
/// 注意:这里**不能**用 `eip712_domain!` 宏 — 该宏要求 `name`/`version` 是编译期
/// `&'static str` 字面量,而我们的 [`DomainMeta`] 里是运行期 `Cow<'static, str>`
/// (`Cow::Owned(String)` 满足 `'static` 但宏不接 runtime 值)。所以直接构造
/// `Eip712Domain`,把 Cow 移进去。
pub fn build_domain(
    meta: &DomainMeta,
    chain_id: u64,
    escrow_contract: Address,
) -> Eip712Domain {
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
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
        let m = DomainMeta::default();
        let a = build_domain(&m, 196, escrow);
        let b = build_domain(&m, 196, escrow);
        assert_eq!(a.separator(), b.separator());
    }

    #[test]
    fn different_chain_id_yields_different_separator() {
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
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
            address!("eb18025208061781a287fFc2c1F31C03A24a24c0"),
        )
        .separator();
        assert_ne!(a, b);
    }

    #[test]
    fn different_meta_yields_different_separator() {
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
        let default = DomainMeta::default();
        let custom = DomainMeta::new("Forked Channel", "2");
        let a = build_domain(&default, 196, escrow).separator();
        let b = build_domain(&custom, 196, escrow).separator();
        assert_ne!(a, b);
    }

    #[test]
    fn custom_meta_with_default_values_matches_default() {
        // 显式传跟 Default 同样的值 → separator 相等
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
        let default = DomainMeta::default();
        let custom = DomainMeta::new(VOUCHER_DOMAIN_NAME, VOUCHER_DOMAIN_VERSION);
        assert_eq!(
            build_domain(&default, 196, escrow).separator(),
            build_domain(&custom, 196, escrow).separator(),
        );
    }
}
