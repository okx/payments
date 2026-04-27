//! EIP-712 domain constants for OKX EvmPaymentChannel.
//!
//! `domainSeparator` 由合约暴露 `domainSeparator()` 方法可直接读链上取，
//! SDK 启动时建议做一次链上 vs 本地 `eip712_domain!` 算出的值的相等性校验
//! （Q16）—— 不一致直接拒绝启动，避免后续所有签名都无效。

use alloy_primitives::Address;
use alloy_sol_types::{eip712_domain, Eip712Domain};

/// EIP-712 domain `name` 字段。值来自合约 source code，必须**逐字节**和合约一致：
/// 大小写、空格、标点不能错。
pub const VOUCHER_DOMAIN_NAME: &str = "EVM Payment Channel";

/// EIP-712 domain `version` 字段。
pub const VOUCHER_DOMAIN_VERSION: &str = "1";

/// 构造 EIP-712 domain，用于 Voucher / SettleAuthorization / CloseAuthorization
/// 三种 typed message 的签名与验签。
pub fn build_domain(chain_id: u64, escrow_contract: Address) -> Eip712Domain {
    eip712_domain! {
        name: VOUCHER_DOMAIN_NAME,
        version: VOUCHER_DOMAIN_VERSION,
        chain_id: chain_id,
        verifying_contract: escrow_contract,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn build_domain_is_deterministic() {
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
        let a = build_domain(196, escrow);
        let b = build_domain(196, escrow);
        assert_eq!(a.separator(), b.separator());
    }

    #[test]
    fn different_chain_id_yields_different_separator() {
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
        let a = build_domain(196, escrow).separator();
        let b = build_domain(1, escrow).separator();
        assert_ne!(a, b);
    }

    #[test]
    fn different_escrow_yields_different_separator() {
        let chain_id = 196u64;
        let a = build_domain(chain_id, Address::ZERO).separator();
        let b = build_domain(chain_id, address!("eb18025208061781a287fFc2c1F31C03A24a24c0"))
            .separator();
        assert_ne!(a, b);
    }
}
