//! `PermitSubscriptionScheme` — Seller-side `period` scheme.
//!
//! Builds the 402 `PaymentRequirements`. `enhance_payment_requirements` injects
//! the `facilitator` EOA (from `/supported`), the on-chain `contracts`
//! (subscription + Permit2), and the EIP-712 `domain`. Per-plan params
//! (`amountPerPeriod` / `periodSec` / `maxPeriods` / `initialCharge` / `plan`)
//! are expected already in `extra`; caller-set values are always preserved.

use async_trait::async_trait;
use serde_json::json;

use x402_core::error::X402Error;
use x402_core::types::{
    AssetAmount, Network, PaymentRequirements, Price, SchemeNetworkServer, SettlementMode,
    SupportedKind,
};

use crate::exact::ExactEvmScheme;

/// The scheme identifier.
pub const SUBSCRIPTION_SCHEME: &str = "period";

/// Extract the numeric EVM chain id from a CAIP-2 network (`eip155:196` → 196).
fn caip2_chain_id(network: &str) -> Result<u64, X402Error> {
    network
        .strip_prefix("eip155:")
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| X402Error::UnsupportedNetwork(network.to_string()))
}

/// Server implementation for the `period` scheme.
///
/// `facilitator`, the subscription contract and the Permit2 contract are all
/// sourced from the facilitator's `/supported`; a merchant override takes
/// priority when set.
pub struct PermitSubscriptionScheme {
    /// Reused for price → token-amount parsing (same logic as `exact`).
    exact: ExactEvmScheme,
    facilitator: Option<String>,
    subscription_contract: Option<String>,
    permit2_contract: Option<String>,
}

impl PermitSubscriptionScheme {
    pub fn new() -> Self {
        Self {
            exact: ExactEvmScheme::new(),
            facilitator: None,
            subscription_contract: None,
            permit2_contract: None,
        }
    }

    /// Override the facilitator EOA (else `/supported` `kind.extra.facilitator`).
    pub fn with_facilitator(mut self, addr: impl Into<String>) -> Self {
        self.facilitator = Some(addr.into());
        self
    }

    /// Override the subscription (`A2APaySubscription`) contract address (else
    /// `/supported` `kind.extra.subscriptionContract`).
    pub fn with_subscription_contract(mut self, addr: impl Into<String>) -> Self {
        self.subscription_contract = Some(addr.into());
        self
    }

    /// Override the Permit2 contract address (else `/supported`
    /// `kind.extra.permit2Contract`).
    pub fn with_permit2_contract(mut self, addr: impl Into<String>) -> Self {
        self.permit2_contract = Some(addr.into());
        self
    }
}

/// Resolve an address: merchant override → `/supported` `kind.extra.<key>`.
fn resolve_addr(
    override_val: &Option<String>,
    supported_kind: &SupportedKind,
    key: &str,
    what: &str,
    network: &str,
) -> Result<String, X402Error> {
    override_val
        .clone()
        .or_else(|| extra_str(supported_kind, key))
        .ok_or_else(|| {
            X402Error::Config(format!(
                "period: missing {what} for network {network}; \
                 /supported did not advertise `{key}` and no merchant override is set"
            ))
        })
}

