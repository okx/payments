//! EIP-712 SettleAuthorization / CloseAuthorization signing for the payee SDK.
//!
//! Mirrors the design of mpp-rs `protocol/methods/tempo/voucher.rs`:
//! - Typed structs defined via `alloy::sol!`.
//! - `eip712_signing_hash` computes the digest (single EIP-712 source of truth).
//! - `Signer` trait injection — private keys / KMS / Ledger come from
//!   the caller; the SDK never holds them.

use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_signer::Signer;
use alloy_sol_types::{sol, SolStruct};

use super::domain::{build_domain, DomainMeta};
use crate::error::SaApiError;

sol! {
    /// EIP-712 typed struct; matches the contract's `SettleAuthorization` 1:1.
    /// Signer = `channel.payee`.
    #[derive(Debug)]
    struct SettleAuthorization {
        bytes32 channelId;
        uint128 cumulativeAmount;
        uint256 nonce;
        uint256 deadline;
    }

    /// EIP-712 typed struct; matches the contract's `CloseAuthorization` 1:1.
    /// Signer = `channel.payee`. Shares the `(payee, channelId, nonce)`
    /// "used" set with SettleAuthorization.
    #[derive(Debug)]
    struct CloseAuthorization {
        bytes32 channelId;
        uint128 cumulativeAmount;
        uint256 nonce;
        uint256 deadline;
    }
}

/// Signed SettleAuthorization / CloseAuthorization output.
///
/// `signature` is the standard 65-byte `(r, s, v)` format; EIP-2098
/// compact 64-byte form is not allowed. alloy's `Signer::sign_hash`
/// produces this format, and the contract layer rejects high-s.
#[derive(Debug, Clone)]
pub struct SignedAuthorization {
    pub channel_id: B256,
    pub cumulative_amount: u128,
    pub nonce: U256,
    pub deadline: U256,
    pub signature: Bytes,
}

/// Shared signing path: `Signer.sign_hash(digest)` + wrap into
/// `SignedAuthorization`. `label` is for error messages only.
async fn sign_with_digest(
    signer: &(impl Signer + ?Sized),
    digest: alloy_primitives::B256,
    label: &'static str,
    channel_id: B256,
    cumulative_amount: u128,
    nonce: U256,
    deadline: U256,
) -> Result<SignedAuthorization, SaApiError> {
    let sig = signer
        .sign_hash(&digest)
        .await
        .map_err(|e| SaApiError::new(8000, format!("sign {label}: {e}")))?;
    Ok(SignedAuthorization {
        channel_id,
        cumulative_amount,
        nonce,
        deadline,
        signature: Bytes::from(sig.as_bytes().to_vec()),
    })
}

/// Sign a SettleAuthorization with the injected Signer.
///
/// `meta` selects the EIP-712 domain `name` / `version` (defaults to the
/// OKX canonical values — see [`DomainMeta::default`]). Pass a custom
/// meta if the merchant has forked the contract with a different domain.
///
/// The signer comes from the caller:
/// - dev: `PrivateKeySigner::random()` / `from_str(env_var)`
/// - prod: KMS (`alloy_signer_aws::AwsSigner`) or hardware wallets (`alloy_signer_ledger::LedgerSigner`)
pub async fn sign_settle_authorization(
    meta: &DomainMeta,
    signer: &(impl Signer + ?Sized),
    escrow_contract: Address,
    chain_id: u64,
    channel_id: B256,
    cumulative_amount: u128,
    nonce: U256,
    deadline: U256,
) -> Result<SignedAuthorization, SaApiError> {
    let domain = build_domain(meta, chain_id, escrow_contract);
    let digest = SettleAuthorization {
        channelId: channel_id,
        cumulativeAmount: cumulative_amount,
        nonce,
        deadline,
    }
    .eip712_signing_hash(&domain);
    sign_with_digest(
        signer,
        digest,
        "SettleAuthorization",
        channel_id,
        cumulative_amount,
        nonce,
        deadline,
    )
    .await
}

