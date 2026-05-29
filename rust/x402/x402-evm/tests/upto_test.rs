//! Integration tests for the upto scheme.

use std::collections::HashMap;

use serde_json::json;

use x402_core::types::{
    PaymentRequirements, SchemeNetworkServer, SupportedKind,
};
use x402_evm::{
    is_upto_permit2_payload, UptoEvmScheme, UptoPermit2Authorization, UptoPermit2Payload,
    UptoPermit2Witness, PERMIT2_UPTO_WITNESS_TYPE_STRING, X402_UPTO_PERMIT2_PROXY_ADDRESS,
};

fn make_requirements() -> PaymentRequirements {
    PaymentRequirements {
        scheme: "upto".to_string(),
        network: "eip155:196".to_string(),
        asset: "0x779ded0c9e1022225f8e0630b35a9b54be713736".to_string(),
        amount: "5000000".to_string(), // cap
        pay_to: "0xMerchant".to_string(),
        max_timeout_seconds: 600,
        extra: HashMap::new(),
    }
}

fn make_supported_kind_with_facilitator(addr: &str) -> SupportedKind {
    let mut extra = HashMap::new();
    extra.insert(
        "facilitatorAddress".to_string(),
        serde_json::Value::String(addr.to_string()),
    );
    SupportedKind {
        x402_version: 2,
        scheme: "upto".to_string(),
        network: "eip155:196".to_string(),
        extra: Some(extra),
    }
}

#[test]
fn upto_proxy_address_constant_unchanged() {
    assert_eq!(
        X402_UPTO_PERMIT2_PROXY_ADDRESS,
        "0x4020e7393B728A3939659E5732F87fdd8e680002"
    );
}

#[test]
fn upto_type_string_includes_facilitator() {
    assert!(PERMIT2_UPTO_WITNESS_TYPE_STRING
        .contains("Witness(address to,address facilitator,uint256 validAfter)"));
}

#[test]
fn upto_payload_round_trips_wire_json() {
    let wire = json!({
        "signature": "0xfa42c11c",
        "permit2Authorization": {
            "from": "0xBuyer",
            "permitted": { "token": "0xToken", "amount": "5000000" },
            "spender": X402_UPTO_PERMIT2_PROXY_ADDRESS,
            "nonce": "1027389",
            "deadline": "1714813500",
            "witness": {
                "to": "0xMerchant",
                "facilitator": "0xFacilitator",
                "validAfter": "1714812840"
            }
        }
    });

    let parsed: UptoPermit2Payload = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(parsed.signature, "0xfa42c11c");
    assert_eq!(
        parsed.permit2_authorization.witness.facilitator,
        "0xFacilitator"
    );
    assert_eq!(parsed.permit2_authorization.permitted.amount, "5000000");

    let reserialized = serde_json::to_value(&parsed).unwrap();
    assert_eq!(reserialized, wire);
}

#[test]
fn is_upto_payload_distinguishes_by_witness_facilitator() {
    let upto = json!({
        "signature": "0xs",
        "permit2Authorization": {
            "witness": { "to": "0xB", "facilitator": "0xF", "validAfter": "0" }
        }
    });
    let exact_permit2 = json!({
        "signature": "0xs",
        "permit2Authorization": {
            "witness": { "to": "0xB", "validAfter": "0" }
        }
    });
    let eip3009 = json!({
        "signature": "0xs",
        "authorization": { "from": "0xA", "to": "0xB" }
    });

    assert!(is_upto_permit2_payload(&upto));
    assert!(!is_upto_permit2_payload(&exact_permit2));
    assert!(!is_upto_permit2_payload(&eip3009));
}

#[tokio::test]
async fn enhance_forces_permit2_and_injects_facilitator() {
    let scheme = UptoEvmScheme::new();
    let enhanced = scheme
        .enhance_payment_requirements(
            make_requirements(),
            &make_supported_kind_with_facilitator("0xFacilitatorAaa"),
            &[],
        )
        .await
        .unwrap();

    assert_eq!(
        enhanced.extra.get("assetTransferMethod"),
        Some(&serde_json::Value::String("permit2".to_string()))
    );
    assert_eq!(
        enhanced.extra.get("facilitatorAddress"),
        Some(&serde_json::Value::String("0xFacilitatorAaa".to_string()))
    );
}

#[tokio::test]
async fn enhance_overrides_caller_eip3009_attempt() {
    let scheme = UptoEvmScheme::new();
    let mut req = make_requirements();
    req.extra.insert(
        "assetTransferMethod".to_string(),
        serde_json::Value::String("eip3009".to_string()),
    );

    let enhanced = scheme
        .enhance_payment_requirements(req, &make_supported_kind_with_facilitator("0xF"), &[])
        .await
        .unwrap();

    assert_eq!(
        enhanced.extra.get("assetTransferMethod"),
        Some(&serde_json::Value::String("permit2".to_string()))
    );
}

#[test]
fn upto_witness_field_order_pinned() {
    let w = UptoPermit2Witness {
        to: "0xM".to_string(),
        facilitator: "0xF".to_string(),
        valid_after: "0".to_string(),
    };
    let s = serde_json::to_string(&w).unwrap();

    let to_pos = s.find("\"to\"").unwrap();
    let facilitator_pos = s.find("\"facilitator\"").unwrap();
    let valid_after_pos = s.find("\"validAfter\"").unwrap();

    // Order is locked — any swap changes the EIP-712 typehash.
    assert!(to_pos < facilitator_pos);
    assert!(facilitator_pos < valid_after_pos);
}

#[test]
fn upto_payload_can_be_built_from_typed_components() {
    let _payload = UptoPermit2Payload {
        signature: "0xsig".to_string(),
        permit2_authorization: UptoPermit2Authorization {
            from: "0xBuyer".to_string(),
            permitted: x402_evm::Permit2Permitted {
                token: "0xT".to_string(),
                amount: "1".to_string(),
            },
            spender: X402_UPTO_PERMIT2_PROXY_ADDRESS.to_string(),
            nonce: "1".to_string(),
            deadline: "1".to_string(),
            witness: UptoPermit2Witness {
                to: "0xB".to_string(),
                facilitator: "0xF".to_string(),
                valid_after: "0".to_string(),
            },
        },
    };
}
