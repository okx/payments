//! Helpers for credential parsing and field extraction shared across the
//! `session_method` submodules. All items are crate-internal (`pub(super)`)
//! and only visible inside `session_method/`.

use alloy_primitives::{hex, Address, Bytes, B256};
use mpp::protocol::core::{Base64UrlJson, PaymentCredential};

use crate::error::SaApiError;
use crate::types::SessionMethodDetails;

/// Session credential action names (spec §8.3).
pub(super) const ACTION_OPEN: &str = "open";
pub(super) const ACTION_VOUCHER: &str = "voucher";
pub(super) const ACTION_TOPUP: &str = "topUp";
pub(super) const ACTION_CLOSE: &str = "close";

/// Read an optional string field. Missing key, non-string value, or
/// `null` all fold to `""`. Use this only for fields that are genuinely
/// optional (e.g. `action` as a dispatch key, where `""` is the
/// no-action default). For any required field, use
/// [`extract_required_str`] so a missing field surfaces as
/// `70000 missing field X` instead of being silently parsed downstream as
/// "cannot parse u128 from empty string".
pub(super) fn extract_str_or_empty<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Read a required string field. Returns `70000 missing field X` when the
/// key is absent, the value is not a string, or the value is `null`.
/// Empty-string values are accepted (some SA payloads use `""` as a
/// sentinel — e.g. close-waiver `voucherSignature`).
pub(super) fn extract_required_str<'a>(
    value: &'a serde_json::Value,
    field: &str,
) -> Result<&'a str, SaApiError> {
    match value.get(field).and_then(|v| v.as_str()) {
        Some(s) => Ok(s),
        None => Err(SaApiError::new(
            70000,
            format!("missing required field {field}"),
        )),
    }
}

pub(super) fn parse_b256(s: &str) -> Result<B256, SaApiError> {
    s.parse::<B256>()
        .map_err(|e| SaApiError::new(70000, format!("invalid bytes32 channelId {s}: {e}")))
}

pub(super) fn parse_address(s: &str) -> Result<Address, SaApiError> {
    s.parse::<Address>()
        .map_err(|e| SaApiError::new(70000, format!("invalid address {s}: {e}")))
}

pub(super) fn parse_u128_str(s: &str) -> Result<u128, SaApiError> {
    s.parse::<u128>()
        .map_err(|e| SaApiError::new(70000, format!("invalid u128 {s}: {e}")))
}

/// Parse an optional u128 field; missing / empty string / null = 0.
pub(super) fn parse_u128_default_zero(v: Option<&serde_json::Value>) -> Result<u128, SaApiError> {
    match v.and_then(|x| x.as_str()) {
        None | Some("") => Ok(0),
        Some(s) => parse_u128_str(s),
    }
}

/// Parse an optional hex-bytes field ("0x..." | "" | null).
pub(super) fn parse_optional_hex_bytes(
    v: Option<&serde_json::Value>,
) -> Result<Option<Bytes>, SaApiError> {
    match v.and_then(|x| x.as_str()) {
        None | Some("") => Ok(None),
        Some(s) => {
            let stripped = s.strip_prefix("0x").unwrap_or(s);
            let bytes = hex::decode(stripped)
                .map_err(|e| SaApiError::new(70000, format!("invalid hex {s}: {e}")))?;
            Ok(Some(Bytes::from(bytes)))
        }
    }
}

