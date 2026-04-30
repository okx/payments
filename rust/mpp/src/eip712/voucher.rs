//! Local EIP-712 Voucher verification.
//!
//! 用于：
//! - Payee SDK 验 Payer / authorizedSigner 在 HTTP 402 流程中送来的 voucher
//!   （`submit_voucher` 9 步守卫的 H 步）
//! - ACTION_OPEN 时验 Payer 提供的初始 voucher（如有）
//! - ACTION_CLOSE B-1 路径里验 Payer 提交的最终 voucher
//!
//! 设计要点：
//! 1. 长度严格 65 字节（拒 EIP-2098 紧凑），对齐合约
//! 2. Low-s 显式预检（s ≤ secp256k1_order/2），从 Java Eip712VerifyUtil 借
//! 3. EIP-712 编码用 `alloy::sol!` 宏 + `eip712_signing_hash`，单一真源
//! 4. ecrecover + 严格地址比对（Address 类型天然大小写无关）

use alloy_primitives::{Address, U256};
use alloy_signer::Signature;
use alloy_sol_types::{sol, SolStruct};

use super::domain::{build_domain, DomainMeta};

/// secp256k1 曲线阶的一半：N/2。s > 此值即视为 high-s（malleable signature）。
/// 借自 Java `Eip712VerifyUtil.SECP256K1_ORDER_HALF`。
const SECP256K1_HALF_N: U256 = U256::from_limbs([
    0xDFE9_2F46_681B_20A0,
    0x5D57_6E73_57A4_501D,
    0xFFFF_FFFF_FFFF_FFFF,
    0x7FFF_FFFF_FFFF_FFFF,
]);

sol! {
    /// EIP-712 typed struct，必须与 OKX EvmPaymentChannel 合约 1:1 对齐。
    #[derive(Debug)]
    struct Voucher {
        bytes32 channelId;
        uint128 cumulativeAmount;
    }
}

/// 本地验签 Voucher。返回 `Ok(())` 表示签名有效且 `recovered == expected_signer`。
///
/// `meta` 指定 EIP-712 domain 的 `name` / `version`(默认走 OKX 标准值,见
/// [`DomainMeta::default`])。商户 fork 合约改了 domain 的话需传自定义 meta。
///
/// # 守卫顺序
/// 1. 长度严格 65 字节
/// 2. Low-s 预检
/// 3. EIP-712 digest 计算
/// 4. ecrecover + 严格地址比对
pub fn verify_voucher(
    meta: &DomainMeta,
    escrow_contract: Address,
    chain_id: u64,
    channel_id: alloy_primitives::B256,
    cumulative_amount: u128,
    signature: &[u8],
    expected_signer: Address,
) -> Result<(), VerifyError> {
    // ① 长度严格 65 字节（拒 EIP-2098 紧凑 64 字节格式）
    if signature.len() != 65 {
        return Err(VerifyError::BadLength(signature.len()));
    }

    // ② Low-s 预检：s 必须 ≤ secp256k1_order / 2，否则视为 malleable
    let s = U256::from_be_slice(&signature[32..64]);
    if s > SECP256K1_HALF_N {
        return Err(VerifyError::HighS);
    }

    // ③ EIP-712 digest（sol! + eip712_signing_hash）
    let domain = build_domain(meta, chain_id, escrow_contract);
    let voucher = Voucher {
        channelId: channel_id,
        cumulativeAmount: cumulative_amount,
    };
    let digest = voucher.eip712_signing_hash(&domain);

    // ④ ecrecover + 严格地址比对
    let sig = Signature::try_from(signature).map_err(|_| VerifyError::SignatureParse)?;
    let recovered = sig
        .recover_address_from_prehash(&digest)
        .map_err(|_| VerifyError::Recover)?;
    if recovered != expected_signer {
        return Err(VerifyError::AddressMismatch {
            recovered,
            expected: expected_signer,
        });
    }
    Ok(())
}

