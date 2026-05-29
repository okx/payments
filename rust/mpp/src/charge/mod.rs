//! Charge intent: one-shot pay → settle → done.
//!
//! Three submodules:
//! - [`method`] — [`EvmChargeMethod`], implements `mpp::protocol::traits::ChargeMethod`.
//! - [`challenger`] — [`EvmChargeChallenger`], builds 402 challenges and verifies credentials.
//! - [`challenge`] — `method="evm"` challenge builders shared between charge and session.

pub mod challenge;
pub mod challenger;
pub mod method;

pub use challenger::{EvmChargeChallenger, EvmChargeChallengerConfig};
pub use method::EvmChargeMethod;