pub(super) fn hex_with_prefix(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

/// Decode `challenge.request` (base64url JSON) and extract `recipient`.
pub(super) fn decode_challenge_request_recipient(
    request: &Base64UrlJson,
) -> Result<Address, SaApiError> {
    let value = request
        .decode_value()
        .map_err(|e| SaApiError::new(70000, format!("decode challenge request: {e}")))?;
    let recipient = value
        .get("recipient")
        .and_then(|r| r.as_str())
        .ok_or_else(|| SaApiError::new(70000, "challenge.request missing recipient"))?;
    parse_address(recipient)
}

/// Decode `method_details` JSON into [`SessionMethodDetails`].
pub(super) fn decode_method_details(
    method_details: Option<&serde_json::Value>,
) -> Result<SessionMethodDetails, SaApiError> {
    let v = method_details.ok_or_else(|| SaApiError::new(8000, "method_details not configured"))?;
    serde_json::from_value(v.clone())
        .map_err(|e| SaApiError::new(70000, format!("invalid method_details: {e}")))
}

/// Strictly parse a `did:pkh:eip155:<chainId>:<address>` DID per spec and
/// return the address segment.
///
/// Checks (matching mpp-rs `parse_proof_source`):
/// - Prefix must be `did:pkh:eip155:` (method = pkh, namespace = eip155).
/// - The chainId segment must parse as `u64` with no leading zeros (`"0"`
///   alone is valid; `"01"` is rejected).
/// - The address segment must not contain further colons (prevents
///   suffix forgery).
/// - The address must be a valid 0x + 40-hex string.
/// - Extra: the parsed chainId must equal `expected_chain_id` (prevents
///   accidental cross-chain reuse — e.g. a mainnet DID hitting a testnet
///   deployment).
///
/// Any failure → `70000 invalid source DID`.
pub(super) fn parse_did_pkh_eip155(
    did: &str,
    expected_chain_id: u64,
) -> Result<Address, SaApiError> {
    let rest = did.strip_prefix("did:pkh:eip155:").ok_or_else(|| {
        SaApiError::new(
            70000,
            format!("source DID must start with did:pkh:eip155: ({did})"),
        )
    })?;
    let (chain_id_str, address_str) = rest.split_once(':').ok_or_else(|| {
        SaApiError::new(70000, format!("source DID missing address segment ({did})"))
    })?;
    if chain_id_str.len() > 1 && chain_id_str.starts_with('0') {
        return Err(SaApiError::new(
            70000,
            format!("source DID chainId has leading zero: {chain_id_str}"),
        ));
    }
    let chain_id: u64 = chain_id_str
        .parse()
        .map_err(|e| SaApiError::new(70000, format!("invalid chainId in source DID: {e}")))?;
    if chain_id != expected_chain_id {
        return Err(SaApiError::new(
            70000,
            format!("source DID chainId {chain_id} != expected {expected_chain_id}"),
        ));
    }
    if address_str.contains(':') {
        return Err(SaApiError::new(
            70000,
            format!("source DID address segment has invalid chars: {address_str}"),
        ));
    }
    parse_address(address_str)
}

/// Extract `(payer, authorized_signer)` by branching on `payload.type`.
///
/// - **transaction mode**: `payer = payload.authorization.from`. The SDK
///   does not cross-check against the `source` DID — in transaction mode
///   `source` is an optional auxiliary field, and `authorization.from` is
///   the authoritative signature-bound value.
/// - **hash mode**: `payer = parse_did_pkh_eip155(source, chain_id)` (spec
///   requires `source` in hash mode).
/// - **authorized_signer**: prefer `payload.authorizedSigner` (non-zero);
///   otherwise fall back to `payer`. A client explicitly sending
///   `authorizedSigner == payer` (redundant but valid) is silently
///   accepted, matching mpp-rs behavior.
///
/// All errors map to 70000 invalid_payload.
pub(super) fn extract_payer_and_signer(
    payload: &serde_json::Value,
    source: Option<&str>,
    chain_id: u64,
) -> Result<(Address, Address), SaApiError> {
    let payload_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let payer = match payload_type {
        "transaction" => {
            let authorization = payload
                .get("authorization")
                .ok_or_else(|| SaApiError::new(70000, "missing required field authorization"))?;
            parse_address(extract_required_str(authorization, "from")?)?
        }
        "hash" => {
            let did = source
                .filter(|s| !s.is_empty())
                .ok_or_else(|| SaApiError::new(70000, "hash mode credential missing source"))?;
            parse_did_pkh_eip155(did, chain_id)?
        }
        other => {
            return Err(SaApiError::new(
                70000,
                format!("unsupported payload type {other:?} (expected transaction|hash)"),
            ))
        }
    };

    let raw_signer = payload
        .get("authorizedSigner")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<Address>())
        .transpose()
        .map_err(|e| SaApiError::new(70000, format!("invalid authorizedSigner: {e}")))?;
    let authorized_signer = match raw_signer {
        Some(a) if a != Address::ZERO => a,
        _ => payer,
    };
    Ok((payer, authorized_signer))
}

/// Strip SDK-only fields from `payload` before forwarding the credential
/// to SA `/session/open`.
///
/// `cumulativeAmount` and the voucher-signature field are baseline voucher
/// fields the client passes to the SDK only — SA's spec doesn't list them,
/// so we strip before forwarding (avoids strict-schema rejection and
/// reduces wire size). `challenge` / `source` at the top level stay (the
/// open endpoint still needs `challenge`).
///
/// **The voucher-signature field name to strip depends on `payload.type`**:
/// - `transaction`: voucher signature lives in `voucherSignature`
///   (`signature` is the EIP-3009 deposit signature — SA must keep it).
/// - `hash`: voucher signature occupies `signature` directly (no deposit
///   signature exists, so the whole `signature` field is SDK-only and
///   must be stripped).
pub(super) fn strip_sdk_only_open_fields(
    credential: &PaymentCredential,
) -> Result<serde_json::Value, SaApiError> {
    let mut credential_json = serde_json::to_value(credential)
        .map_err(|e| SaApiError::new(8000, format!("serialize credential: {e}")))?;
    if let Some(payload_obj) = credential_json
        .get_mut("payload")
        .and_then(|v| v.as_object_mut())
    {
        let payload_type = payload_obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        payload_obj.remove("cumulativeAmount");
        if payload_type == "hash" {
            payload_obj.remove("signature");
        } else {
            payload_obj.remove("voucherSignature");
        }
    }
    Ok(credential_json)
}
