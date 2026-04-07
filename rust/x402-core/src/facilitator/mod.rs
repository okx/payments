//! Facilitator abstraction layer.
//!
//! Mirrors: `@x402/core/src/facilitator/x402Facilitator.ts`
//!
//! Defines the `FacilitatorClient` trait (interface) for communicating
//! with a remote facilitator. Only the trait is here; the HTTP implementation
//! lives in `crate::http::http_facilitator_client`.

mod x402_facilitator;

pub use x402_facilitator::*;
