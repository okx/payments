//! `SubscriptionFacilitatorClient` — the subscription write/read endpoints the
//! Seller calls on the facilitator, with OK-ACCESS auth.
//!
//! Kept separate from the base [`crate::facilitator::FacilitatorClient`]
//! (verify/settle) so exact/upto impls need not implement subscription methods.
//! `OkxHttpFacilitatorClient` implements both.

use async_trait::async_trait;

use crate::error::X402Error;

use super::types::{
    ChangeResponse, ChangeSubscriptionRequest, ChargeResponse, ChargesResponse,
    CancelPendingChangeRequest, CancelSubscriptionRequest, CreateSubscriptionRequest,
    CreateSubscriptionResponse, PendingPlanChange, SubscriptionStatus, TxResultResponse,
};

/// Subscription endpoints (`/api/v6/pay/x402/subscriptions/*`). Writes are
/// authenticated with the Seller's OK-ACCESS API key; the server derives
/// `callerMerchantId` from the key (the SDK never sends merchantId).
#[async_trait]
pub trait SubscriptionFacilitatorClient: Send + Sync {
    /// `POST /subscriptions` — create and force-charge the first period.
    async fn create_subscription(
        &self,
        req: &CreateSubscriptionRequest,
    ) -> Result<CreateSubscriptionResponse, X402Error>;

    /// `POST /subscriptions/charge` — periodic charge.
    async fn charge(&self, sub_id: &str, sync_settle: bool)
        -> Result<ChargeResponse, X402Error>;

    /// `POST /subscriptions/change` — up/downgrade.
    async fn change_subscription(
        &self,
        req: &ChangeSubscriptionRequest,
    ) -> Result<ChangeResponse, X402Error>;

    /// `POST /subscriptions/cancel` — cancel with `CancelAuth`.
    async fn cancel_subscription(
        &self,
        req: &CancelSubscriptionRequest,
    ) -> Result<TxResultResponse, X402Error>;

    /// `POST /subscriptions/cancel-pending-change` — cancel a not-yet-effective
    /// downgrade (payer-signed).
    async fn cancel_pending_change(
        &self,
        req: &CancelPendingChangeRequest,
    ) -> Result<TxResultResponse, X402Error>;

    /// `POST /subscriptions/finalize-expired` — release the reservation of an
    /// ended subscription (any caller).
    async fn finalize_expired(&self, sub_id: &str) -> Result<TxResultResponse, X402Error>;

    /// `GET /subscriptions/detail?subId=` — current status.
    async fn get_subscription(&self, sub_id: &str) -> Result<SubscriptionStatus, X402Error>;

    /// `GET /subscriptions/charges?subId=&limit=&offset=` — charge ledger.
    async fn get_charges(
        &self,
        sub_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<ChargesResponse, X402Error>;

    /// `GET /subscriptions/pending?subId=` — the PENDING downgrade, or `None`.
    async fn get_pending_change(
        &self,
        sub_id: &str,
    ) -> Result<Option<PendingPlanChange>, X402Error>;
}
