//! Nonce allocation trait + default UUID v4 implementation.
//!
//! The contract's used-nonce set is keyed by `(payee, channelId, nonce)`;
//! reuse reverts with `NonceAlreadyUsed`. The SDK is only responsible for
//! allocating a "very likely unused" nonce — it **does not track the used
//! set** (the contract is the source of truth). The trait therefore has a
//! single `allocate` method, with no `mark_used` hook.
//!
//! The default `UuidNonceProvider` encodes UUID v4 as a U256:
//! - 128 bits of pure randomness, collision probability ~2⁻¹²⁸ (effectively zero).
//! - Stateless — safe across replicas and restarts.
//! - No external storage required.
//!
//! For custom strategies (incrementing counter, external KMS, central
//! Redis allocator, ...), implement `NonceProvider` and inject via
//! `EvmSessionMethod::with_nonce_provider(...)`.

use alloy_primitives::{Address, B256, U256};
use async_trait::async_trait;

use crate::error::SaApiError;

/// Nonce allocation trait.
///
/// One method, `allocate`, returns a uint256 that **has not been used
/// for the current `(payee, channel_id)` pair**. Implementations decide
/// how to ensure "not used" (the default UUID-random impl relies on
/// probability; a persistent impl should consult its used-set).
#[async_trait]
pub trait NonceProvider: Send + Sync {
    /// Allocate a nonce for the given `(payee, channel_id)`.
    ///
    /// Implementations must guarantee the returned value has not been
    /// consumed under this key. On failure (e.g. external storage
    /// unavailable) return `SaApiError` — the caller will stop the
    /// settle / close flow.
    async fn allocate(&self, payee: Address, channel_id: B256) -> Result<U256, SaApiError>;
}

/// Default implementation: UUID v4 encoded as a U256 (upper 128 bits
/// zero, lower 128 bits = UUID bytes).
///
/// Suitable for nearly every deployment: safe across single-process,
/// multi-replica, and restart scenarios (stateless). Not suitable for
/// "deterministic sequence numbers / auditable used-nonce trail" needs —
/// those require a persistent implementation.
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

        // 1000 iterations must not collide. Birthday-paradox collision
        // probability ~2⁻¹¹⁸ (1000² / 2¹²⁸) — effectively zero. If this
        // test fails, either the UUID impl is broken or you got hit by a
        // cosmic ray.
        let mut seen = HashSet::new();
        for _ in 0..1000 {
            let nonce = provider.allocate(payee, channel_id).await.unwrap();
            assert!(seen.insert(nonce), "duplicate nonce: {nonce}");
        }
    }

    #[tokio::test]
    async fn uuid_provider_ignores_payee_channel_input() {
        // The default impl is stateless — inputs don't influence outputs
        // (kept in the signature only to keep the trait extensible).
        // Different (payee, channel_id) pairs still produce independent
        // random values.
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
        // `UuidNonceProvider` nonces always fit in the lower 128 bits
        // (UUID is 16 bytes, packed into the lower half of the U256).
        // The contract accepts any uint256, but this keeps SDK logs
        // readable and makes upper-bound checks easy.
        let provider = UuidNonceProvider;
        let nonce = provider.allocate(Address::ZERO, B256::ZERO).await.unwrap();
        let upper_bound = U256::from(1u64) << 128;
        assert!(nonce < upper_bound);
    }
}
