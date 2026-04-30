//! EIP-712 SettleAuthorization / CloseAuthorization signing for the payee SDK.
//!
//! Mirrors mpp-rs `protocol/methods/tempo/voucher.rs` 的设计：
//! - 用 `alloy::sol!` 定义 typed struct
//! - `eip712_signing_hash` 算 digest（EIP-712 单一真源）
//! - `Signer` trait 注入，私钥/KMS/Ledger 由调用方提供，SDK 不持有

use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_signer::Signer;
use alloy_sol_types::{sol, SolStruct};

use super::domain::{build_domain, DomainMeta};
use crate::error::SaApiError;

sol! {
    /// EIP-712 typed struct, 与合约 `SettleAuthorization` 1:1 对齐。
    /// 签名人 = `channel.payee`。
    #[derive(Debug)]
    struct SettleAuthorization {
        bytes32 channelId;
        uint128 cumulativeAmount;
        uint256 nonce;
        uint256 deadline;
    }

    /// EIP-712 typed struct, 与合约 `CloseAuthorization` 1:1 对齐。
    /// 签名人 = `channel.payee`，与 SettleAuthorization 共享 `(payee, channelId, nonce)` 已用集。
    #[derive(Debug)]
    struct CloseAuthorization {
        bytes32 channelId;
        uint128 cumulativeAmount;
        uint256 nonce;
        uint256 deadline;
    }
}

/// 已签名的 SettleAuthorization / CloseAuthorization 输出。
///
/// `signature` 为 65 字节 `(r, s, v)` 标准格式，禁用 EIP-2098 紧凑 64 字节。
/// 由 alloy `Signer::sign_hash` 自动产出该格式，high-s 由合约层拒绝。
#[derive(Debug, Clone)]
pub struct SignedAuthorization {
    pub channel_id: B256,
    pub cumulative_amount: u128,
    pub nonce: U256,
    pub deadline: U256,
    pub signature: Bytes,
}

/// 共用签名路径:`Signer.sign_hash(digest)` + 包成 `SignedAuthorization`。
/// `label` 仅用于错误信息,无业务意义。
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

/// 用注入的 Signer 签 SettleAuthorization。
///
/// `meta` 指定 EIP-712 domain 的 `name` / `version`(默认走 OKX 标准值,见
/// [`DomainMeta::default`])。商户 fork 合约改了 domain 的话需传自定义 meta。
///
/// Signer 来源由调用方决定:
/// - dev: `PrivateKeySigner::random()` / `from_str(env_var)`
/// - 生产: KMS（如 `alloy_signer_aws::AwsSigner`）/ 硬件钱包（`alloy_signer_ledger::LedgerSigner`）
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

/// 用注入的 Signer 签 CloseAuthorization。结构对称 `sign_settle_authorization`，
/// 只换 typed struct 类型；共享 `sign_with_digest` 完成实际签名。
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

    fn fixture_signer() -> PrivateKeySigner {
        "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318"
            .parse()
            .unwrap()
    }

    #[tokio::test]
    async fn sign_settle_authorization_produces_65_byte_sig() {
        let signer = fixture_signer();
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
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
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
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

    /// SettleAuth 和 CloseAuth 是不同的 typed struct（typehash 不同），
    /// 同样输入下产出的签名必然不同 —— 防止跨类型重用 nonce 时被替换攻击。
    #[tokio::test]
    async fn settle_and_close_signatures_differ_for_same_inputs() {
        let signer = fixture_signer();
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
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
            "Settle 和 Close typed struct 不同，签名必须不同"
        );
    }

    /// 同一签名人对相同输入签名是确定性的（ECDSA + RFC 6979）。
    #[tokio::test]
    async fn deterministic_signature_for_identical_input() {
        let signer = fixture_signer();
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
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

    /// 不同 DomainMeta(自定义 name/version)产出的签名跟 default 不同。
    #[tokio::test]
    async fn custom_meta_yields_different_signature() {
        let signer = fixture_signer();
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
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
