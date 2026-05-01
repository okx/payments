//! Shared types for the payment router.
//!
//! Mirrors spec §3 / §6.2 (UnifiedRouteConfig, PaymentRouterConfig, ErrorContext).

use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapter::ProtocolAdapter;

/// Phase in which an error occurred (passed to the on_error callback).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorPhase {
    /// During adapter.detect() — spec §3 #2: should not throw, any error is treated as miss.
    Detect,
    /// During adapter.get_challenge() — a single adapter failing must not block 402 merging.
    Challenge,
    /// During adapter-wrapped service call (verify/handle).
    Handle,
}

impl ErrorPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorPhase::Detect => "detect",
            ErrorPhase::Challenge => "challenge",
            ErrorPhase::Handle => "handle",
        }
    }
}

/// Context passed to the on_error callback.
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Phase the error was observed in.
    pub phase: ErrorPhase,
    /// Adapter.name() that raised the error.
    pub protocol: String,
    /// Matched route key ("METHOD /path"), if any.
    pub route: Option<String>,
}

/// User-supplied error handler.
///
/// Called when any adapter stage errors out. Returning does not change behavior;
/// it is for observability only. Intentionally non-async to encourage logging
/// rather than blocking work.
pub type ErrorHandler =
    dyn Fn(&(dyn std::error::Error + Send + Sync), ErrorContext) + Send + Sync + 'static;

/// Per-route config. `adapter_configs` maps adapter.name() → adapter-specific JSON.
///
/// Mirrors spec §3 / §6.2. JSON value is late-bound: each adapter deserializes
/// its own slice. This avoids forcing a shared concrete type across protocols.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedRouteConfig {
    /// Optional human-readable description (ignored by startup validation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Map from adapter.name() → config JSON. Missing keys = adapter skipped for this route.
    #[serde(default)]
    pub adapter_configs: HashMap<String, Value>,
}

/// Main config passed to `PaymentRouterLayer::new`.
///
/// Routes are a Vec (not HashMap) to preserve declaration order per spec §9
/// (first-match-wins).
pub struct PaymentRouterConfig {
    /// `Vec<(pattern, route_cfg)>` in declaration order. Pattern format: "METHOD /path"
    /// or just "/path" (any method). See `router::CompiledRouter` for normalization.
    pub routes: Vec<(String, UnifiedRouteConfig)>,
    /// Protocol adapter instances (MPP / x402 / user-custom). Must be eager-initialized
    /// (SDK clients already ready) per spec §3 hard convention.
    pub protocols: Vec<Arc<dyn ProtocolAdapter>>,
    /// Optional error observer — invoked for detect/challenge/handle failures.
    pub on_error: Option<Arc<ErrorHandler>>,
}