/// Sign a CloseAuthorization with the injected Signer. Symmetric to
/// `sign_settle_authorization` — only the typed struct differs; both
/// share `sign_with_digest` for the actual signing.
pub async fn sign_close_authorization(
    meta: &DomainMeta,
    signer: &(impl Signer + ?Sized),
    escrow_contract: Address,
    chain_id: u64,
    channel_id: B256,
    cumulative_amount: u128,
    nonce: U256,
    deadline: U256,
) -> Result<SignedAuthorization, SaApiError> {
    let domain = build_domain(meta, chain_id, escrow_contract);
    let digest = CloseAuthorization {
        channelId: channel_id,
        cumulativeAmount: cumulative_amount,
        nonce,
        deadline,
    }
    .eip712_signing_hash(&domain);
    sign_with_digest(
        signer,
        digest,
        "CloseAuthorization",
        channel_id,
        cumulative_amount,
        nonce,
        deadline,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, b256};
    use alloy_signer_local::PrivateKeySigner;

    /// **PUBLICLY KNOWN** fixture key from the web3.js documentation —
    /// safe ONLY in tests. Never copy into production / deployment configs:
    /// the corresponding address is widely scraped and any funds sent to
    /// it will be drained immediately. Tests use it for determinism.
    const PUBLIC_TEST_KEY: &str =
        "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318";

    fn fixture_signer() -> PrivateKeySigner {
        PUBLIC_TEST_KEY.parse().unwrap()
    }

    #[tokio::test]
    async fn sign_settle_authorization_produces_65_byte_sig() {
        let signer = fixture_signer();
        let escrow = address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b");
        let channel_id = b256!("6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f");

        let signed = sign_settle_authorization(
            &DomainMeta::default(),
            &signer,
            escrow,
            196,
            channel_id,
            1_000_000u128,
            U256::from(42u64),
            U256::from(1_800_000_000u64),
        )
        .await
        .expect("sign succeeds");

        assert_eq!(signed.signature.len(), 65);
        assert_eq!(signed.channel_id, channel_id);
        assert_eq!(signed.cumulative_amount, 1_000_000);
        assert_eq!(signed.nonce, U256::from(42u64));
    }

    #[tokio::test]
    async fn sign_close_authorization_produces_65_byte_sig() {
        let signer = fixture_signer();
        let escrow = address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b");
        let channel_id = b256!("6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f");

        let signed = sign_close_authorization(
            &DomainMeta::default(),
            &signer,
            escrow,
            196,
            channel_id,
            500_000u128,
            U256::from(99u64),
            U256::MAX,
        )
        .await
        .expect("sign succeeds");

        assert_eq!(signed.signature.len(), 65);
        assert_eq!(signed.deadline, U256::MAX);
    }

    /// SettleAuth and CloseAuth are distinct typed structs (different
    /// typehashes), so identical inputs must yield different signatures —
    /// preventing cross-type substitution attacks when reusing a nonce.
    #[tokio::test]
    async fn settle_and_close_signatures_differ_for_same_inputs() {
        let signer = fixture_signer();
        let escrow = address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b");
        let channel_id = b256!("6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f");
        let nonce = U256::from(7u64);
        let deadline = U256::from(1_800_000_000u64);
        let meta = DomainMeta::default();

        let s1 = sign_settle_authorization(
            &meta, &signer, escrow, 196, channel_id, 100, nonce, deadline,
        )
        .await
        .unwrap();
        let s2 = sign_close_authorization(
            &meta, &signer, escrow, 196, channel_id, 100, nonce, deadline,
        )
        .await
        .unwrap();

        assert_ne!(
            s1.signature, s2.signature,
            "Settle and Close are distinct typed structs; signatures must differ"
        );
    }

    /// A single signer over identical inputs is deterministic (ECDSA + RFC 6979).
    #[tokio::test]
    async fn deterministic_signature_for_identical_input() {
        let signer = fixture_signer();
        let escrow = address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b");
        let channel_id = b256!("6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f");
        let meta = DomainMeta::default();

        let s1 = sign_settle_authorization(
            &meta,
            &signer,
            escrow,
            196,
            channel_id,
            42,
            U256::from(1u64),
            U256::from(100u64),
        )
        .await
        .unwrap();
        let s2 = sign_settle_authorization(
            &meta,
            &signer,
            escrow,
            196,
            channel_id,
            42,
            U256::from(1u64),
            U256::from(100u64),
        )
        .await
        .unwrap();

        assert_eq!(s1.signature, s2.signature);
    }

    /// A different DomainMeta (custom name / version) must produce a
    /// different signature than the default.
    #[tokio::test]
    async fn custom_meta_yields_different_signature() {
        let signer = fixture_signer();
        let escrow = address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b");
        let channel_id = b256!("6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f");

        let default = DomainMeta::default();
        let custom = DomainMeta::new("Forked Channel", "2");
        let nonce = U256::from(1u64);
        let deadline = U256::from(100u64);

        let s_default = sign_settle_authorization(
            &default, &signer, escrow, 196, channel_id, 42, nonce, deadline,
        )
        .await
        .unwrap();
        let s_custom = sign_settle_authorization(
            &custom, &signer, escrow, 196, channel_id, 42, nonce, deadline,
        )
        .await
        .unwrap();

        assert_ne!(s_default.signature, s_custom.signature);
    }
}
