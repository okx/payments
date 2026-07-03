//! Subscription support for the payment middleware.
//!
//! Framework-agnostic logic; `middleware.rs` dispatches each operation as an
//! explicit verify → settle pair (mirroring the TS `verifyX`/`settleX` split):
//! - `APP-Access` header → [`verify_access`] → serve (no settle; access is free);
//! - `period` create/change → [`verify_subscription`] → [`settle_subscription`]
//!   (settle-before-serve) → serve + `PAYMENT-RESPONSE`;
//! - cancel → [`SubscriptionSupport::verify_cancel`] → [`SubscriptionSupport::settle_cancel`];
//! - cancel-pending → [`SubscriptionSupport::verify_cancel_pending`] →
//!   [`SubscriptionSupport::settle_cancel_pending`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::json;

use x402_core::http::{AcceptConfig, RoutePaymentConfig};
use x402_core::subscription::{
    change_effective_at, elapsed_periods, subscription_state, to_change_request, to_create_request,
    unpack_subscription_payload, CancelAuth, CancelPendingChangeRequest, CancelSubscriptionRequest,
    PendingChangeCancelAuth, SubscriptionFacilitatorClient, SubscriptionPayloadInner,
    SubscriptionRecord, SubscriptionStatus, SubscriptionStore, SubscriptionTerms, TxResultResponse,
    PERIOD_MODE_CALENDAR_MONTH,
};
use x402_core::types::PaymentPayload;
use x402_evm::subscription::{verify_access_proof, SUBSCRIPTION_SCHEME};

/// Subsequent-access header name.
pub const SUBSCRIPTION_ACCESS_HEADER: &str = "APP-Access";

/// Interval between facilitator polls while a submission is pending.
pub const SUBSCRIPTION_POLL_INTERVAL_SECS: u64 = 1;
/// Max facilitator polls before returning the still-pending state.
pub const SUBSCRIPTION_POLL_MAX_ATTEMPTS: u32 = 5;

/// Subscription wiring: facilitator client, AccessProof replay window, and an
/// optional [`SubscriptionStore`].
#[derive(Clone)]
pub struct SubscriptionSupport {
    facilitator: Arc<dyn SubscriptionFacilitatorClient>,
    access_window_secs: u64,
    store: Option<Arc<dyn SubscriptionStore>>,
    /// Access-cache freshness (secs). `0` = always query the facilitator.
    access_cache_ttl_secs: u64,
    /// Optional merchant access-policy veto; runs after proof verify, before the
    /// period gate.
    on_before_access: Option<OnBeforeAccessHook>,
}

impl SubscriptionSupport {
    pub fn new(facilitator: Arc<dyn SubscriptionFacilitatorClient>, access_window_secs: u64) -> Self {
        Self {
            facilitator,
            access_window_secs,
            store: None,
            access_cache_ttl_secs: 0,
            on_before_access: None,
        }
    }

    /// Set the merchant access-policy veto hook. Runs after proof verify, before
    /// the period gate; `abort = true` denies without consulting period state
    /// (e.g. deny access to a subscription the seller has canceled). See
    /// [`OnBeforeAccessHook`].
    pub fn on_before_access(mut self, hook: OnBeforeAccessHook) -> Self {
        self.on_before_access = Some(hook);
        self
    }

    /// Attach a subscription store: enables write-after-sync, `due_subscriptions`,
    /// and a default 30s access cache.
    pub fn with_store(mut self, store: Arc<dyn SubscriptionStore>) -> Self {
        self.store = Some(store);
        if self.access_cache_ttl_secs == 0 {
            self.access_cache_ttl_secs = 30;
        }
        self
    }

    /// Override the access-cache freshness window (secs). `0` disables the fast
    /// path (always query the facilitator).
    pub fn with_access_cache_ttl(mut self, secs: u64) -> Self {
        self.access_cache_ttl_secs = secs;
        self
    }

