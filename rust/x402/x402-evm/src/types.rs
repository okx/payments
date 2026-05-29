//! EVM-specific payload types.

use serde::{Deserialize, Serialize};

/// Asset transfer methods for the exact EVM scheme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetTransferMethod {
    /// Uses transferWithAuthorization (USDC, etc.) — recommended for compatible tokens.
    #[serde(rename = "eip3009")]
    Eip3009,
    /// Uses Permit2 + x402Permit2Proxy — universal fallback for any ERC-20.
    Permit2,
}

/// EIP-3009 authorization parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EIP3009Authorization {
    pub from: String,
    pub to: String,
    pub value: String,
    pub valid_after: String,
    pub valid_before: String,
    pub nonce: String,
}

/// EIP-3009 payload for tokens with native transferWithAuthorization support.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactEIP3009Payload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub authorization: EIP3009Authorization,
}

/// Permit2 witness data structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Witness {
    pub to: String,
    pub valid_after: String,
}

/// Permit2 permitted token info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permit2Permitted {
    pub token: String,
    pub amount: String,
}

/// Permit2 authorization parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Authorization {
    pub from: String,
    pub permitted: Permit2Permitted,
    pub spender: String,
    pub nonce: String,
    pub deadline: String,
    pub witness: Permit2Witness,
}

/// Permit2 payload for tokens using the Permit2 + x402Permit2Proxy flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactPermit2Payload {
    pub signature: String,
    pub permit2_authorization: Permit2Authorization,
}

/// Union of exact EVM payload types (V2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExactEvmPayloadV2 {
    EIP3009(ExactEIP3009Payload),
    Permit2(ExactPermit2Payload),
}

impl ExactEvmPayloadV2 {
    /// Check if this is a Permit2 payload.
    pub fn is_permit2(&self) -> bool {
        matches!(self, Self::Permit2(_))
    }

    /// Check if this is an EIP-3009 payload.
    pub fn is_eip3009(&self) -> bool {
        matches!(self, Self::EIP3009(_))
    }
}

/// Upto witness — adds `facilitator` to the exact-scheme witness so the
/// upto proxy can enforce `msg.sender == witness.facilitator` on chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoPermit2Witness {
    pub to: String,
    pub facilitator: String,
    pub valid_after: String,
}

/// Upto authorization — `permitted.amount` is the cap (facilitator may
/// settle ≤ this), not the exact charge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoPermit2Authorization {
    pub from: String,
    pub permitted: Permit2Permitted,
    pub spender: String,
    pub nonce: String,
    pub deadline: String,
    pub witness: UptoPermit2Witness,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoPermit2Payload {
    pub signature: String,
    pub permit2_authorization: UptoPermit2Authorization,
}

