//! Nonce 分配 trait + 默认 UUID v4 实现。
//!
//! 合约层 nonce 已用集 key = `(payee, channelId, nonce)`，重复使用以
//! `NonceAlreadyUsed` revert。SDK 只负责分配「大概率没用过」的 nonce，
//! **不追踪已用集**（合约本身就是权威）—— 因此 trait 只有 `allocate` 一个方法，
//! 没有 `mark_used` 钩子。
//!
//! 默认实现 `UuidNonceProvider` 用 UUID v4 → U256：
//! - 128 位纯随机，碰撞概率约 2⁻¹²⁸（实际可视为零）
//! - 无状态，多副本部署 / 进程重启都不会撞车
//! - 不需要外部存储，即开即用
//!
//! 如需自定义（递增计数、外部 KMS、Redis 中心化分配等），实现 `NonceProvider`
//! 后通过 `EvmSessionMethod::with_nonce_provider(...)` 注入即可。

use alloy_primitives::{Address, B256, U256};
use async_trait::async_trait;

use crate::error::SaApiError;

/// Nonce 分配 trait。
///
/// 单一方法 `allocate`，返回一个**当前 (payee, channel_id) 三元组下未使用过**
/// 的 uint256。是否「未使用过」由实现方保证（默认 UUID 随机方案靠概率，
/// 持久化方案应查询已用记录）。
#[async_trait]
pub trait NonceProvider: Send + Sync {
    /// 为给定 `(payee, channel_id)` 分配一个 nonce。
    ///
    /// 实现方应保证返回值在该 key 下未被消费过。失败时（例如外部存储不可用）
    /// 返回 `SaApiError`，调用方将停止后续 settle / close 流程。
    async fn allocate(&self, payee: Address, channel_id: B256) -> Result<U256, SaApiError>;
}

/// 默认实现：UUID v4 编码为 U256（高 128 位补零，低 128 位为 UUID 字节）。
///
/// 适合绝大多数场景：单进程 / 多副本 / 进程重启都安全（无状态）。
/// 不适合「需要确定性序号 / 审计已用 nonce」的场景，那种需要自实现持久化版本。
#[derive(Debug, Default, Clone)]
pub struct UuidNonceProvider;

#[async_trait]
impl NonceProvider for UuidNonceProvider {
    async fn allocate(&self, _payee: Address, _channel_id: B256) -> Result<U256, SaApiError> {
        Ok(U256::from_be_slice(uuid::Uuid::new_v4().as_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[tokio::test]
    async fn uuid_provider_returns_distinct_nonces() {
        let provider = UuidNonceProvider;
        let payee = Address::from([0x11u8; 20]);
        let channel_id = B256::from([0x22u8; 32]);

        // 1000 次循环不允许碰撞。理论碰撞概率约 2⁻¹¹⁸（生日悖论，1000²/2¹²⁸），
        // 实际可视为零。如果这个测试失败，要么 UUID 实现坏了，要么宇宙射线击中。
        let mut seen = HashSet::new();
        for _ in 0..1000 {
            let nonce = provider.allocate(payee, channel_id).await.unwrap();
            assert!(seen.insert(nonce), "duplicate nonce: {nonce}");
        }
    }

    #[tokio::test]
    async fn uuid_provider_ignores_payee_channel_input() {
        // 默认实现是无状态的，input 不影响输出（除了避免实现方误用）。
        // 不同 (payee, channel_id) 也产出独立随机值。
        let provider = UuidNonceProvider;
        let payee_a = Address::from([0x11u8; 20]);
        let payee_b = Address::from([0x22u8; 20]);
        let cid_a = B256::from([0x33u8; 32]);
        let cid_b = B256::from([0x44u8; 32]);

        let n1 = provider.allocate(payee_a, cid_a).await.unwrap();
        let n2 = provider.allocate(payee_b, cid_b).await.unwrap();
        assert_ne!(n1, n2);
    }

    #[tokio::test]
    async fn uuid_nonce_fits_in_lower_128_bits() {
        // UuidNonceProvider 的 nonce 必然 < 2^128（UUID 是 16 字节，
        // 填充到 U256 低 128 位）。这点对合约无影响（合约接受任意 uint256），
        // 但便于 SDK 端日志可读和上限检查。
        let provider = UuidNonceProvider;
        let nonce = provider
            .allocate(Address::ZERO, B256::ZERO)
            .await
            .unwrap();
        let upper_bound = U256::from(1u64) << 128;
        assert!(nonce < upper_bound);
    }
}
