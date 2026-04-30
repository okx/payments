//! Local EIP-712 Voucher verification.
//!
//! Used by:
//! - The payee SDK to verify vouchers from the payer / authorizedSigner
//!   over the HTTP 402 flow (step H of `submit_voucher`'s 9-step guards).
//! - `ACTION_OPEN` to verify the initial voucher (when present).
//! - `ACTION_CLOSE` B-1 path to verify the final payer-submitted voucher.
//!
//! Design notes:
//! 1. Strict 65-byte length (rejects EIP-2098 compact form), matching the contract.
//! 2. Explicit low-s precheck (s <= secp256k1_order / 2), borrowed from Java `Eip712VerifyUtil`.
//! 3. EIP-712 encoding via `alloy::sol!` + `eip712_signing_hash` — single source of truth.
//! 4. ecrecover + strict address comparison (`Address` is naturally case-insensitive).

use alloy_primitives::{Address, U256};
use alloy_signer::Signature;
use alloy_sol_types::{sol, SolStruct};

use super::domain::{build_domain, DomainMeta};

/// Half the secp256k1 curve order (N/2). `s > N/2` is high-s
/// (malleable signature). Borrowed from Java `Eip712VerifyUtil.SECP256K1_ORDER_HALF`.
const SECP256K1_HALF_N: U256 = U256::from_limbs([
    0xDFE9_2F46_681B_20A0,
    0x5D57_6E73_57A4_501D,
    0xFFFF_FFFF_FFFF_FFFF,
    0x7FFF_FFFF_FFFF_FFFF,
]);

sol! {
    /// EIP-712 typed struct; must match the OKX EvmPaymentChannel contract 1:1.
    #[derive(Debug)]
    struct Voucher {
        bytes32 channelId;
        uint128 cumulativeAmount;
    }
}

/// Locally verify a Voucher. Returns `Ok(())` when the signature is
/// valid and `recovered == expected_signer`.
///
/// `meta` selects the EIP-712 domain's `name` / `version` (defaults to
/// the OKX canonical values — see [`DomainMeta::default`]). Pass a custom
/// meta when the merchant has forked the contract with a different domain.
///
/// # Guard order
/// 1. Strict 65-byte length.
/// 2. Low-s precheck.
/// 3. EIP-712 digest computation.
/// 4. ecrecover + strict address comparison.
pub fn verify_voucher(
    meta: &DomainMeta,
    escrow_contract: Address,
    chain_id: u64,
    channel_id: alloy_primitives::B256,
    cumulative_amount: u128,
    signature: &[u8],
    expected_signer: Address,
) -> Result<(), VerifyError> {
    // (1) Strict 65-byte length (rejects EIP-2098 compact 64-byte form).
    if signature.len() != 65 {
        return Err(VerifyError::BadLength(signature.len()));
    }

    // (2) Low-s precheck: s must be <= secp256k1_order / 2; otherwise malleable.
    let s = U256::from_be_slice(&signature[32..64]);
    if s > SECP256K1_HALF_N {
        return Err(VerifyError::HighS);
    }

    // (3) EIP-712 digest (sol! + eip712_signing_hash).
    let domain = build_domain(meta, chain_id, escrow_contract);
    let voucher = Voucher {
        channelId: channel_id,
        cumulativeAmount: cumulative_amount,
    };
    let digest = voucher.eip712_signing_hash(&domain);

    // (4) ecrecover + strict address comparison.
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

/// Detailed error types for local verification — useful for diagnosing
/// production failures.
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
        // Fixed key — keeps round-trip tests reproducible.
        "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318"
            .parse()
            .unwrap()
    }

    fn fixture_escrow() -> Address {
        address!("5E550002e64FaF79B41D89fE8439eEb1be66CE3b")
    }

    fn fixture_channel_id() -> alloy_primitives::B256 {
        b256!("6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f")
    }

    /// Helper: sign a Voucher with the given signer, returning 65 bytes.
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
        // Build a signature where s exceeds N/2.
        let mut sig_bytes = vec![0u8; 65];
        // r: any non-zero value.
        sig_bytes[0] = 0x01;
        // s = secp256k1_order_half + 1 (all-0xff bytes are guaranteed > N/2).
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
        // r = 0 is an invalid ECDSA signature → Signature::try_from or recover fails.
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

        // Sign with cum=100 but verify with cum=200.
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
        // Sign with custom meta, verify with the same custom meta → pass.
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
        // Sign with custom meta, verify with default meta → AddressMismatch.
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
