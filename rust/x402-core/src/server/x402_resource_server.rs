//! Core x402 Resource Server implementation.
//!
//! Mirrors: `@x402/core/src/server/x402ResourceServer.ts`
//!
//! This is the framework-agnostic server logic. It manages scheme registrations,
//! builds payment requirements, and orchestrates verify/settle calls.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::error::X402Error;
use crate::facilitator::FacilitatorClient;
use crate::types::{
    PaymentPayload, PaymentRequirements,
    ResourceInfo, SchemeNetworkServer, SettleRequest, SettleResponse,
    SupportedResponse, VerifyRequest, VerifyResponse,
};

/// Core x402 Resource Server.
///
/// Manages scheme registrations and orchestrates payment verification/settlement.
/// Framework-agnostic — does not depend on Axum, Express, or any HTTP framework.
///
/// Mirrors TS: `x402ResourceServer` from `core/src/server/x402ResourceServer.ts`
pub struct X402ResourceServer {
    facilitator: Arc<dyn FacilitatorClient>,
    /// network → scheme → SchemeNetworkServer
    schemes: HashMap<String, HashMap<String, Arc<dyn SchemeNetworkServer>>>,
    supported: Option<SupportedResponse>,
}

impl X402ResourceServer {
    /// Create a new resource server with the given facilitator client.
    ///
    /// Mirrors TS: `new x402ResourceServer(facilitatorClient)`
    pub fn new(facilitator: impl FacilitatorClient + 'static) -> Self {
        Self {
            facilitator: Arc::new(facilitator),
            schemes: HashMap::new(),
            supported: None,
        }
    }

    /// Register a scheme implementation for a specific network.
    ///
    /// Mirrors TS: `server.register(network, scheme)`
    pub fn register(
        mut self,
        network: &str,
        scheme: impl SchemeNetworkServer + 'static,
    ) -> Self {
        let scheme_name = scheme.scheme().to_string();
        self.schemes
            .entry(network.to_string())
            .or_insert_with(HashMap::new)
            .insert(scheme_name, Arc::new(scheme));
        self
    }

    /// Initialize by fetching supported kinds from the facilitator.
    ///
    /// Mirrors TS: `server.initialize()`
    pub async fn initialize(&mut self) -> Result<(), X402Error> {
        let supported = self.facilitator.get_supported().await?;
        self.supported = Some(supported);
        Ok(())
    }

    /// Get the cached supported response (must call initialize() first).
    pub fn supported(&self) -> Option<&SupportedResponse> {
        self.supported.as_ref()
    }

    /// Get a reference to the facilitator client.
    pub fn facilitator(&self) -> &dyn FacilitatorClient {
        self.facilitator.as_ref()
    }

    /// Find a registered scheme by network and scheme name.
    fn find_scheme(&self, network: &str, scheme: &str) -> Option<Arc<dyn SchemeNetworkServer>> {
        // Direct match
        if let Some(schemes) = self.schemes.get(network) {
            if let Some(s) = schemes.get(scheme) {
                return Some(Arc::clone(s));
            }
        }

        // Pattern match (e.g., "eip155:*" matches "eip155:196")
        for (pattern, schemes) in &self.schemes {
            if crate::utils::network_matches_pattern(network, pattern) {
                if let Some(s) = schemes.get(scheme) {
                    return Some(Arc::clone(s));
                }
            }
        }

        None
    }

