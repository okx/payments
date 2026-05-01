//! Extension trait definitions for the x402 protocol.
//!
//! Mirrors: `@x402/core/src/types/extensions.ts`

use async_trait::async_trait;
use std::collections::HashMap;

use super::{Network, PaymentPayload, PaymentRequired, PaymentRequirements, SettleResponse};

/// Context available when building PaymentRequired responses.
///
/// Mirrors TS: `export interface PaymentRequiredContext`
pub struct PaymentRequiredContext {
    pub url: String,
    pub method: String,
}

/// Context available after settlement completes.
///
/// Mirrors TS: `export interface SettleResultContext`
pub struct SettleResultContext {
    pub url: String,
    pub method: String,
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
    pub settle_response: SettleResponse,
}

/// Extension interface for facilitator-side extensions.
///
/// Mirrors TS: `export interface FacilitatorExtension`
#[async_trait]
pub trait FacilitatorExtension: Send + Sync {
    /// Unique key identifying this extension.
    fn key(&self) -> &str;

    /// Networks this extension supports.
    fn supported_networks(&self) -> Vec<Network>;
}

/// Extension interface for resource-server-side extensions.
///
/// Mirrors TS: `export interface ResourceServerExtension`
#[async_trait]
pub trait ResourceServerExtension: Send + Sync {
    /// Unique key identifying this extension.
    fn key(&self) -> &str;

    /// Enrich the PaymentRequired response before sending to the client.
    async fn enrich_payment_required(
        &self,
        payment_required: PaymentRequired,
        _context: &PaymentRequiredContext,
    ) -> PaymentRequired {
        payment_required
    }

    /// Enrich the verify request extensions before calling the facilitator.
    async fn enrich_verify_extensions(
        &self,
        extensions: HashMap<String, serde_json::Value>,
        _payment_payload: &PaymentPayload,
        _payment_requirements: &PaymentRequirements,
    ) -> HashMap<String, serde_json::Value> {
        extensions
    }

    /// Enrich the settle request extensions before calling the facilitator.
    async fn enrich_settle_extensions(
        &self,
        extensions: HashMap<String, serde_json::Value>,
        _payment_payload: &PaymentPayload,
        _payment_requirements: &PaymentRequirements,
    ) -> HashMap<String, serde_json::Value> {
        extensions
    }
}
