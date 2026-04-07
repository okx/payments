//! x402-core: Core types, server logic, facilitator client, and HTTP utilities
//! for the x402 payment protocol.
//!
//! Mirrors: `@x402/core` from the Coinbase x402 TypeScript SDK.
//!
//! # Modules
//!
//! - [`types`] — Core type definitions (PaymentRequirements, PaymentPayload, etc.)
//! - [`error`] — Unified error types
//! - [`schemas`] — Validation logic (Rust equivalent of Zod schemas)
//! - [`utils`] — Base64 encoding, pattern matching, deep equality
//! - [`server`] — Framework-agnostic server logic (x402ResourceServer)
//! - [`facilitator`] — FacilitatorClient trait definition
//! - [`http`] — HTTP facilitator client, header encoding, route config types

pub mod error;
pub mod facilitator;
pub mod http;
pub mod schemas;
pub mod server;
pub mod types;
pub mod utils;