    /// Active subscriptions due for charge at `now` (empty if no store attached).
    /// Scans the store's `list()` and keeps active records whose
    /// `next_chargeable_at <= now`. A durable backend with a large subscription
    /// set can push this filter down via its own indexed query instead.
    pub async fn due_subscriptions(&self, now_unix: u64) -> Vec<SubscriptionRecord> {
        match &self.store {
            Some(store) => store
                .list()
                .await
                .into_iter()
                .filter(|r| is_due(r, now_unix))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Charge a subscription and reconcile the store:
    /// - normal charge → refresh the record (advances `next_chargeable_at`, state);
    /// - downgrade activation (`planChangeTriggered`) → record the successor subId
    ///   and refresh the old one so future charges target the new subId.
    ///
    /// On facilitator error the store is left untouched and the error is returned.
    pub async fn charge_and_record(
        &self,
        sub_id: &str,
        sync_settle: bool,
    ) -> Result<ChargeRecordOutcome, String> {
        let resp = self
            .facilitator
            .charge(sub_id, sync_settle)
            .await
            .map_err(|e| format!("{e}"))?;
        if resp.plan_change_triggered {
            if let Some(new_id) = &resp.new_sub_id {
                // Successor sub is now active — record it for future charges.
                let _ = refresh_subscription(self, new_id).await;
            }
        }
        // Refresh the charged sub. If pending, poll for the on-chain result and
        // report the settled state.
        let final_state = if resp.state == subscription_state::PENDING {
            poll_until_settled(self, sub_id)
                .await
                .map(|s| s.state)
                .unwrap_or(resp.state)
        } else {
            let _ = refresh_subscription(self, sub_id).await;
            resp.state
        };
        Ok(ChargeRecordOutcome {
            state: final_state,
            period: resp.period,
            tx_hash: resp.tx_hash,
            plan_change_triggered: resp.plan_change_triggered,
            new_sub_id: resp.new_sub_id,
        })
    }

    /// Verify phase for cancel: fail-fast on a malformed relay
    /// ([`validate_cancel_auth`]).
    pub(crate) fn verify_cancel(&self, auth: &CancelAuth) -> Result<(), String> {
        validate_cancel_auth(auth)
    }

    /// Settle phase for cancel: relay the buyer-signed `CancelAuth` to the
    /// facilitator (the buyer signs; the seller forwards) and refresh the store so
    /// a confirmed cancel (state `CANCELED`) is reflected locally.
    pub(crate) async fn settle_cancel(&self, auth: CancelAuth) -> Result<TxResultResponse, String> {
        let req = CancelSubscriptionRequest {
            sub_id: auth.sub_id.clone(),
            cancel_auth: auth,
            sync_settle: true,
        };
        let resp = self
            .facilitator
            .cancel_subscription(&req)
            .await
            .map_err(|e| format!("{e}"))?;
        let _ = refresh_subscription(self, &resp.sub_id).await;
        Ok(resp)
    }

    /// Verify phase for cancel-pending: fail-fast on a malformed relay
    /// ([`validate_pending_cancel_auth`]).
    pub(crate) fn verify_cancel_pending(
        &self,
        auth: &PendingChangeCancelAuth,
    ) -> Result<(), String> {
        validate_pending_cancel_auth(auth)
    }

    /// Settle phase for cancel-pending: relay a buyer-signed `PendingChangeCancelAuth`
    /// (revert a scheduled downgrade before it activates) to the facilitator.
    pub(crate) async fn settle_cancel_pending(
        &self,
        auth: PendingChangeCancelAuth,
    ) -> Result<TxResultResponse, String> {
        let req = CancelPendingChangeRequest {
            sub_id: auth.sub_id.clone(),
            cancel_auth: auth,
            sync_settle: true,
        };
        let resp = self
            .facilitator
            .cancel_pending_change(&req)
            .await
            .map_err(|e| format!("{e}"))?;
        let _ = refresh_subscription(self, &resp.sub_id).await;
        Ok(resp)
    }
}

/// A record is due for charge when it is active and its next chargeable time has
/// arrived (`next_chargeable_at <= now`). Records with no known next time are
/// skipped until a facilitator refresh backfills it.
fn is_due(r: &SubscriptionRecord, now_unix: u64) -> bool {
    r.is_active() && r.next_chargeable_at.is_some_and(|t| t <= now_unix)
}

/// Result of [`SubscriptionSupport::charge_and_record`].
pub struct ChargeRecordOutcome {
    pub state: u8,
    pub period: u32,
    /// Charge transaction hash from the facilitator (submission-time hash; for a
    /// pending charge it is the submitted tx, sufficient for logging/reconcile).
    /// `None` if the facilitator omitted it.
    pub tx_hash: Option<String>,
    /// `true` on the period a scheduled downgrade activated.
    pub plan_change_triggered: bool,
    /// Successor subId when `plan_change_triggered`; future charges use this.
    pub new_sub_id: Option<String>,
}

/// `true` if this scheme is the subscription scheme (settle-before-serve).
pub fn is_subscription_scheme(scheme: &str) -> bool {
    scheme == SUBSCRIPTION_SCHEME
}

/// Extract the numeric chainIndex from a CAIP-2 network (`eip155:196` → 196).
pub fn chain_index_from_network(network: &str) -> Option<u64> {
    network.strip_prefix("eip155:").and_then(|s| s.parse::<u64>().ok())
}

/// Current Unix time in seconds.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `true` if the buyer's current period has been charged — the single access /
/// settle predicate. Prefers backend `elapsedPeriods`, else computes it from
/// terms. No state gate (no-refund: a paid period stays accessible after cancel;
/// a prepaid COMPLETED sub is accessible while `elapsed <= maxPeriods`).
fn current_period_charged(status: &SubscriptionStatus, now: u64) -> bool {
    let elapsed = if status.elapsed_periods > 0 {
        status.elapsed_periods
    } else {
        elapsed_periods(
            status.period_mode,
            status.start_at,
            status.billing_anchor_at,
            status.period_sec,
            now,
        )
    };
    elapsed > 0 && (status.last_charged_period as u64) >= elapsed
}

/// Poll the facilitator (write-through each tick) until the current period is
/// charged, the submission `FAILED`, or the attempt budget runs out. Returns the
/// latest status (`None` only if every lookup failed). Used after a pending write.
pub async fn poll_until_settled(
    support: &SubscriptionSupport,
    sub_id: &str,
) -> Option<SubscriptionStatus> {
    let mut last = None;
    for attempt in 0..SUBSCRIPTION_POLL_MAX_ATTEMPTS {
        if let Some(status) = refresh_subscription(support, sub_id).await {
            let settled = current_period_charged(&status, now_unix());
            let failed = status.state == subscription_state::FAILED;
            last = Some(status);
            if settled || failed {
                return last;
            }
        }
        // No sleep after the final attempt.
        if attempt + 1 < SUBSCRIPTION_POLL_MAX_ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_secs(SUBSCRIPTION_POLL_INTERVAL_SECS))
                .await;
        }
    }
    last
}

/// Why an `APP-Access` request was denied. Caller maps `NotActive` to a 402 with
/// the (re)subscribe offer, and `Unauthorized` / `Denied` to a bare 402.
pub enum AccessError {
    /// Proof/auth failure (bad signature, replay, wrong owner, lookup error). No
    /// offer (paying can't fix it).
    Unauthorized(String),
    /// Subscription known but not active (expired/canceled); return the offer.
    NotActive(String),
    /// [`OnBeforeAccessHook`] denied access (e.g. after a seller cancel, or a
    /// ban). Hard deny, no offer.
    Denied(String),
}

/// The signature-verified `(subId, payer)` from the AccessProof, passed to
/// [`OnBeforeAccessHook`]. `sub_id` is signature-verified but NOT yet confirmed
/// owned by `payer` on-chain (the period check does that); gate on it only as a
/// merchant-side identifier (access-deny / ban list).
#[derive(Debug, Clone)]
pub struct AccessContext {
    pub sub_id: String,
    pub payer: String,
}