    /// Build payment requirements for a given route configuration.
    ///
    /// **Requires `initialize()` to be called first.** If the server has not been
    /// initialized, this method returns `X402Error::NotInitialized`. If the
    /// facilitator does not support the requested scheme+network combination,
    /// returns `X402Error::UnsupportedScheme`.
    ///
    /// Mirrors TS: `server.buildPaymentRequirements()` (lines 481-555)
    pub async fn build_payment_requirements(
        &self,
        scheme: &str,
        network: &str,
        price: &str,
        pay_to: &str,
        max_timeout_seconds: u64,
        _resource: &ResourceInfo,
        config_extra: Option<&std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<PaymentRequirements, X402Error> {
        let scheme_impl = self
            .find_scheme(network, scheme)
            .ok_or_else(|| X402Error::UnsupportedScheme(format!("{}:{}", scheme, network)))?;

        // Parse the price using the scheme's parser
        let price_obj = crate::types::Price::Money(price.to_string());
        let asset_amount = scheme_impl.parse_price(&price_obj, &network.to_string()).await?;

        // Merge extra: start with parsed price extras, then overlay config extras
        // Mirrors TS: { ...parsedPrice.extra, ...resourceConfig.extra }
        let mut extra = asset_amount.extra.unwrap_or_default();
        if let Some(cfg_extra) = config_extra {
            for (k, v) in cfg_extra {
                extra.insert(k.clone(), v.clone());
            }
        }

        let mut requirements = PaymentRequirements {
            scheme: scheme.to_string(),
            network: network.to_string(),
            asset: asset_amount.asset,
            amount: asset_amount.amount,
            pay_to: pay_to.to_string(),
            max_timeout_seconds,
            extra,
        };

        // MUST have supported data (align with TS: throws if not initialized)
        let supported = self.supported.as_ref().ok_or_else(|| {
            X402Error::NotInitialized(
                "call initialize() before building payment requirements".into(),
            )
        })?;

        // Find the matching supported kind from facilitator
        let extensions: Vec<String> = supported.extensions.clone();
        let kind = supported
            .kinds
            .iter()
            .find(|k| k.scheme == scheme && k.network == network)
            .ok_or_else(|| {
                X402Error::UnsupportedScheme(format!(
                    "Facilitator does not support {} on {}. Make sure to call initialize().",
                    scheme, network
                ))
            })?;

        // Delegate to the scheme for scheme-specific enhancements
        requirements = scheme_impl
            .enhance_payment_requirements(requirements, kind, &extensions)
            .await?;

        Ok(requirements)
    }

    /// Validate route configurations against registered schemes and facilitator support.
    ///
    /// Mirrors TS: `validateRouteConfiguration()` from `x402HTTPResourceServer.ts`
    ///
    /// Should be called after `initialize()` to ensure all routes are valid
    /// before the server starts accepting requests.
    pub fn validate_routes(
        &self,
        routes: &crate::http::RoutesConfig,
    ) -> Result<(), X402Error> {
        let mut errors = Vec::new();

        for (path, config) in routes {
            for accept in &config.accepts {
                // Check scheme is registered
                if self.find_scheme(&accept.network, &accept.scheme).is_none() {
                    errors.push(format!(
                        "Route '{}': no scheme implementation registered for {} on {}",
                        path, accept.scheme, accept.network
                    ));
                }
                // Check facilitator supports this scheme+network
                if let Some(supported) = &self.supported {
                    if !supported
                        .kinds
                        .iter()
                        .any(|k| k.scheme == accept.scheme && k.network == accept.network)
                    {
                        errors.push(format!(
                            "Route '{}': facilitator does not support {} on {}",
                            path, accept.scheme, accept.network
                        ));
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(X402Error::RouteConfig(errors.join("; ")))
        }
    }

    /// Verify a payment payload against the given requirements.
    ///
    /// Mirrors TS: `server.verifyPayment()`
    pub async fn verify_payment(
        &self,
        payment_payload: &PaymentPayload,
        payment_requirements: &PaymentRequirements,
    ) -> Result<VerifyResponse, X402Error> {
        let request = VerifyRequest {
            x402_version: payment_payload.x402_version,
            payment_payload: payment_payload.clone(),
            payment_requirements: payment_requirements.clone(),
        };
        self.facilitator.verify(&request).await
    }

    /// Settle a verified payment.
    ///
    /// Mirrors TS: `server.settlePayment()`
    pub async fn settle_payment(
        &self,
        payment_payload: &PaymentPayload,
        payment_requirements: &PaymentRequirements,
        sync_settle: Option<bool>,
    ) -> Result<SettleResponse, X402Error> {
        let request = SettleRequest {
            x402_version: payment_payload.x402_version,
            payment_payload: payment_payload.clone(),
            payment_requirements: payment_requirements.clone(),
            sync_settle,
        };
        self.facilitator.settle(&request).await
    }

    /// Poll `GET /settle/status` until a terminal state is reached or deadline expires.
    ///
    /// Used when settle returns `status="timeout"` (exact + syncSettle=true).
    /// Each Buyer request polls independently with its own deadline.
    ///
    /// # Arguments
    /// - `tx_hash` - Transaction hash to query
    /// - `poll_interval` - Time between polls (default 1s)
    /// - `poll_deadline` - Max time to poll (default 5s)
    pub async fn poll_settle_status(
        &self,
        tx_hash: &str,
        poll_interval: Duration,
        poll_deadline: Duration,
    ) -> crate::http::PollResult {
        use crate::http::PollResult;

        tracing::info!(
            "[x402] polling /settle/status for tx={} (interval={:?}, deadline={:?})",
            tx_hash, poll_interval, poll_deadline
        );

        let result = tokio::time::timeout(poll_deadline, async {
            loop {
                match self.facilitator.get_settle_status(tx_hash).await {
                    Ok(resp) => {
                        if !resp.success {
                            tracing::info!("[x402] poll: tx={} → failed", tx_hash);
                            return PollResult::Failed;
                        }
                        match resp.status.as_deref() {
                            Some("success") => {
                                tracing::info!("[x402] poll: tx={} → success", tx_hash);
                                return PollResult::Success;
                            }
                            Some("pending") => {
                                tracing::debug!("[x402] poll: tx={} → pending, retrying...", tx_hash);
                            }
                            Some(other) => {
                                tracing::warn!("[x402] poll: tx={} → unknown status '{}', treating as pending", tx_hash, other);
                            }
                            None => {
                                tracing::debug!("[x402] poll: tx={} → no status, retrying...", tx_hash);
                            }
                        }
                    }
                    Err(e) => {
                        // API error — continue polling if still within deadline
                        tracing::warn!("[x402] poll: tx={} → API error: {}, retrying...", tx_hash, e);
                    }
                }
                tokio::time::sleep(poll_interval).await;
            }
        })
        .await;

        match result {
            Ok(poll_result) => poll_result,
            Err(_) => {
                tracing::info!("[x402] poll: tx={} → deadline expired", tx_hash);
                PollResult::Timeout
            }
        }
    }
}
