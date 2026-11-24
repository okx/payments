//! AccessProof verification (Seller side).
//!
//! Verifies an `APP-Access` header: `kind` == "subscription-id", ±window replay
//! guard, and `ecrecover(eip191(inner)) == payer` where
//! `inner = keccak256(abi.encodePacked(subId, payer, timestamp))`.
//! Does not check subscription state; a verified proof only binds `(subId, payer)`.

use std::str::FromStr;

use alloy_primitives::{eip191_hash_message, keccak256, Address, Signature, B256, U256};

use x402_core::error::X402Error;
use x402_core::subscription::{decode_access_proof, AccessProof};

/// Byte width `timestamp` is packed as in `abi.encodePacked`; must match the
/// buyer's encoder byte-for-byte (uint256 = 32, uint64 = 8, uint32 = 4).
const ACCESS_PROOF_TIMESTAMP_BYTES: usize = 32;

const ACCESS_PROOF_KIND: &str = "subscription-id";

/// Default replay window: ±300 seconds.
pub const DEFAULT_ACCESS_PROOF_WINDOW_SECS: u64 = 300;

/// A verified AccessProof binding `(subId, payer)`.
#[derive(Debug, Clone)]
pub struct VerifiedAccess {
    pub sub_id: String,
    pub payer: String,
}

fn parse_addr(s: &str) -> Result<Address, X402Error> {
    Address::from_str(s.trim()).map_err(|_| X402Error::Other(format!("invalid address: {s}")))
}

/// `inner = keccak256(abi.encodePacked(subId[32], payer[20], timestamp[width]))`.
/// `abi.encodePacked` uses raw bytes: address is 20 bytes, not padded.
fn access_proof_inner_hash(sub_id: &str, payer: &str, timestamp: u64) -> Result<B256, X402Error> {
    let sub = B256::from_str(sub_id.trim())
        .map_err(|_| X402Error::Other(format!("invalid subId bytes32: {sub_id}")))?;
    let addr = parse_addr(payer)?;
    let ts = U256::from(timestamp).to_be_bytes::<32>();

    let mut buf = Vec::with_capacity(32 + 20 + ACCESS_PROOF_TIMESTAMP_BYTES);
    buf.extend_from_slice(sub.as_slice());
    buf.extend_from_slice(addr.as_slice());
    buf.extend_from_slice(&ts[32 - ACCESS_PROOF_TIMESTAMP_BYTES..]);
    Ok(keccak256(&buf))
}

fn parse_signature(sig: &str) -> Result<Signature, X402Error> {
    let bytes = alloy_primitives::hex::decode(sig.trim().trim_start_matches("0x"))
        .map_err(|e| X402Error::Other(format!("invalid signature hex: {e}")))?;
    Signature::try_from(bytes.as_slice())
        .map_err(|e| X402Error::Other(format!("invalid signature: {e}")))
}

/// Verify a decoded AccessProof (kind + window + `ecrecover == payer`).
pub fn verify_access_proof_decoded(
    proof: &AccessProof,
    now_unix: u64,
    window_secs: u64,
) -> Result<VerifiedAccess, X402Error> {
    if proof.kind != ACCESS_PROOF_KIND {
        return Err(X402Error::Other(format!(
            "AccessProof: unexpected kind {:?}",
            proof.kind
        )));
    }
    if now_unix.abs_diff(proof.timestamp) > window_secs {
        return Err(X402Error::Other(
            "AccessProof: timestamp outside ±window (replay guard)".into(),
        ));
    }

    let inner = access_proof_inner_hash(&proof.sub_id, &proof.payer, proof.timestamp)?;
    let msg_hash = eip191_hash_message(inner.as_slice());
    let sig = parse_signature(&proof.signature)?;
    let recovered = sig
        .recover_address_from_prehash(&msg_hash)
        .map_err(|e| X402Error::Other(format!("AccessProof: ecrecover failed: {e}")))?;

    let payer = parse_addr(&proof.payer)?;
    if recovered != payer {
        return Err(X402Error::Other(
            "AccessProof: signature does not match payer".into(),
        ));
    }
    Ok(VerifiedAccess {
        sub_id: proof.sub_id.clone(),
        payer: proof.payer.clone(),
    })
}

/// Decode (base64 JSON) + verify an `APP-Access` header value.
pub fn verify_access_proof(
    header_value: &str,
    now_unix: u64,
    window_secs: u64,
) -> Result<VerifiedAccess, X402Error> {
    let proof = decode_access_proof(header_value)?;
    verify_access_proof_decoded(&proof, now_unix, window_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::SigningKey;

    const SUB_ID: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";

    /// secp256k1 public key → Ethereum address (keccak of the 64-byte pubkey).
    fn address_of(sk: &SigningKey) -> Address {
        let vk = sk.verifying_key();
        let point = vk.to_encoded_point(false); // 0x04 || X(32) || Y(32)
        Address::from_slice(&keccak256(&point.as_bytes()[1..])[12..])
    }

    /// Produce a valid AccessProof signed by `sk` (EIP-191 over the inner hash).
    fn make_proof(sk: &SigningKey, sub_id: &str, ts: u64) -> AccessProof {
        let payer = format!("{:#x}", address_of(sk));
        let inner = access_proof_inner_hash(sub_id, &payer, ts).unwrap();
        let prehash = eip191_hash_message(inner.as_slice());
        let (sig, recid) = sk.sign_prehash_recoverable(prehash.as_slice()).unwrap();
        let mut bytes = sig.to_bytes().to_vec(); // 64 bytes r||s
        bytes.push(27 + recid.to_byte()); // v = 27/28
        AccessProof {
            kind: ACCESS_PROOF_KIND.to_string(),
            sub_id: sub_id.to_string(),
            payer,
            timestamp: ts,
            signature: format!("0x{}", alloy_primitives::hex::encode(bytes)),
        }
    }

    #[test]
    fn round_trip_recovers_payer() {
        let sk = SigningKey::from_slice(&[0x11u8; 32]).unwrap();
        let proof = make_proof(&sk, SUB_ID, 1_747_200_000);
        let v = verify_access_proof_decoded(&proof, 1_747_200_010, 300).unwrap();
        assert_eq!(v.sub_id, SUB_ID);
        assert_eq!(
            v.payer.to_lowercase(),
            format!("{:#x}", address_of(&sk))
        );
    }

    #[test]
    fn rejects_timestamp_outside_window() {
        let sk = SigningKey::from_slice(&[0x22u8; 32]).unwrap();
        let proof = make_proof(&sk, SUB_ID, 1_000_000);
        let err = verify_access_proof_decoded(&proof, 1_000_000 + 301, 300).unwrap_err();
        assert!(format!("{err}").contains("window"));
    }

    #[test]
    fn rejects_wrong_kind() {
        let sk = SigningKey::from_slice(&[0x33u8; 32]).unwrap();
        let mut proof = make_proof(&sk, SUB_ID, 1_747_200_000);
        proof.kind = "something-else".to_string();
        assert!(verify_access_proof_decoded(&proof, 1_747_200_000, 300).is_err());
    }

    #[test]
    fn rejects_tampered_payer() {
        // Re-pointing payer changes the inner hash, so recovery no longer matches.
        let sk = SigningKey::from_slice(&[0x44u8; 32]).unwrap();
        let mut proof = make_proof(&sk, SUB_ID, 1_747_200_000);
        proof.payer = "0x000000000000000000000000000000000000dEaD".to_string();
        assert!(verify_access_proof_decoded(&proof, 1_747_200_000, 300).is_err());
    }
}