/// Returns true for raw `PaymentPayload.payload` JSON that has the upto
/// witness shape (`permit2Authorization.witness.facilitator` present).
pub fn is_upto_permit2_payload(payload: &serde_json::Value) -> bool {
    payload.get("permit2Authorization").is_some()
        && payload["permit2Authorization"]
            .get("witness")
            .and_then(|w| w.get("facilitator"))
            .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_asset_transfer_method_serialization() {
        // Wire format must be lowercase strings — protocol agreement.
        assert_eq!(
            serde_json::to_value(AssetTransferMethod::Eip3009).unwrap(),
            json!("eip3009")
        );
        assert_eq!(
            serde_json::to_value(AssetTransferMethod::Permit2).unwrap(),
            json!("permit2")
        );

        // Round-trip from wire JSON.
        let parsed: AssetTransferMethod = serde_json::from_str("\"permit2\"").unwrap();
        assert_eq!(parsed, AssetTransferMethod::Permit2);
    }

    #[test]
    fn test_permit2_witness_field_names() {
        // Witness fields are serialized as camelCase: `to`, `validAfter`.
        let witness = Permit2Witness {
            to: "0xMerchant".to_string(),
            valid_after: "1714812840".to_string(),
        };
        let v = serde_json::to_value(&witness).unwrap();
        assert_eq!(v["to"], "0xMerchant");
        assert_eq!(v["validAfter"], "1714812840");
        assert!(v.get("valid_after").is_none(), "snake_case must not leak");
    }

    #[test]
    fn test_permit2_authorization_field_names_and_order() {
        // Field set must exactly match the EIP-712 message field set:
        // from, permitted, spender, nonce, deadline, witness.
        // Field order in serialized JSON follows struct definition order.
        let auth = Permit2Authorization {
            from: "0xBuyer".to_string(),
            permitted: Permit2Permitted {
                token: "0xToken".to_string(),
                amount: "1234000".to_string(),
            },
            spender: "0x402085c248EeA27D92E8b30b2C58ed07f9E20001".to_string(),
            nonce: "1027389".to_string(),
            deadline: "1714813500".to_string(),
            witness: Permit2Witness {
                to: "0xMerchant".to_string(),
                valid_after: "1714812840".to_string(),
            },
        };

        // Verify by checking the field order in the canonical serialized form.
        let serialized = serde_json::to_string(&auth).unwrap();
        let from_pos = serialized.find("\"from\"").unwrap();
        let permitted_pos = serialized.find("\"permitted\"").unwrap();
        let spender_pos = serialized.find("\"spender\"").unwrap();
        let nonce_pos = serialized.find("\"nonce\"").unwrap();
        let deadline_pos = serialized.find("\"deadline\"").unwrap();
        let witness_pos = serialized.find("\"witness\"").unwrap();

        // Order matters because the facilitator recomputes the EIP-712 hash
        // by walking the payload in this order.
        assert!(from_pos < permitted_pos);
        assert!(permitted_pos < spender_pos);
        assert!(spender_pos < nonce_pos);
        assert!(nonce_pos < deadline_pos);
        assert!(deadline_pos < witness_pos);
    }

    #[test]
    fn test_exact_permit2_payload_serialization() {
        // The top-level payload uses camelCase: `signature`, `permit2Authorization`.
        let payload = ExactPermit2Payload {
            signature: "0xfa42c11c".to_string(),
            permit2_authorization: Permit2Authorization {
                from: "0xBuyer".to_string(),
                permitted: Permit2Permitted {
                    token: "0xToken".to_string(),
                    amount: "1234000".to_string(),
                },
                spender: X402_EXACT_PERMIT2_PROXY_ADDRESS_FOR_TEST.to_string(),
                nonce: "1".to_string(),
                deadline: "999".to_string(),
                witness: Permit2Witness {
                    to: "0xPayTo".to_string(),
                    valid_after: "0".to_string(),
                },
            },
        };
        let v = serde_json::to_value(&payload).unwrap();
        assert_eq!(v["signature"], "0xfa42c11c");
        assert!(v["permit2Authorization"].is_object());
        assert!(
            v.get("permit2_authorization").is_none(),
            "snake_case key must not leak"
        );
    }

    #[test]
    fn test_exact_evm_payload_v2_untagged_routing() {
        // Untagged union must route EIP-3009 vs Permit2 by structural shape:
        // - presence of `authorization` field → EIP3009
        // - presence of `permit2Authorization` field → Permit2

        let eip3009_json = json!({
            "signature": "0xsig",
            "authorization": {
                "from": "0xA",
                "to": "0xB",
                "value": "100",
                "validAfter": "0",
                "validBefore": "999",
                "nonce": "0xabc"
            }
        });
        let parsed: ExactEvmPayloadV2 = serde_json::from_value(eip3009_json).unwrap();
        assert!(parsed.is_eip3009());
        assert!(!parsed.is_permit2());

        let permit2_json = json!({
            "signature": "0xsig",
            "permit2Authorization": {
                "from": "0xA",
                "permitted": { "token": "0xT", "amount": "100" },
                "spender": "0xS",
                "nonce": "1",
                "deadline": "999",
                "witness": { "to": "0xB", "validAfter": "0" }
            }
        });
        let parsed: ExactEvmPayloadV2 = serde_json::from_value(permit2_json).unwrap();
        assert!(parsed.is_permit2());
        assert!(!parsed.is_eip3009());
    }

    // Local copy of the proxy address constant for the test above. This avoids
    // a cross-module dependency cycle in tests; the real constant lives in
    // `crate::constants::X402_EXACT_PERMIT2_PROXY_ADDRESS` and is validated by
    // a dedicated test in that module.
    const X402_EXACT_PERMIT2_PROXY_ADDRESS_FOR_TEST: &str =
        "0x402085c248EeA27D92E8b30b2C58ed07f9E20001";

    #[test]
    fn test_upto_witness_has_facilitator_in_camel_case() {
        // upto Witness must be camelCase on the wire and field order
        // must be: to, facilitator, validAfter.
        let w = UptoPermit2Witness {
            to: "0xMerchant".to_string(),
            facilitator: "0xFacilitator".to_string(),
            valid_after: "0".to_string(),
        };
        let s = serde_json::to_string(&w).unwrap();
        let to_pos = s.find("\"to\"").unwrap();
        let facilitator_pos = s.find("\"facilitator\"").unwrap();
        let valid_after_pos = s.find("\"validAfter\"").unwrap();
        assert!(to_pos < facilitator_pos && facilitator_pos < valid_after_pos);
    }

    #[test]
    fn test_upto_payload_round_trip() {
        let wire = json!({
            "signature": "0xfa42c11c",
            "permit2Authorization": {
                "from": "0xBuyer",
                "permitted": { "token": "0xT", "amount": "5000000" },
                "spender": "0x4020e7393B728A3939659E5732F87fdd8e680002",
                "nonce": "1",
                "deadline": "999",
                "witness": {
                    "to": "0xMerchant",
                    "facilitator": "0xFacilitator",
                    "validAfter": "0"
                }
            }
        });
        let parsed: UptoPermit2Payload = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(parsed.permit2_authorization.witness.facilitator, "0xFacilitator");
        let reserialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(reserialized, wire);
    }

    #[test]
    fn test_is_upto_payload_detects_facilitator() {
        let upto_payload = json!({
            "signature": "0xsig",
            "permit2Authorization": {
                "witness": { "to": "0xB", "facilitator": "0xF", "validAfter": "0" }
            }
        });
        let exact_payload = json!({
            "signature": "0xsig",
            "permit2Authorization": {
                "witness": { "to": "0xB", "validAfter": "0" }
            }
        });
        let eip3009_payload = json!({
            "signature": "0xsig",
            "authorization": { "from": "0xA", "to": "0xB" }
        });

        assert!(is_upto_permit2_payload(&upto_payload));
        assert!(!is_upto_permit2_payload(&exact_payload));
        assert!(!is_upto_permit2_payload(&eip3009_payload));
    }
}
