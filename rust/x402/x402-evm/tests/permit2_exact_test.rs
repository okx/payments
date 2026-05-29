//! Integration tests for the exact + Permit2 path.

use std::collections::HashMap;

use serde_json::json;

use x402_core::types::{
    AssetAmount, PaymentRequirements, Price, SchemeNetworkServer, SupportedKind,
};
use x402_evm::{
    AssetTransferMethod, ExactEvmPayloadV2, ExactEvmScheme, ExactPermit2Payload, PERMIT2_ADDRESS,
    X402_EXACT_PERMIT2_PROXY_ADDRESS,
};

fn make_supported_kind() -> SupportedKind {
    SupportedKind {
        x402_version: 2,
        scheme: "exact".to_string(),
        network: "eip155:196".to_string(),
        extra: None,
    }
}

fn make_requirements(network: &str, asset: &str) -> PaymentRequirements {
    PaymentRequirements {
        scheme: "exact".to_string(),
        network: network.to_string(),
        asset: asset.to_string(),
        amount: "1000".to_string(),
        pay_to: "0xMerchant".to_string(),
        max_timeout_seconds: 600,
        extra: HashMap::new(),
    }
}

#[test]
fn permit2_address_constant_unchanged() {
    assert_eq!(PERMIT2_ADDRESS, "0x000000000022D473030F116dDEE9F6B43aC78BA3");
}

#[test]
fn exact_permit2_proxy_address_unchanged() {
    assert_eq!(
        X402_EXACT_PERMIT2_PROXY_ADDRESS,
        "0x402085c248EeA27D92E8b30b2C58ed07f9E20001"
    );
}

#[test]
fn exact_permit2_payload_round_trip_via_wire_json() {
    let wire = json!({
        "signature": "0xfa42c11c",
        "permit2Authorization": {
            "from":      "0xBuyer",
            "permitted": { "token": "0xToken", "amount": "1234000" },
            "spender":   X402_EXACT_PERMIT2_PROXY_ADDRESS,
            "nonce":     "1027389",
            "deadline":  "1714813500",
            "witness":   { "to": "0xMerchant", "validAfter": "1714812840" }
        }
    });

    let payload: ExactPermit2Payload = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(payload.signature, "0xfa42c11c");
    assert_eq!(payload.permit2_authorization.from, "0xBuyer");
    assert_eq!(payload.permit2_authorization.permitted.token, "0xToken");
    assert_eq!(payload.permit2_authorization.permitted.amount, "1234000");
    assert_eq!(
        payload.permit2_authorization.spender,
        X402_EXACT_PERMIT2_PROXY_ADDRESS
    );
    assert_eq!(payload.permit2_authorization.witness.to, "0xMerchant");
    assert_eq!(
        payload.permit2_authorization.witness.valid_after,
        "1714812840"
    );

    let reserialized = serde_json::to_value(&payload).unwrap();
    assert_eq!(reserialized, wire);
}

#[test]
fn exact_evm_payload_v2_dispatches_by_payload_shape() {
    let eip3009: ExactEvmPayloadV2 = serde_json::from_value(json!({
        "signature": "0xsig",
        "authorization": {
            "from": "0xA", "to": "0xB", "value": "100",
            "validAfter": "0", "validBefore": "999", "nonce": "0xabc"
        }
    }))
    .unwrap();
    assert!(eip3009.is_eip3009());
    assert!(!eip3009.is_permit2());

    let permit2: ExactEvmPayloadV2 = serde_json::from_value(json!({
        "signature": "0xsig",
        "permit2Authorization": {
            "from": "0xA",
            "permitted": { "token": "0xT", "amount": "100" },
            "spender": X402_EXACT_PERMIT2_PROXY_ADDRESS,
            "nonce": "1", "deadline": "999",
            "witness": { "to": "0xB", "validAfter": "0" }
        }
    }))
    .unwrap();
    assert!(permit2.is_permit2());
    assert!(!permit2.is_eip3009());
}

#[test]
fn asset_transfer_method_wire_format_is_lowercase() {
    assert_eq!(
        serde_json::to_value(AssetTransferMethod::Eip3009).unwrap(),
        json!("eip3009")
    );
    assert_eq!(
        serde_json::to_value(AssetTransferMethod::Permit2).unwrap(),
        json!("permit2")
    );
}

#[tokio::test]
async fn enhance_preserves_caller_set_extra() {
    let scheme = ExactEvmScheme::new();
    let mut req = make_requirements("eip155:196", "0xToken");
    req.extra.insert(
        "assetTransferMethod".to_string(),
        serde_json::Value::String("permit2".to_string()),
    );

    let enhanced = scheme
        .enhance_payment_requirements(req, &make_supported_kind(), &[])
        .await
        .unwrap();

    assert_eq!(
        enhanced.extra.get("assetTransferMethod"),
        Some(&serde_json::Value::String("permit2".to_string()))
    );
}

#[tokio::test]
async fn parse_price_with_explicit_permit2_extra_preserves_it() {
    let scheme = ExactEvmScheme::new();
    let price = Price::Asset(AssetAmount {
        amount: "5000".to_string(),
        asset: "0xToken".to_string(),
        extra: Some(HashMap::from([(
            "assetTransferMethod".to_string(),
            json!("permit2"),
        )])),
    });

    let result = scheme
        .parse_price(&price, &"eip155:196".to_string())
        .await
        .unwrap();

    assert_eq!(
        result.extra.unwrap().get("assetTransferMethod"),
        Some(&json!("permit2"))
    );
}
