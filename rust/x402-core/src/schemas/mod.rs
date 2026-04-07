//! Schema validation for x402 payment types.
//!
//! Mirrors: `@x402/core/src/schemas/index.ts`
//!
//! In TS, Zod schemas provide runtime validation. In Rust, serde handles
//! deserialization validation. This module provides additional validation
//! functions for business-rule checks beyond what serde can enforce.

use crate::error::X402Error;
use crate::types::{PaymentPayload, PaymentRequired, PaymentRequirements};

/// Validate that a PaymentRequirements has all required fields populated.
pub fn validate_payment_requirements(req: &PaymentRequirements) -> Result<(), X402Error> {
    if req.scheme.is_empty() {
        return Err(X402Error::Config("scheme is required".into()));
    }
    if req.network.is_empty() {
        return Err(X402Error::Config("network is required".into()));
    }
    if req.asset.is_empty() {
        return Err(X402Error::Config("asset is required".into()));
    }
    if req.amount.is_empty() {
        return Err(X402Error::Config("amount is required".into()));
    }
    if req.pay_to.is_empty() {
        return Err(X402Error::Config("payTo is required".into()));
    }
    Ok(())
}

/// Validate that a PaymentPayload has the required structure.
pub fn validate_payment_payload(payload: &PaymentPayload) -> Result<(), X402Error> {
    if payload.x402_version == 0 {
        return Err(X402Error::Config("x402Version is required".into()));
    }
    validate_payment_requirements(&payload.accepted)?;
    if payload.payload.is_empty() {
        return Err(X402Error::Config("payload is required".into()));
    }
    Ok(())
}

/// Validate that a PaymentRequired response has the required structure.
pub fn validate_payment_required(required: &PaymentRequired) -> Result<(), X402Error> {
    if required.x402_version == 0 {
        return Err(X402Error::Config("x402Version is required".into()));
    }
    if required.accepts.is_empty() {
        return Err(X402Error::Config("accepts must not be empty".into()));
    }
    for accept in &required.accepts {
        validate_payment_requirements(accept)?;
    }
    Ok(())
}
