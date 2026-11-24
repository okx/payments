//! `period` scheme — Seller-side support.
//!
//! For subscriptions the Seller:
//! 1. builds the 402 `PaymentRequirements` (scheme `period`);
//! 2. receives the buyer's double-signed `PAYMENT-SIGNATURE` payload and
//!    [`codec`]-unpacks it into a flat facilitator request body;
//! 3. forwards writes (subscribe/charge/change/cancel) to the facilitator via
//!    `SubscriptionFacilitatorClient`;
//! 4. verifies `APP-Access` AccessProof on subsequent access (x402-evm).
//!
//! This module holds framework-agnostic types and codec only; EIP-712/ecrecover
//! live in `x402-evm`.

mod codec;
mod facilitator;
mod period;
mod store;
mod types;

pub use codec::*;
pub use facilitator::*;
pub use period::*;
pub use store::*;
pub use types::*;
