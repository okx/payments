//! Optional Seller-side subscription cache. The facilitator/chain stay
//! authoritative; this cache lets the Seller:
//! - serve repeat `APP-Access` without a facilitator round-trip (bounded by a
//!   freshness TTL);
//! - back `due_subscriptions(now)` (a `list()` scan filtered by the caller) for
//!   Seller-driven charging;
//! - keep a local view for reconcile/display.
//!
//! [`InMemorySubscriptionStore`] is the default; implement [`SubscriptionStore`]
//! over Redis/SQL for a durable backend.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::types::subscription_state;

/// A cached view of one subscription. `updated_at` drives the access-cache TTL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionRecord {
    pub sub_id: String,
    /// `SubscriptionState` (0 pending / 1 active / 2 completed / 3 canceled /
    /// 4 changed / 99 failed).
    pub state: u8,
    pub payer: String,
    /// Business plan id from `SubscriptionTerms.planId`; empty = unknown. Drives
    /// route-level plan gating on `APP-Access`.
    #[serde(default)]
    pub plan_id: String,
    /// Plan tier from `SubscriptionTerms.planTier`; `0` = unknown.
    #[serde(default)]
    pub plan_tier: u8,
    /// Next chargeable time (Unix secs) — drives `due_subscriptions`. `None`
    /// until known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_chargeable_at: Option<u64>,
    /// Successor subId once changed (up/downgrade).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_to_sub_id: Option<String>,
    /// Subscription start (Unix secs). `0` = unknown/placeholder, backfilled by a
    /// later facilitator refresh. Drives local period math.
    #[serde(default)]
    pub start_at: u64,
    /// Fixed-mode period length (secs); `0` in calendar-month mode.
    #[serde(default)]
    pub period_sec: u64,
    /// Period mode: 0 fixed_seconds / 1 calendar_month (`period::PERIOD_MODE_*`).
    #[serde(default)]
    pub period_mode: u8,
    /// Calendar-month billing anchor (Unix secs); `0` falls back to `start_at`;
    /// ignored in fixed mode.
    #[serde(default)]
    pub billing_anchor_at: u64,
    /// Total committed periods — the upper bound for expiry (`elapsed > max`).
    #[serde(default)]
    pub max_periods: u32,
    /// Highest period number charged so far (1-based). `None` = never charged or
    /// not yet fetched (forces the access slow-path to refresh). The current
    /// period is not cached — it is recomputed from the clock each time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_charged_period: Option<u32>,
    /// When this record was last written (Unix secs).
    pub updated_at: u64,
}

impl SubscriptionRecord {
    pub fn is_active(&self) -> bool {
        self.state == subscription_state::ACTIVE
    }
}

/// Seller-side subscription cache. All methods are individually atomic;
/// the facilitator/chain stay the source of truth.
#[async_trait]
pub trait SubscriptionStore: Send + Sync {
    async fn get(&self, sub_id: &str) -> Option<SubscriptionRecord>;
    async fn put(&self, record: SubscriptionRecord);
    async fn remove(&self, sub_id: &str);
    /// All cached records. Due-for-charge filtering is done by the caller
    /// (`SubscriptionSupport::due_subscriptions`); a durable backend may add its
    /// own indexed due-query outside this trait if a full scan is too costly.
    async fn list(&self) -> Vec<SubscriptionRecord>;
}

/// In-memory [`SubscriptionStore`] (process-local, non-durable). Recovers from
/// a poisoned mutex rather than panicking.
#[derive(Default)]
pub struct InMemorySubscriptionStore {
    inner: Mutex<HashMap<String, SubscriptionRecord>>,
}

impl InMemorySubscriptionStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, SubscriptionRecord>> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[async_trait]
impl SubscriptionStore for InMemorySubscriptionStore {
    async fn get(&self, sub_id: &str) -> Option<SubscriptionRecord> {
        self.lock().get(sub_id).cloned()
    }

    async fn put(&self, record: SubscriptionRecord) {
        self.lock().insert(record.sub_id.clone(), record);
    }

    async fn remove(&self, sub_id: &str) {
        self.lock().remove(sub_id);
    }

    async fn list(&self) -> Vec<SubscriptionRecord> {
        self.lock().values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(sub_id: &str, state: u8, next: Option<u64>, updated_at: u64) -> SubscriptionRecord {
        SubscriptionRecord {
            sub_id: sub_id.into(),
            state,
            payer: "0xpayer".into(),
            plan_id: String::new(),
            plan_tier: 0,
            next_chargeable_at: next,
            changed_to_sub_id: None,
            start_at: 0,
            period_sec: 0,
            period_mode: 0,
            billing_anchor_at: 0,
            max_periods: 0,
            last_charged_period: None,
            updated_at,
        }
    }

    #[tokio::test]
    async fn put_get_remove_round_trip() {
        let s = InMemorySubscriptionStore::new();
        s.put(rec("0x1", 1, None, 100)).await;
        assert_eq!(s.get("0x1").await.unwrap().state, 1);
        s.remove("0x1").await;
        assert!(s.get("0x1").await.is_none());
    }

    #[tokio::test]
    async fn put_overwrites() {
        let s = InMemorySubscriptionStore::new();
        s.put(rec("0x1", 1, None, 100)).await;
        s.put(rec("0x1", 3, None, 200)).await; // canceled, newer
        let r = s.get("0x1").await.unwrap();
        assert_eq!(r.state, 3);
        assert_eq!(r.updated_at, 200);
    }

    #[tokio::test]
    async fn list_returns_all_records() {
        let s = InMemorySubscriptionStore::new();
        s.put(rec("0x1", 1, Some(1_000), 0)).await;
        s.put(rec("0x2", 3, None, 0)).await;
        let mut ids: Vec<String> = s.list().await.into_iter().map(|r| r.sub_id).collect();
        ids.sort();
        assert_eq!(ids, vec!["0x1".to_string(), "0x2".to_string()]);
    }

    #[test]
    fn is_active_only_for_state_1() {
        assert!(rec("x", 1, None, 0).is_active());
        assert!(!rec("x", 3, None, 0).is_active());
        assert!(!rec("x", 0, None, 0).is_active());
    }

    #[test]
    fn old_record_without_period_fields_deserializes() {
        // A record persisted before the period fields were added must still
        // decode: the new fields fall back to their `serde(default)` values.
        let json = r#"{
            "subId": "0xabc",
            "state": 1,
            "payer": "0xpayer",
            "planId": "pro_monthly",
            "planTier": 2,
            "updatedAt": 100
        }"#;
        let r: SubscriptionRecord = serde_json::from_str(json).unwrap();
        assert_eq!(r.sub_id, "0xabc");
        assert_eq!(r.plan_id, "pro_monthly");
        assert_eq!(r.start_at, 0);
        assert_eq!(r.period_sec, 0);
        assert_eq!(r.period_mode, 0);
        assert_eq!(r.billing_anchor_at, 0);
        assert_eq!(r.max_periods, 0);
        assert_eq!(r.last_charged_period, None);
    }

    #[test]
    fn last_charged_period_none_is_omitted_from_json() {
        let v = serde_json::to_value(rec("0x1", 1, None, 100)).unwrap();
        assert!(v.get("lastChargedPeriod").is_none());
    }
}
