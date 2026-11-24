//! `SubscriptionPlan` — a typed builder for a `period` offer.
//!
//! Turns a declared plan into an [`AcceptConfig`] for a route, building the
//! `extra` JSON so it need not be hand-written. The registered
//! [`super::PermitSubscriptionScheme`] injects `contracts` / `facilitator` /
//! `domain` at 402-build time.

use std::collections::HashMap;

use serde_json::{json, Value};
use x402_core::http::AcceptConfig;

use super::server_scheme::SUBSCRIPTION_SCHEME;

/// A subscription plan offered on a protected route.
///
/// Atomic amounts (`amount_per_period`, `initial_charge_amount`) are decimal
/// strings in the token's smallest unit. `price` is the first-charge price
/// (e.g. `"$5"`) parsed by the scheme into `PaymentRequirements.amount`.
#[derive(Debug, Clone)]
pub struct SubscriptionPlan {
    /// Business plan id (e.g. `"pro_monthly"`).
    pub id: String,
    /// Plan tier (`> 0`); higher = more access.
    pub tier: u8,
    pub network: String,
    /// Merchant receiving address (`payTo`).
    pub pay_to: String,
    /// First-charge price string (parsed by the scheme into `amount`).
    pub price: String,
    /// Per-period amount, atomic (`uint160`).
    pub amount_per_period: String,
    /// Fixed-seconds period length. In calendar-month mode (`period_mode == 1`)
    /// this MUST be 0.
    pub period_sec: u64,
    /// Period mode: 0 = fixed_seconds, 1 = calendar_month.
    pub period_mode: u8,
    pub max_periods: u32,
    /// `0` → contract uses `block.timestamp`.
    pub start_at: u64,
    /// Periods pre-charged at subscribe (`0` = no separate initial charge).
    pub initial_charge_periods: u32,
    /// Initial-charge total, atomic (`"0"` if none).
    pub initial_charge_amount: String,
    pub max_timeout_seconds: Option<u64>,
    pub name: Option<String>,
    pub features: Option<Vec<String>>,
}

impl SubscriptionPlan {
    /// Build the plan-level `extra` object (the scheme injects
    /// contracts / facilitator / domain later).
    fn build_extra(&self) -> HashMap<String, Value> {
        let mut plan = json!({ "id": self.id, "tier": self.tier });
        if let Some(name) = &self.name {
            plan["name"] = json!(name);
        }
        if let Some(features) = &self.features {
            plan["features"] = json!(features);
        }

        let mut extra = HashMap::new();
        extra.insert("amountPerPeriod".to_string(), json!(self.amount_per_period));
        extra.insert("periodSec".to_string(), json!(self.period_sec));
        extra.insert("periodMode".to_string(), json!(self.period_mode));
        extra.insert("maxPeriods".to_string(), json!(self.max_periods));
        extra.insert("startAt".to_string(), json!(self.start_at));
        // initialCharge only when there is one.
        if self.initial_charge_periods > 0 {
            extra.insert(
                "initialCharge".to_string(),
                json!({
                    "periodCount": self.initial_charge_periods,
                    "totalAmount": self.initial_charge_amount,
                    "coversFirstPeriods": true,
                }),
            );
        }
        extra.insert("plan".to_string(), plan);
        extra
    }

    /// Convert into an [`AcceptConfig`] for a route's `accepts` list.
    pub fn to_accept_config(&self) -> AcceptConfig {
        AcceptConfig {
            scheme: SUBSCRIPTION_SCHEME.to_string(),
            price: self.price.clone(),
            network: self.network.clone(),
            pay_to: self.pay_to.clone(),
            max_timeout_seconds: self.max_timeout_seconds,
            extra: Some(self.build_extra()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SubscriptionPlan {
        SubscriptionPlan {
            id: "pro_monthly".into(),
            tier: 2,
            network: "eip155:196".into(),
            pay_to: "0xMerchant".into(),
            price: "$5".into(),
            amount_per_period: "5000000".into(),
            period_sec: 2_592_000,
            period_mode: 0,
            max_periods: 12,
            start_at: 0,
            initial_charge_periods: 1,
            initial_charge_amount: "5000000".into(),
            max_timeout_seconds: Some(600),
            name: Some("Pro".into()),
            features: Some(vec!["api_basic".into(), "api_pro".into()]),
        }
    }

    #[test]
    fn to_accept_config_shapes_scheme_price_and_extra() {
        let cfg = sample().to_accept_config();
        assert_eq!(cfg.scheme, "period");
        assert_eq!(cfg.price, "$5");
        assert_eq!(cfg.network, "eip155:196");
        assert_eq!(cfg.pay_to, "0xMerchant");
        assert_eq!(cfg.max_timeout_seconds, Some(600));

        let extra = cfg.extra.unwrap();
        assert_eq!(extra["amountPerPeriod"], json!("5000000"));
        assert_eq!(extra["periodSec"], json!(2_592_000));
        assert_eq!(extra["maxPeriods"], json!(12));
        assert_eq!(extra["initialCharge"]["periodCount"], json!(1));
        assert_eq!(extra["initialCharge"]["totalAmount"], json!("5000000"));
        assert_eq!(extra["plan"]["id"], json!("pro_monthly"));
        assert_eq!(extra["plan"]["tier"], json!(2));
        assert_eq!(extra["plan"]["name"], json!("Pro"));
        assert_eq!(extra["plan"]["features"][1], json!("api_pro"));
        // planId is NOT a top-level extra field — it lives under plan.id.
        assert!(!extra.contains_key("planId"));
    }

    #[test]
    fn no_initial_charge_omits_block() {
        let mut p = sample();
        p.initial_charge_periods = 0;
        p.initial_charge_amount = "0".into();
        let cfg = p.to_accept_config();
        assert!(!cfg.extra.unwrap().contains_key("initialCharge"));
    }
}