/// Result of [`OnBeforeAccessHook`]. Veto-only: `abort = true` denies before the
/// period gate; it cannot force-grant access (the period gate still applies when
/// `abort = false`).
#[derive(Debug, Clone, Default)]
pub struct BeforeAccessResult {
    /// If true, deny the access request (hard 401, no offer).
    pub abort: bool,
    /// Reason surfaced to the client on abort.
    pub reason: Option<String>,
}

/// Merchant access-policy veto hook: runs on an `APP-Access` request after the
/// proof is verified, before the period gate. `abort = true` denies without
/// consulting period state (e.g. deny access after a seller cancel, or a ban).
///
/// `Arc`-based (not `Box`) so [`SubscriptionSupport`] stays `Clone`.
pub type OnBeforeAccessHook = Arc<
    dyn Fn(
            AccessContext,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = BeforeAccessResult> + Send>>
        + Send
        + Sync,
>;

/// Verify an `APP-Access` AccessProof and confirm the buyer's current period is
/// charged (via the merchant veto, then the period gate). Returns `(subId,
/// planId, planTier)` on success, or an [`AccessError`]. Plan-level gating is
/// applied by the caller.
pub async fn verify_access(
    support: &SubscriptionSupport,
    app_access_header: &str,
    now: u64,
) -> Result<(String, String, u8), AccessError> {
    let verified = verify_access_proof(app_access_header, now, support.access_window_secs)
        .map_err(|e| AccessError::Unauthorized(format!("{e}")))?;

    // Merchant access-policy veto: runs before the period gate; `abort` = hard deny.
    if let Some(hook) = &support.on_before_access {
        let result = hook(AccessContext {
            sub_id: verified.sub_id.clone(),
            payer: verified.payer.clone(),
        })
        .await;
        if result.abort {
            let reason = result.reason.unwrap_or_else(|| {
                format!("access denied by merchant policy for subscription {}", verified.sub_id)
            });
            return Err(AccessError::Denied(reason));
        }
    }

    // Fast path: serve from a fresh cached record without a facilitator round-trip.
    // Predicate is the same period gate; `currentPeriod` is recomputed from the
    // clock, `lastChargedPeriod` read from cache. Bounded by `access_cache_ttl_secs`;
    // any miss falls through to the slow path.
    if let Some(store) = &support.store {
        if support.access_cache_ttl_secs > 0 {
            if let Some(rec) = store.get(&verified.sub_id).await {
                let fresh =
                    now.saturating_sub(rec.updated_at) <= support.access_cache_ttl_secs;
                let owner_ok =
                    !rec.payer.is_empty() && rec.payer.eq_ignore_ascii_case(&verified.payer);
                // Local period calc needs the real start; `start_at == 0` →
                // slow path (which backfills it). Calendar-month subs also need
                // the anchor: `billing_anchor_at == 0` (not yet refreshed) →
                // slow path, since falling back to `startAt` would drift the
                // month-end rhythm and mis-count the period.
                let anchor_known =
                    rec.period_mode != PERIOD_MODE_CALENDAR_MONTH || rec.billing_anchor_at > 0;
                if fresh && owner_ok && !rec.plan_id.is_empty() && rec.start_at > 0 && anchor_known {
                    if let Some(lcp) = rec.last_charged_period {
                        let current = elapsed_periods(
                            rec.period_mode,
                            rec.start_at,
                            rec.billing_anchor_at,
                            rec.period_sec,
                            now,
                        );
                        if current > 0 && (lcp as u64) >= current {
                            return Ok((verified.sub_id, rec.plan_id, rec.plan_tier));
                        }
                    }
                }
            }
        }
    }

    // Authoritative: query the facilitator, then write-through to the store.
    let status = support
        .facilitator
        .get_subscription(&verified.sub_id)
        .await
        .map_err(|e| AccessError::Unauthorized(format!("subscription lookup failed: {e}")))?;

    // Ownership: the proof signer must be the subscription's payer (skip only
    // when the backend omits it).
    if !status.payer.is_empty() && !status.payer.eq_ignore_ascii_case(&verified.payer) {
        return Err(AccessError::Unauthorized(format!(
            "AccessProof payer {} does not own subscription {}",
            verified.payer, verified.sub_id
        )));
    }

    // Write-through the authoritative status; returns the resolved plan id/tier.
    let (plan_id, plan_tier) =
        record_status(support, &verified.sub_id, &status, &verified.payer, now).await;

    // Period gate on fresh data.
    if !current_period_charged(&status, now) {
        return Err(AccessError::NotActive(format!(
            "subscription {} current period not charged (lastCharged={} elapsed={})",
            verified.sub_id, status.last_charged_period, status.elapsed_periods
        )));
    }
    Ok((verified.sub_id, plan_id, plan_tier))
}

/// Write a facilitator [`SubscriptionStatus`] through to the store, preserving a
/// locally-recorded plan id/tier when the status omits it and using
/// `fallback_payer` when it carries none. Returns the resolved `(plan_id,
/// plan_tier)`. No-op when no store is attached.
async fn record_status(
    support: &SubscriptionSupport,
    sub_id: &str,
    status: &SubscriptionStatus,
    fallback_payer: &str,
    now: u64,
) -> (String, u8) {
    let mut plan_id = status.plan_id.clone();
    let mut plan_tier = status.plan_tier;
    if let Some(store) = &support.store {
        // start_at / billing_anchor_at are backfilled on-chain (0 = pending):
        // keep a prior real value rather than clobbering it back to 0.
        let mut start_at = status.start_at;
        let mut billing_anchor_at = status.billing_anchor_at;
        if let Some(prior) = store.get(sub_id).await {
            if plan_id.is_empty() {
                plan_id = prior.plan_id.clone();
            }
            if plan_tier == 0 {
                plan_tier = prior.plan_tier;
            }
            if start_at == 0 {
                start_at = prior.start_at;
            }
            if billing_anchor_at == 0 {
                billing_anchor_at = prior.billing_anchor_at;
            }
        }
        let payer = if status.payer.is_empty() {
            fallback_payer.to_string()
        } else {
            status.payer.clone()
        };
        store
            .put(SubscriptionRecord {
                sub_id: sub_id.to_string(),
                state: status.state,
                payer,
                plan_id: plan_id.clone(),
                plan_tier,
                next_chargeable_at: status.next_chargeable_at,
                changed_to_sub_id: status.changed_to_sub_id.clone(),
                start_at,
                period_sec: status.period_sec,
                period_mode: status.period_mode,
                billing_anchor_at,
                max_periods: status.max_periods,
                // `Some(0)` (known: never charged) is distinct from `None` (not
                // yet fetched). `elapsed_periods` is not persisted — the access
                // path recomputes it from the clock.
                last_charged_period: Some(status.last_charged_period),
                updated_at: now,
            })
            .await;
    }
    (plan_id, plan_tier)
}

/// Query the facilitator for `sub_id`'s latest status and write it through to the
/// store. Used after subscribe / change / charge so scheduling doesn't wait on an
/// `APP-Access`. Best-effort: returns `None` (store untouched) if the lookup fails.
pub async fn refresh_subscription(
    support: &SubscriptionSupport,
    sub_id: &str,
) -> Option<SubscriptionStatus> {
    let status = support.facilitator.get_subscription(sub_id).await.ok()?;
    record_status(support, sub_id, &status, "", now_unix()).await;
    Some(status)
}

/// The plan ids a route accepts: the `extra.plan.id` of each `period` accept.
/// Membership is tested via [`plan_id_accepted`].
pub fn accepted_plan_ids(route_config: &RoutePaymentConfig) -> Vec<String> {
    route_config
        .accepts
        .iter()
        .filter(|a| is_subscription_scheme(&a.scheme))
        .filter_map(|a| {
            a.extra
                .as_ref()
                .and_then(|e| e.get("plan"))
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .collect()
}

/// Build the change-offer accepts for a `/changePlan` route. Each candidate plan
/// gets `extra.changeFrom` injected; the plan the buyer is on is dropped (the
/// contract rejects same-tier changes, so plan tiers MUST be unique).
///
/// - `from_plan_tier < plan.tier` → `upgrade` / `immediate`
/// - `from_plan_tier > plan.tier` → `downgrade` / `period_end`
/// - equal (same plan/tier) → dropped
pub fn build_change_accepts(
    route_config: &RoutePaymentConfig,
    from_sub_id: &str,
    from_plan_id: &str,
    from_plan_tier: u8,
) -> Vec<AcceptConfig> {
    // Warn on duplicate plan tiers (same-tier changes are contract-rejected).
    let mut seen_tiers = Vec::new();
    for a in &route_config.accepts {
        if !is_subscription_scheme(&a.scheme) {
            continue;
        }
        if let Some(t) = a
            .extra
            .as_ref()
            .and_then(|e| e.get("plan"))
            .and_then(|p| p.get("tier"))
            .and_then(|v| v.as_u64())
        {
            if seen_tiers.contains(&t) {
                tracing::warn!(
                    "[x402-sub] change-plan: duplicate plan.tier={} in route accepts — \
                     changes to/from a same-tier plan are not possible (use unique tiers)",
                    t
                );
            }
            seen_tiers.push(t);
        }
    }

    route_config
        .accepts
        .iter()
        .filter(|a| is_subscription_scheme(&a.scheme))
        .filter_map(|a| {
            let extra = a.extra.as_ref()?;
            let plan = extra.get("plan")?;
            let plan_id = plan.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let plan_tier = plan.get("tier").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            // Drop the plan the buyer is already on (same plan id or same tier).
            if plan_id == from_plan_id || plan_tier == from_plan_tier {
                return None;
            }
            let (direction, effective_at) = if from_plan_tier < plan_tier {
                ("upgrade", "immediate")
            } else {
                ("downgrade", "period_end")
            };
            let mut accept = a.clone();
            let extra_mut = accept.extra.get_or_insert_with(HashMap::new);
            // A downgrade offer must drop `initialCharge`: the contract rejects
            // scheduleDowngrade with a non-zero initial charge (the cheaper plan's
            // first period is charged on activation). An upgrade keeps it (charged
            // now).
            if direction == "downgrade" {
                extra_mut.remove("initialCharge");
            }
            extra_mut.insert(
                "changeFrom".to_string(),
                json!({
                    "fromSubId": from_sub_id,
                    "fromPlanId": from_plan_id,
                    "fromPlanTier": from_plan_tier,
                    "direction": direction,
                    "effectiveAt": effective_at,
                }),
            );
            Some(accept)
        })
        .collect()
}

/// `true` if a subscription's `planId` is among the route's accepted plan ids
/// (matched verbatim).
pub fn plan_id_accepted(accepted_plain_ids: &[String], sub_plan_id: &str) -> bool {
    accepted_plain_ids.iter().any(|p| p == sub_plan_id)
}

/// Result of a subscribe-or-change submission. `sub_id` is the live subscription
/// id (new subId on create, successor subId on change).
pub struct SubmitOutcome {
    pub sub_id: String,
    pub tx_hash: Option<String>,
    pub state: u8,
}

/// Cancel-auth well-formedness (local): a relay must carry a subId. Signature /
/// deadline / state are adjudicated by the contract on settle.
fn validate_cancel_auth(auth: &CancelAuth) -> Result<(), String> {
    if auth.sub_id.trim().is_empty() {
        return Err("cancel auth missing subId".into());
    }
    Ok(())
}

/// Pending-cancel-auth well-formedness (local): must carry both the subId and the
/// pending `newSubId` the auth is bound to.
fn validate_pending_cancel_auth(auth: &PendingChangeCancelAuth) -> Result<(), String> {
    if auth.sub_id.trim().is_empty() {
        return Err("cancel-pending auth missing subId".into());
    }
    if auth.new_sub_id.trim().is_empty() {
        return Err("cancel-pending auth missing newSubId".into());
    }
    Ok(())
}

/// `true` if a `bytes32` is all-zero (`0x0…0`) — i.e. NOT a change.
fn is_zero_bytes32(s: &str) -> bool {
    let t = s.trim().trim_start_matches("0x");
    t.is_empty() || t.chars().all(|c| c == '0')
}

/// Bind the buyer-signed terms to an advertised plan on the route: the facilitator
/// verifies the signature but can't see the offer, so a buyer could otherwise sign
/// cheaper/mismatched economics for a plan the route gates by `planId` alone. Match
/// the offer plan by `planId`, then check merchant + per-period economics + token +
/// initial charge (fields an honest buyer copies verbatim). `offer_asset` is the
/// advertised token (`PaymentRequirements.asset`); empty = seller didn't advertise
/// one, so the token check is skipped. `startAt` is intentionally NOT compared (an
/// unset offer lets the contract resolve `0` → block.timestamp).
fn verify_terms_match_offer(
    terms: &SubscriptionTerms,
    offer_accepts: &[AcceptConfig],
    offer_asset: &str,
) -> Result<(), String> {
    let accept = offer_accepts
        .iter()
        .filter(|a| is_subscription_scheme(&a.scheme))
        .find(|a| {
            a.extra
                .as_ref()
                .and_then(|e| e.get("plan"))
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_str())
                == Some(terms.plan_id.as_str())
        })
        .ok_or_else(|| format!("planId `{}` is not offered on this route", terms.plan_id))?;
    let extra = accept
        .extra
        .as_ref()
        .ok_or("offered plan has no extra to bind against")?;

    if !accept.pay_to.eq_ignore_ascii_case(&terms.merchant) {
        return Err(format!(
            "terms.merchant {} does not match offer payTo {}",
            terms.merchant, accept.pay_to
        ));
    }
    let u64_field = |k: &str| extra.get(k).and_then(serde_json::Value::as_u64);
    let str_field = |k: &str| extra.get(k).and_then(|v| v.as_str());
    let plan_tier = extra
        .get("plan")
        .and_then(|p| p.get("tier"))
        .and_then(serde_json::Value::as_u64);

    if str_field("amountPerPeriod") != Some(terms.amount_per_period.as_str()) {
        return Err(format!(
            "terms.amountPerPeriod {} does not match offer {:?}",
            terms.amount_per_period,
            str_field("amountPerPeriod")
        ));
    }
    if u64_field("periodSec") != Some(terms.period_sec) {
        return Err("terms.periodSec does not match offer".into());
    }
    if u64_field("maxPeriods") != Some(terms.max_periods as u64) {
        return Err("terms.maxPeriods does not match offer".into());
    }
    if u64_field("periodMode").unwrap_or(0) != terms.period_mode as u64 {
        return Err("terms.periodMode does not match offer".into());
    }
    if plan_tier != Some(terms.plan_tier as u64) {
        return Err("terms.planTier does not match offer".into());
    }

    // Token: bind to the advertised asset. The contract does NOT pin the token
    // (it's a signed `terms` field), so a buyer could otherwise pay the merchant
    // in a worthless token. Empty `offer_asset` = seller advertised none → skip.
    // On a change the contract locks the token to the original sub
    // (`TokenChanged`); this primarily guards the create path.
    if !offer_asset.is_empty() && !offer_asset.eq_ignore_ascii_case(&terms.token) {
        return Err(format!(
            "terms.token {} does not match offer asset {}",
            terms.token, offer_asset
        ));
    }

    // Initial charge: bound on create only. On a change the upfront charge varies
    // (upgrade charges the new plan's first period now; downgrade must be 0 and is
    // contract-enforced), so binding it against the base offer would false-reject.
    if is_zero_bytes32(&terms.change_from_sub_id) {
        let (offer_periods, offer_amount) = extra
            .get("initialCharge")
            .map(|ic| {
                (
                    ic.get("periodCount").and_then(serde_json::Value::as_u64).unwrap_or(0),
                    ic.get("totalAmount").and_then(|v| v.as_str()).unwrap_or("0"),
                )
            })
            .unwrap_or((0, "0"));
        if terms.initial_charge_periods as u64 != offer_periods {
            return Err(format!(
                "terms.initialChargePeriods {} does not match offer {}",
                terms.initial_charge_periods, offer_periods
            ));
        }
        if terms.initial_charge_amount != offer_amount {
            return Err(format!(
                "terms.initialChargeAmount {} does not match offer {}",
                terms.initial_charge_amount, offer_amount
            ));
        }
    }
    Ok(())
}

/// A subscribe/change payload that has passed the local verify phase (unpacked +
/// bound to the advertised offer). Ready for [`settle_subscription`]; no network
/// I/O has happened yet.
pub(crate) struct VerifiedSubscription {
    inner: SubscriptionPayloadInner,
}

/// Verify phase (local, no network): unpack the buyer's `PAYMENT-SIGNATURE`
/// payload and bind the signed terms to an advertised plan (`offer_accepts` +
/// `offer_asset`). Fails fast before any facilitator round-trip.
pub(crate) fn verify_subscription(
    payment_payload: &PaymentPayload,
    offer_accepts: &[AcceptConfig],
    offer_asset: &str,
) -> Result<VerifiedSubscription, String> {
    let inner = unpack_subscription_payload(payment_payload)
        .map_err(|e| format!("invalid subscription payload: {e}"))?;
    verify_terms_match_offer(&inner.terms, offer_accepts, offer_asset)
        .map_err(|e| format!("subscription terms do not match the offer: {e}"))?;
    Ok(VerifiedSubscription { inner })
}

/// Settle phase (facilitator + store): forward a [`VerifiedSubscription`] to the
/// facilitator, routing by `terms.changeFromSubId` (all-zero → `create_subscription`;
/// non-zero → `change_subscription`), then write-after-sync the store.
pub(crate) async fn settle_subscription(
    support: &SubscriptionSupport,
    verified: VerifiedSubscription,
    chain_index: u64,
    sync_settle: bool,
) -> Result<SubmitOutcome, String> {
    let inner = verified.inner;
    // Capture before `inner` is moved into the request builder.
    let payer = inner.terms.payer.clone();
    let plan_id = inner.terms.plan_id.clone();
    let plan_tier = inner.terms.plan_tier;
    // Period params come from the signed terms. `start_at` may be 0 (chain uses
    // block.timestamp); `billing_anchor_at` isn't in terms — both stay 0 until
    // the post-submit refresh backfills them.
    let start_at = inner.terms.start_at;
    let period_sec = inner.terms.period_sec;
    let period_mode = inner.terms.period_mode;
    let max_periods = inner.terms.max_periods;
    let is_change = !is_zero_bytes32(&inner.terms.change_from_sub_id);
    // A downgrade stays pending until the period boundary — don't block-poll on it.
    let is_downgrade =
        is_change && inner.terms.change_effective_at == change_effective_at::PERIOD_END;

    let mut outcome = if is_zero_bytes32(&inner.terms.change_from_sub_id) {
        // Create — new subscription.
        let req = to_create_request(chain_index, inner, sync_settle);
        let resp = support
            .facilitator
            .create_subscription(&req)
            .await
            .map_err(|e| format!("{e}"))?;
        SubmitOutcome { sub_id: resp.sub_id, tx_hash: resp.tx_hash, state: resp.state }
    } else {
        // Change — up/downgrade; old subId is carried in terms.changeFromSubId.
        let old_sub_id = inner.terms.change_from_sub_id.clone();
        let req = to_change_request(chain_index, old_sub_id, inner, sync_settle);
        let resp = support
            .facilitator
            .change_subscription(&req)
            .await
            .map_err(|e| format!("{e}"))?;
        SubmitOutcome { sub_id: resp.new_sub_id, tx_hash: resp.tx_hash, state: resp.state }
    };

    // Write-after-sync: record the live subscription for the access cache and
    // `due_subscriptions`.
    if let Some(store) = &support.store {
        store
            .put(SubscriptionRecord {
                sub_id: outcome.sub_id.clone(),
                state: outcome.state,
                payer,
                plan_id,
                plan_tier,
                next_chargeable_at: None,
                changed_to_sub_id: None,
                start_at,
                period_sec,
                period_mode,
                billing_anchor_at: 0,
                max_periods,
                // Not fetched yet — forces the access slow path until the refresh
                // below fills it in.
                last_charged_period: None,
                updated_at: now_unix(),
            })
            .await;
        // Refresh so the scheduler and access path see this sub without waiting
        // for the first `APP-Access`. Poll a pending create/upgrade for the first
        // charge; a downgrade stays pending until the boundary, so single refresh.
        if outcome.state == subscription_state::PENDING && !is_downgrade {
            if let Some(status) = poll_until_settled(support, &outcome.sub_id).await {
                outcome.state = status.state;
            }
        } else {
            let _ = refresh_subscription(support, &outcome.sub_id).await;
        }
    }

    Ok(outcome)
}

/// Base64(JSON) `PAYMENT-RESPONSE` value for a created/changed subscription.
pub fn encode_subscription_response_header(outcome: &SubmitOutcome) -> Option<String> {
    let body = json!({
        "subId": outcome.sub_id,
        "txHash": outcome.tx_hash,
        "state": outcome.state,
    });
    serde_json::to_vec(&body).ok().map(|b| B64.encode(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_subscription_scheme_matches() {
        assert!(is_subscription_scheme("period"));
        assert!(!is_subscription_scheme("exact"));
        assert!(!is_subscription_scheme("upto"));
    }

    #[test]
    fn is_due_only_for_active_and_arrived() {
        let mut r = SubscriptionRecord {
            sub_id: "0x1".into(),
            state: 1,
            payer: "0xp".into(),
            plan_id: String::new(),
            plan_tier: 0,
            next_chargeable_at: Some(1_000),
            changed_to_sub_id: None,
            start_at: 0,
            period_sec: 0,
            period_mode: 0,
            billing_anchor_at: 0,
            max_periods: 0,
            last_charged_period: None,
            updated_at: 0,
        };
        assert!(is_due(&r, 5_000)); // active + arrived
        assert!(!is_due(&r, 500)); // active but not yet due
        r.next_chargeable_at = None;
        assert!(!is_due(&r, 5_000)); // active but next time unknown
        r.next_chargeable_at = Some(1_000);
        r.state = 3;
        assert!(!is_due(&r, 5_000)); // due time but canceled
    }

    #[test]
    fn plan_id_accepted_matches_plain_ids() {
        let accepted = vec!["basic_monthly".to_string(), "pro_monthly".to_string()];
        assert!(plan_id_accepted(&accepted, "basic_monthly"));
        assert!(plan_id_accepted(&accepted, "pro_monthly"));
        assert!(!plan_id_accepted(&accepted, "pro_yearly"));
        assert!(!plan_id_accepted(&accepted, ""));
    }

    #[test]
    fn chain_index_from_network_parses_eip155() {
        assert_eq!(chain_index_from_network("eip155:196"), Some(196));
        assert_eq!(chain_index_from_network("eip155:1952"), Some(1952));
        assert_eq!(chain_index_from_network("solana:xyz"), None);
        assert_eq!(chain_index_from_network("eip155:abc"), None);
    }

    #[test]
    fn encode_response_header_round_trips() {
        let resp = SubmitOutcome {
            sub_id: "0xsub".into(),
            tx_hash: Some("0xtx".into()),
            state: 1,
        };
        let header = encode_subscription_response_header(&resp).unwrap();
        let decoded = B64.decode(&header).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(v["subId"], "0xsub");
        assert_eq!(v["state"], 1);
        assert_eq!(v["txHash"], "0xtx");
    }

    /// Build a `SubscriptionStatus` from the fields `current_period_charged`
    /// reads (everything else falls back to `serde(default)`).
    fn status(
        state: u8,
        elapsed: u64,
        last_charged: u32,
        period_mode: u8,
        start_at: u64,
        period_sec: u64,
        max_periods: u32,
    ) -> SubscriptionStatus {
        serde_json::from_value(json!({
            "subId": "0xsub",
            "state": state,
            "elapsedPeriods": elapsed,
            "lastChargedPeriod": last_charged,
            "periodMode": period_mode,
            "startAt": start_at,
            "periodSec": period_sec,
            "maxPeriods": max_periods,
        }))
        .unwrap()
    }

    #[test]
    fn prepaid_completed_in_window_is_granted() {
        // Annual prepaid: charged all 12 periods up front, now COMPLETED(2).
        // The old `is_active` gate denied this (the year-pay access bug); the
        // period rule grants while elapsed <= maxPeriods.
        use subscription_state::COMPLETED;
        assert!(current_period_charged(&status(COMPLETED, 1, 12, 0, 1000, 2_592_000, 12), 0));
        assert!(current_period_charged(&status(COMPLETED, 12, 12, 0, 1000, 2_592_000, 12), 0));
    }

    #[test]
    fn prepaid_expired_is_denied() {
        // elapsed (13) > maxPeriods (12) → window ended; lastCharged(12) < 13.
        use subscription_state::COMPLETED;
        assert!(!current_period_charged(&status(COMPLETED, 13, 12, 0, 1000, 2_592_000, 12), 0));
    }

    #[test]
    fn monthly_current_period_paid_or_unpaid() {
        use subscription_state::ACTIVE;
        // Charged through the current period → granted.
        assert!(current_period_charged(&status(ACTIVE, 3, 3, 0, 1000, 2_592_000, 12), 0));
        // Current period not yet charged → denied (the per-period gate).
        assert!(!current_period_charged(&status(ACTIVE, 3, 2, 0, 1000, 2_592_000, 12), 0));
    }

    #[test]
    fn not_started_is_denied() {
        // elapsed == 0 (pre-start) → denied even with the `> 0` guard.
        use subscription_state::ACTIVE;
        assert!(!current_period_charged(&status(ACTIVE, 0, 0, 0, 0, 0, 12), 0));
    }

    #[test]
    fn backend_omits_elapsed_falls_back_to_local() {
        // elapsedPeriods == 0 (older backend) → derive locally from terms.
        // fixed: (now - startAt)/periodSec + 1 = (1350-1000)/100 + 1 = 4.
        use subscription_state::ACTIVE;
        assert!(current_period_charged(&status(ACTIVE, 0, 4, 0, 1000, 100, 12), 1350));
        assert!(!current_period_charged(&status(ACTIVE, 0, 3, 0, 1000, 100, 12), 1350));
    }

    fn sub_accept(plan_id: &str, tier: u64) -> x402_core::http::AcceptConfig {
        let mut extra = std::collections::HashMap::new();
        extra.insert("plan".to_string(), json!({ "id": plan_id, "tier": tier }));
        extra.insert(
            "initialCharge".to_string(),
            json!({ "periodCount": 1, "totalAmount": "5000" }),
        );
        x402_core::http::AcceptConfig {
            scheme: SUBSCRIPTION_SCHEME.to_string(),
            price: "0".into(),
            network: "eip155:196".into(),
            pay_to: "0xrecipient".into(),
            max_timeout_seconds: None,
            extra: Some(extra),
        }
    }

    #[test]
    fn change_accepts_strip_initial_charge_on_downgrade_only() {
        let route = x402_core::http::RoutePaymentConfig {
            accepts: vec![sub_accept("basic_monthly", 1), sub_accept("pro_monthly", 2)],
            ..Default::default()
        };
        // Downgrade (current pro tier2 → basic tier1): the offered basic plan must
        // drop its initialCharge — the contract rejects scheduleDowngrade with a
        // non-zero initialChargeAmount (the year-pay/downgrade access bug class).
        let down = build_change_accepts(&route, "0xsub", "pro_monthly", 2);
        assert_eq!(down.len(), 1);
        let e = down[0].extra.as_ref().unwrap();
        assert_eq!(e["changeFrom"]["direction"], "downgrade");
        assert!(
            !e.contains_key("initialCharge"),
            "downgrade offer must not carry an initial charge"
        );
        // Upgrade (current basic → pro): the offered pro plan KEEPS its
        // initialCharge — an immediate upgrade charges the new first period now.
        let up = build_change_accepts(&route, "0xsub", "basic_monthly", 1);
        assert_eq!(up.len(), 1);
        let e = up[0].extra.as_ref().unwrap();
        assert_eq!(e["changeFrom"]["direction"], "upgrade");
        assert!(
            e.contains_key("initialCharge"),
            "immediate upgrade offer keeps the new plan's first-period charge"
        );
    }

    #[tokio::test]
    async fn on_before_access_hook_vetoes_by_sub_id() {
        // A merchant policy hook (the shape a seller implements): deny a subId's
        // access, allow everything else. Confirms the public hook type is
        // constructible from a plain async closure and the veto/allow semantics.
        let hook: OnBeforeAccessHook = Arc::new(|ctx: AccessContext| {
            Box::pin(async move {
                if ctx.sub_id == "0xdenied" {
                    BeforeAccessResult { abort: true, reason: Some("access denied by merchant".into()) }
                } else {
                    BeforeAccessResult::default()
                }
            })
        });

        let denied = hook(AccessContext { sub_id: "0xdenied".into(), payer: "0xp".into() }).await;
        assert!(denied.abort);
        assert_eq!(denied.reason.as_deref(), Some("access denied by merchant"));

        let allowed = hook(AccessContext { sub_id: "0xactive".into(), payer: "0xp".into() }).await;
        assert!(!allowed.abort);
        assert!(allowed.reason.is_none());
    }

    fn offer_accept() -> x402_core::http::AcceptConfig {
        let mut extra = std::collections::HashMap::new();
        extra.insert("plan".to_string(), json!({ "id": "pro_monthly", "tier": 2 }));
        extra.insert("amountPerPeriod".to_string(), json!("20000"));
        extra.insert("periodSec".to_string(), json!(2_592_000));
        extra.insert("maxPeriods".to_string(), json!(12));
        extra.insert("periodMode".to_string(), json!(0));
        extra.insert(
            "initialCharge".to_string(),
            json!({ "periodCount": 1, "totalAmount": "20000", "coversFirstPeriods": true }),
        );
        x402_core::http::AcceptConfig {
            scheme: SUBSCRIPTION_SCHEME.to_string(),
            price: "0".into(),
            network: "eip155:196".into(),
            pay_to: "0xMerchant".into(),
            max_timeout_seconds: None,
            extra: Some(extra),
        }
    }

    fn terms(plan_id: &str, tier: u8, amount: &str, merchant: &str) -> SubscriptionTerms {
        serde_json::from_value(json!({
            "payer": "0xp", "merchant": merchant, "facilitator": "0xf", "token": "0xt",
            "amountPerPeriod": amount, "periodSec": 2_592_000u64, "maxPeriods": 12,
            "startAt": 0, "initialChargePeriods": 1, "initialChargeAmount": amount,
            "termsDeadline": 0, "permitHash": "0x00", "salt": "0x00",
            "planId": plan_id, "planTier": tier, "changeFromSubId": "0x00",
            "changeEffectiveAt": 0, "periodMode": 0
        }))
        .unwrap()
    }

    #[test]
    fn verify_terms_match_offer_accepts_and_rejects() {
        let accepts = vec![offer_accept()];
        let asset = "0xt"; // matches the token in `terms(..)`
        // Honest terms copied from the offer → OK.
        assert!(verify_terms_match_offer(&terms("pro_monthly", 2, "20000", "0xMerchant"), &accepts, asset).is_ok());
        // Tampered per-period amount (underpay) → rejected.
        assert!(verify_terms_match_offer(&terms("pro_monthly", 2, "1", "0xMerchant"), &accepts, asset).is_err());
        // Wrong merchant (funds redirect) → rejected.
        assert!(verify_terms_match_offer(&terms("pro_monthly", 2, "20000", "0xEvil"), &accepts, asset).is_err());
        // planId not offered on this route → rejected.
        assert!(verify_terms_match_offer(&terms("enterprise", 2, "20000", "0xMerchant"), &accepts, asset).is_err());
        // Tampered tier → rejected.
        assert!(verify_terms_match_offer(&terms("pro_monthly", 3, "20000", "0xMerchant"), &accepts, asset).is_err());
        // Worthless-token substitution (advertised asset != terms.token) → rejected.
        assert!(verify_terms_match_offer(&terms("pro_monthly", 2, "20000", "0xMerchant"), &accepts, "0xOtherToken").is_err());
        // Empty advertised asset → token check skipped, still OK.
        assert!(verify_terms_match_offer(&terms("pro_monthly", 2, "20000", "0xMerchant"), &accepts, "").is_ok());
    }

    #[test]
    fn verify_terms_binds_initial_charge_on_create() {
        let accepts = vec![offer_accept()]; // offer: initialCharge 1 / "20000"
        // Create with a tampered (zeroed) initial charge but honest per-period
        // amount → rejected (buyer would skip the advertised upfront charge).
        let tampered: SubscriptionTerms = serde_json::from_value(json!({
            "payer": "0xp", "merchant": "0xMerchant", "facilitator": "0xf", "token": "0xt",
            "amountPerPeriod": "20000", "periodSec": 2_592_000u64, "maxPeriods": 12,
            "startAt": 0, "initialChargePeriods": 0, "initialChargeAmount": "0",
            "termsDeadline": 0, "permitHash": "0x00", "salt": "0x00",
            "planId": "pro_monthly", "planTier": 2, "changeFromSubId": "0x00",
            "changeEffectiveAt": 0, "periodMode": 0
        })).unwrap();
        assert!(verify_terms_match_offer(&tampered, &accepts, "0xt").is_err());

        // Same tampered initial charge but on a CHANGE (non-zero changeFromSubId)
        // → not bound (contract governs the upfront charge), so it passes here.
        let mut on_change = tampered.clone();
        on_change.change_from_sub_id = "0xabc".into();
        assert!(verify_terms_match_offer(&on_change, &accepts, "0xt").is_ok());
    }

    #[test]
    fn cancel_auth_validation() {
        let ok = CancelAuth {
            action: 0, sub_id: "0xsub".into(), initiator: 0,
            nonce: "1".into(), deadline: 0, signature: "0x".into(),
        };
        assert!(validate_cancel_auth(&ok).is_ok());
        let mut bad = ok.clone();
        bad.sub_id = "  ".into();
        assert!(validate_cancel_auth(&bad).is_err());
    }

    #[test]
    fn pending_cancel_auth_validation() {
        let ok = PendingChangeCancelAuth {
            sub_id: "0xsub".into(), new_sub_id: "0xnew".into(),
            nonce: "1".into(), deadline: 0, signature: "0x".into(),
        };
        assert!(validate_pending_cancel_auth(&ok).is_ok());
        // Missing newSubId → rejected (the auth binds it).
        let mut no_new = ok.clone();
        no_new.new_sub_id = String::new();
        assert!(validate_pending_cancel_auth(&no_new).is_err());
    }
}