/// Read a string field from a `SupportedKind`'s `extra` map.
fn extra_str(kind: &SupportedKind, key: &str) -> Option<String> {
    kind.extra
        .as_ref()
        .and_then(|e| e.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

impl Default for PermitSubscriptionScheme {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SchemeNetworkServer for PermitSubscriptionScheme {
    fn scheme(&self) -> &str {
        SUBSCRIPTION_SCHEME
    }

    async fn parse_price(
        &self,
        price: &Price,
        network: &Network,
    ) -> Result<AssetAmount, X402Error> {
        self.exact.parse_price(price, network).await
    }

    async fn enhance_payment_requirements(
        &self,
        mut payment_requirements: PaymentRequirements,
        supported_kind: &SupportedKind,
        _facilitator_extensions: &[String],
    ) -> Result<PaymentRequirements, X402Error> {
        // facilitator EOA from `/supported`: prefer `facilitatorAddress` (the key
        // other schemes use), fall back to `facilitator` (period's legacy key) so
        // both old and new backends work. A caller-pinned value wins. The offer
        // key stays `facilitator` (buyer reads that + signs it into terms).
        let network = payment_requirements.network.clone();
        if !payment_requirements.extra.contains_key("facilitator") {
            let facilitator = self
                .facilitator
                .clone()
                .or_else(|| extra_str(supported_kind, "facilitatorAddress"))
                .or_else(|| extra_str(supported_kind, "facilitator"))
                .ok_or_else(|| {
                    X402Error::Config(format!(
                        "period: missing facilitator for network {network}; /supported \
                         advertised neither `facilitatorAddress` nor `facilitator` and no \
                         merchant override is set"
                    ))
                })?;
            payment_requirements
                .extra
                .insert("facilitator".to_string(), json!(facilitator));
        }

        // contracts { subscription, permit2 }.
        if !payment_requirements.extra.contains_key("contracts") {
            let subscription = resolve_addr(
                &self.subscription_contract,
                supported_kind,
                "subscriptionContract",
                "subscription contract",
                &network,
            )?;
            let permit2 = resolve_addr(
                &self.permit2_contract,
                supported_kind,
                "permit2Contract",
                "permit2 contract",
                &network,
            )?;
            payment_requirements.extra.insert(
                "contracts".to_string(),
                json!({ "subscription": subscription, "permit2": permit2 }),
            );
        }

        // EIP-712 domain (verifyingContract = subscription contract).
        if !payment_requirements.extra.contains_key("domain") {
            let subscription = resolve_addr(
                &self.subscription_contract,
                supported_kind,
                "subscriptionContract",
                "subscription contract",
                &network,
            )?;
            let chain_id = caip2_chain_id(&payment_requirements.network)?;
            payment_requirements.extra.insert(
                "domain".to_string(),
                json!({
                    "name": "A2APaySubscription",
                    "version": "1",
                    "chainId": chain_id,
                    "verifyingContract": subscription,
                }),
            );
        }

        Ok(payment_requirements)
    }

    /// Subscriptions settle (create / charge) before serving.
    fn settlement_mode(&self) -> SettlementMode {
        SettlementMode::Pre
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn reqs() -> PaymentRequirements {
        PaymentRequirements {
            scheme: SUBSCRIPTION_SCHEME.to_string(),
            network: "eip155:196".to_string(),
            asset: "0x779ded0c9e1022225f8e0630b35a9b54be713736".to_string(),
            amount: "5000000".to_string(),
            pay_to: "0xMerchant".to_string(),
            max_timeout_seconds: 600,
            extra: HashMap::new(),
        }
    }

    fn supported_with_facilitator(addr: Option<&str>) -> SupportedKind {
        // Backend advertises the facilitator EOA under `facilitatorAddress`.
        let extra = addr.map(|a| {
            let mut m = HashMap::new();
            m.insert("facilitatorAddress".to_string(), json!(a));
            m
        });
        SupportedKind {
            x402_version: 2,
            scheme: SUBSCRIPTION_SCHEME.to_string(),
            network: "eip155:196".to_string(),
            extra,
        }
    }

    /// A `/supported` kind carrying the full subscription three-some.
    fn supported_full(facilitator: &str, subscription: &str, permit2: &str) -> SupportedKind {
        let mut m = HashMap::new();
        m.insert("facilitatorAddress".to_string(), json!(facilitator));
        m.insert("subscriptionContract".to_string(), json!(subscription));
        m.insert("permit2Contract".to_string(), json!(permit2));
        SupportedKind {
            x402_version: 2,
            scheme: SUBSCRIPTION_SCHEME.to_string(),
            network: "eip155:196".to_string(),
            extra: Some(m),
        }
    }

    #[tokio::test]
    async fn enhance_falls_back_to_legacy_facilitator_key() {
        // Old backend advertises `facilitator` (not `facilitatorAddress`) — still resolves.
        let mut m = HashMap::new();
        m.insert("facilitator".to_string(), json!("0xLegacyFac"));
        let kind = SupportedKind {
            x402_version: 2,
            scheme: SUBSCRIPTION_SCHEME.to_string(),
            network: "eip155:196".to_string(),
            extra: Some(m),
        };
        let scheme = PermitSubscriptionScheme::new()
            .with_subscription_contract("0x4020000000000000000000000000000000000003")
            .with_permit2_contract("0x000000000022d473030f116ddee9f6b43ac78ba3");
        let out = scheme.enhance_payment_requirements(reqs(), &kind, &[]).await.unwrap();
        // Offer key stays `facilitator` regardless of which /supported key was read.
        assert_eq!(out.extra["facilitator"], json!("0xLegacyFac"));
    }

    #[test]
    fn scheme_name_and_settlement_mode() {
        let s = PermitSubscriptionScheme::new();
        assert_eq!(s.scheme(), "period");
        assert_eq!(s.settlement_mode(), SettlementMode::Pre);
    }

    #[tokio::test]
    async fn enhance_injects_contracts_facilitator_domain() {
        // Merchant overrides for subscription + permit2; facilitator from /supported.
        let scheme = PermitSubscriptionScheme::new()
            .with_subscription_contract("0x4020000000000000000000000000000000000003")
            .with_permit2_contract("0x000000000022d473030f116ddee9f6b43ac78ba3");
        let out = scheme
            .enhance_payment_requirements(reqs(), &supported_with_facilitator(Some("0xFac")), &[])
            .await
            .unwrap();

        assert_eq!(out.extra["facilitator"], json!("0xFac"));
        assert_eq!(
            out.extra["contracts"]["subscription"],
            json!("0x4020000000000000000000000000000000000003")
        );
        assert_eq!(
            out.extra["contracts"]["permit2"],
            json!("0x000000000022d473030f116ddee9f6b43ac78ba3")
        );
        assert_eq!(out.extra["domain"]["name"], json!("A2APaySubscription"));
        assert_eq!(out.extra["domain"]["version"], json!("1"));
        assert_eq!(out.extra["domain"]["chainId"], json!(196));
        assert_eq!(
            out.extra["domain"]["verifyingContract"],
            json!("0x4020000000000000000000000000000000000003")
        );
    }

    #[tokio::test]
    async fn enhance_preserves_caller_set_values() {
        let scheme = PermitSubscriptionScheme::new()
            .with_subscription_contract("0xBuiltIn");
        let mut r = reqs();
        r.extra.insert("facilitator".to_string(), json!("0xCallerFac"));
        r.extra.insert(
            "contracts".to_string(),
            json!({ "subscription": "0xCallerSub", "permit2": "0xCallerP2" }),
        );
        let out = scheme
            .enhance_payment_requirements(r, &supported_with_facilitator(Some("0xFac")), &[])
            .await
            .unwrap();
        // Caller-set facilitator + contracts are preserved.
        assert_eq!(out.extra["facilitator"], json!("0xCallerFac"));
        assert_eq!(out.extra["contracts"]["subscription"], json!("0xCallerSub"));
    }

    #[tokio::test]
    async fn enhance_reads_three_some_from_supported() {
        // No caller config → all three come from `/supported` kind.extra.
        let scheme = PermitSubscriptionScheme::new();
        let kind = supported_full(
            "0x8c71e6826f754300fea650f68625bf3f15e0a7fa",
            "0x3b01d25f7123aaec97762491d597ca7a215d8032",
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
        );
        let out = scheme
            .enhance_payment_requirements(reqs(), &kind, &[])
            .await
            .unwrap();
        assert_eq!(
            out.extra["facilitator"],
            json!("0x8c71e6826f754300fea650f68625bf3f15e0a7fa")
        );
        assert_eq!(
            out.extra["contracts"]["subscription"],
            json!("0x3b01d25f7123aaec97762491d597ca7a215d8032")
        );
        assert_eq!(
            out.extra["contracts"]["permit2"],
            json!("0x000000000022d473030f116ddee9f6b43ac78ba3")
        );
        // domain.verifyingContract tracks the /supported subscription contract.
        assert_eq!(
            out.extra["domain"]["verifyingContract"],
            json!("0x3b01d25f7123aaec97762491d597ca7a215d8032")
        );
    }

    #[tokio::test]
    async fn enhance_errors_when_no_subscription_contract() {
        // /supported advertises facilitator but no subscriptionContract, and no
        // merchant override → error on the subscription contract.
        let mut r = reqs();
        r.network = "eip155:1952".to_string();
        let scheme = PermitSubscriptionScheme::new();
        let err = scheme
            .enhance_payment_requirements(r, &supported_with_facilitator(Some("0xFac")), &[])
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("subscription contract"));
    }
}
