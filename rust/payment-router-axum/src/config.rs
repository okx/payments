//! Type-erased per-adapter route config (Rust equivalent of Go's `any` /
//! TS's `unknown`).
//!
//! Each [`crate::adapter::ProtocolAdapter`] declares its own concrete
//! per-route config struct (e.g. `MppRouteConfig`, `X402RouteConfig`).
//! Users pass instances of those structs into the
//! [`UnifiedRouteConfig::builder`]; storage is type-erased via
//! `Arc<dyn Any>`. Adapter code recovers the concrete type via
//! [`AdapterConfig::downcast_ref`] — the same pattern Go uses with
//! `cfg.(MppRouteConfig)`.
//!
//! No JSON (de)serialization, no schema duplication, no runtime parsing
//! errors for typos.
//!
//! ```ignore
//! use payment_router_axum::{UnifiedRouteConfig, adapters::MppRouteConfig};
//!
//! let route = UnifiedRouteConfig::builder()
//!     .description("Photo download")
//!     .adapter("mpp", MppRouteConfig {
//!         intent: "charge".into(),
//!         amount: "100".into(),
//!         description: Some("One photo".into()),
//!     })
//!     .build();
//! ```
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

/// A single adapter's per-route configuration, type-erased.
///
/// Cloning is cheap — backed by `Arc`. Adapters recover the concrete
/// type via [`AdapterConfig::downcast_ref`].
#[derive(Clone)]
pub struct AdapterConfig(Arc<dyn Any + Send + Sync>);

impl AdapterConfig {
    /// Wrap a concrete config value.
    pub fn new<T: Any + Send + Sync + 'static>(value: T) -> Self {
        Self(Arc::new(value))
    }

    /// Recover the concrete type. Returns `None` when the adapter was
    /// given a config of the wrong type — a programmer error that should
    /// produce a clear adapter-side error (not a silent skip).
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}

impl std::fmt::Debug for AdapterConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn Any` doesn't expose the concrete type's `Debug`. Best we
        // can do is signal the existence and the wrapping. Adapter docs
        // are expected to name the concrete type they require.
        f.debug_struct("AdapterConfig").finish_non_exhaustive()
    }
}

/// Fluent builder for [`crate::types::UnifiedRouteConfig`].
pub struct UnifiedRouteConfigBuilder {
    description: Option<String>,
    adapter_configs: HashMap<String, AdapterConfig>,
}

impl UnifiedRouteConfigBuilder {
    pub(crate) fn new() -> Self {
        Self {
            description: None,
            adapter_configs: HashMap::new(),
        }
    }

    /// Human-readable description (echoed into challenge headers /
    /// 402 bodies by adapters that support it).
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Attach a typed per-route config for a specific adapter. `name`
    /// must match [`crate::adapter::ProtocolAdapter::name`] on the
    /// adapter that should consume this config.
    pub fn adapter<T: Any + Send + Sync + 'static>(
        mut self,
        name: impl Into<String>,
        config: T,
    ) -> Self {
        self.adapter_configs
            .insert(name.into(), AdapterConfig::new(config));
        self
    }

    pub fn build(self) -> crate::types::UnifiedRouteConfig {
        crate::types::UnifiedRouteConfig {
            description: self.description,
            adapter_configs: self.adapter_configs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct MyCfg {
        amount: String,
    }

    #[test]
    fn downcast_recovers_concrete_type() {
        let c = AdapterConfig::new(MyCfg {
            amount: "100".into(),
        });
        let recovered = c.downcast_ref::<MyCfg>().expect("downcast");
        assert_eq!(recovered.amount, "100");
    }

    #[test]
    fn downcast_wrong_type_returns_none() {
        let c = AdapterConfig::new(MyCfg {
            amount: "100".into(),
        });
        assert!(c.downcast_ref::<String>().is_none());
    }

    #[test]
    fn clone_is_cheap_and_shares_state() {
        let c = AdapterConfig::new(MyCfg {
            amount: "100".into(),
        });
        let c2 = c.clone();
        // Same Arc → same pointer behind the trait object.
        let a = c.downcast_ref::<MyCfg>().unwrap() as *const MyCfg;
        let b = c2.downcast_ref::<MyCfg>().unwrap() as *const MyCfg;
        assert_eq!(a, b);
    }

    #[test]
    fn builder_produces_unified_route_config() {
        let route = crate::types::UnifiedRouteConfig::builder()
            .description("Test")
            .adapter(
                "mpp",
                MyCfg {
                    amount: "100".into(),
                },
            )
            .build();
        assert_eq!(route.description.as_deref(), Some("Test"));
        let cfg = route.adapter_configs.get("mpp").expect("mpp config");
        assert_eq!(cfg.downcast_ref::<MyCfg>().unwrap().amount, "100");
    }
}