/// 本地验签的细分错误类型，方便生产环境定位根因。
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum VerifyError {
    #[error("signature must be 65 bytes, got {0}")]
    BadLength(usize),

    #[error("non-canonical signature: s exceeds secp256k1 half-order (high-s)")]
    HighS,

    #[error("signature parse failed (cannot construct Signature from bytes)")]
    SignatureParse,

    #[error("ecrecover failed (cannot recover signer from prehash)")]
    Recover,

    #[error("signer mismatch: recovered {recovered}, expected {expected}")]
    AddressMismatch { recovered: Address, expected: Address },
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, b256};
    use alloy_signer::SignerSync;
    use alloy_signer_local::PrivateKeySigner;

    fn fixture_signer() -> PrivateKeySigner {
        // 固定 key，便于 round-trip 测试可重现
        "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318"
            .parse()
            .unwrap()
    }

    fn fixture_escrow() -> Address {
        address!("eb18025208061781a287fFc2c1F31C03A24a24c0")
    }

    fn fixture_channel_id() -> alloy_primitives::B256 {
        b256!("6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f")
    }

    /// 帮助函数：用给定 signer 签 Voucher，返回 65 字节签名
    fn sign_voucher_for_test(
        signer: &PrivateKeySigner,
        escrow: Address,
        chain_id: u64,
        channel_id: alloy_primitives::B256,
        cum: u128,
    ) -> Vec<u8> {
        let domain = build_domain(&DomainMeta::default(), chain_id, escrow);
        let voucher = Voucher {
            channelId: channel_id,
            cumulativeAmount: cum,
        };
        let digest = voucher.eip712_signing_hash(&domain);
        let sig: Signature = signer.sign_hash_sync(&digest).unwrap();
        sig.as_bytes().to_vec()
    }

    #[test]
    fn round_trip_sign_then_verify() {
        let signer = fixture_signer();
        let signer_addr = signer.address();
        let escrow = fixture_escrow();
        let channel_id = fixture_channel_id();
        let cum: u128 = 1_000_000;

        let meta = DomainMeta::default();
        let sig = sign_voucher_for_test(&signer, escrow, 196, channel_id, cum);
        let result = verify_voucher(&meta, escrow, 196, channel_id, cum, &sig, signer_addr);
        assert!(result.is_ok(), "round trip failed: {result:?}");
    }

    #[test]
    fn wrong_length_returns_bad_length() {
        let signer_addr = fixture_signer().address();
        let result = verify_voucher(
            &DomainMeta::default(),
            fixture_escrow(),
            196,
            fixture_channel_id(),
            1,
            &[0u8; 64], // 64-byte EIP-2098 compact
            signer_addr,
        );
        assert_eq!(result, Err(VerifyError::BadLength(64)));
    }

    #[test]
    fn high_s_signature_returns_high_s() {
        // 构造一个 s 超过 N/2 的签名
        let mut sig_bytes = vec![0u8; 65];
        // r 任意非零
        sig_bytes[0] = 0x01;
        // s = secp256k1_order_half + 1（设最高位为 0xff... 必然 > N/2）
        for i in 32..64 {
            sig_bytes[i] = 0xff;
        }
        sig_bytes[64] = 27;

        let signer_addr = fixture_signer().address();
        let result = verify_voucher(
            &DomainMeta::default(),
            fixture_escrow(),
            196,
            fixture_channel_id(),
            1,
            &sig_bytes,
            signer_addr,
        );
        assert_eq!(result, Err(VerifyError::HighS));
    }

    #[test]
    fn address_mismatch_returns_mismatch_with_recovered_addr() {
        let signer = fixture_signer();
        let escrow = fixture_escrow();
        let channel_id = fixture_channel_id();
        let cum: u128 = 1_000_000;

        let sig = sign_voucher_for_test(&signer, escrow, 196, channel_id, cum);
        let wrong_expected = Address::from([0xaau8; 20]);

        let result = verify_voucher(
            &DomainMeta::default(),
            escrow,
            196,
            channel_id,
            cum,
            &sig,
            wrong_expected,
        );
        match result {
            Err(VerifyError::AddressMismatch { recovered, expected }) => {
                assert_eq!(recovered, signer.address());
                assert_eq!(expected, wrong_expected);
            }
            other => panic!("expected AddressMismatch, got {other:?}"),
        }
    }

    #[test]
    fn corrupted_signature_returns_parse_or_recover_err() {
        // r = 0 是无效 ECDSA 签名 → Signature::try_from 或 recover 失败
        let signer_addr = fixture_signer().address();
        let result = verify_voucher(
            &DomainMeta::default(),
            fixture_escrow(),
            196,
            fixture_channel_id(),
            1,
            &[0u8; 65],
            signer_addr,
        );
        assert!(matches!(
            result,
            Err(VerifyError::SignatureParse) | Err(VerifyError::Recover)
        ));
    }

    #[test]
    fn different_cumulative_amount_fails_verify() {
        let signer = fixture_signer();
        let signer_addr = signer.address();
        let escrow = fixture_escrow();
        let channel_id = fixture_channel_id();

        // 用 cum=100 签，但用 cum=200 验
        let sig = sign_voucher_for_test(&signer, escrow, 196, channel_id, 100);
        let result = verify_voucher(
            &DomainMeta::default(),
            escrow,
            196,
            channel_id,
            200,
            &sig,
            signer_addr,
        );
        assert!(matches!(result, Err(VerifyError::AddressMismatch { .. })));
    }

    #[test]
    fn different_channel_id_fails_verify() {
        let signer = fixture_signer();
        let signer_addr = signer.address();
        let escrow = fixture_escrow();
        let cid_a = fixture_channel_id();
        let cid_b = b256!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

        let sig = sign_voucher_for_test(&signer, escrow, 196, cid_a, 100);
        let result = verify_voucher(
            &DomainMeta::default(),
            escrow,
            196,
            cid_b,
            100,
            &sig,
            signer_addr,
        );
        assert!(matches!(result, Err(VerifyError::AddressMismatch { .. })));
    }

    #[test]
    fn custom_meta_works_when_used_consistently() {
        // sign 用 custom meta,verify 用同一个 custom meta → 通过
        let signer = fixture_signer();
        let signer_addr = signer.address();
        let escrow = fixture_escrow();
        let channel_id = fixture_channel_id();
        let cum: u128 = 100;

        let custom = DomainMeta::new("Forked Channel", "2");
        let domain = build_domain(&custom, 196, escrow);
        let voucher = Voucher {
            channelId: channel_id,
            cumulativeAmount: cum,
        };
        let digest = voucher.eip712_signing_hash(&domain);
        let sig = signer.sign_hash_sync(&digest).unwrap().as_bytes().to_vec();

        let result = verify_voucher(&custom, escrow, 196, channel_id, cum, &sig, signer_addr);
        assert!(result.is_ok());
    }

    #[test]
    fn custom_meta_mismatch_fails_verify() {
        // sign 用 custom meta,verify 用 default meta → AddressMismatch
        let signer = fixture_signer();
        let signer_addr = signer.address();
        let escrow = fixture_escrow();
        let channel_id = fixture_channel_id();
        let cum: u128 = 100;

        let custom = DomainMeta::new("Forked Channel", "2");
        let domain = build_domain(&custom, 196, escrow);
        let voucher = Voucher {
            channelId: channel_id,
            cumulativeAmount: cum,
        };
        let digest = voucher.eip712_signing_hash(&domain);
        let sig = signer.sign_hash_sync(&digest).unwrap().as_bytes().to_vec();

        let result = verify_voucher(
            &DomainMeta::default(),
            escrow,
            196,
            channel_id,
            cum,
            &sig,
            signer_addr,
        );
        assert!(matches!(result, Err(VerifyError::AddressMismatch { .. })));
    }
}
