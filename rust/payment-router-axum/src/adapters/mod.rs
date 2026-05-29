//! Built-in adapters. Users may implement `ProtocolAdapter` on their own
//! types for custom protocols (spec §9: user-defined adapters start at
//! priority 100).

mod mpp;
mod x402;

pub use mpp::{MppAdapter, MppRouteConfig};
pub use x402::{build_routes_config as build_x402_routes_config, X402Adapter, X402RouteConfig};
