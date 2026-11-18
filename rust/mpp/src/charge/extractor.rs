//! Route-price check followed by the existing EVM charge verification pipeline.
use super::EvmChargeChallenger;
use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use mpp::protocol::core::Receipt;
use mpp::server::axum::{
    ChallengeOptions, ChargeChallenger, ChargeConfig, MppChargeRejection, PaymentRequired,
};
use std::{marker::PhantomData, sync::Arc};

pub struct MppCharge<C: ChargeConfig> {
    pub receipt: Receipt,
    _config: PhantomData<C>,
}

impl<S, C> FromRequestParts<S> for MppCharge<C>
where
    S: Send + Sync,
    C: ChargeConfig,
    Arc<EvmChargeChallenger>: FromRef<S>,
{
    type Rejection = MppChargeRejection;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let challenger: Arc<EvmChargeChallenger> = FromRef::from_ref(state);
        if let Some(auth) = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        {
            if let Ok(receipt) = challenger
                .verify_payment_for_amount(auth, C::amount())
                .await
            {
                return Ok(Self {
                    receipt,
                    _config: PhantomData,
                });
            }
        }
        let challenge = challenger
            .challenge(
                C::amount(),
                ChallengeOptions {
                    description: C::description(),
                },
            )
            .map_err(MppChargeRejection::InternalError)?;
        Err(MppChargeRejection::Challenge(PaymentRequired(challenge)))
    }
}
