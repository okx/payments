//! `period` scheme (EVM, Seller side): offer building and AccessProof verification.

mod access_proof;
mod plan;
mod server_scheme;

pub use access_proof::{
    verify_access_proof, verify_access_proof_decoded, VerifiedAccess,
    DEFAULT_ACCESS_PROOF_WINDOW_SECS,
};
pub use plan::SubscriptionPlan;
pub use server_scheme::{PermitSubscriptionScheme, SUBSCRIPTION_SCHEME};
