//! Core x402 Resource Server implementation.
//!
//! This is the framework-agnostic server logic. It manages scheme registrations,
//! builds payment requirements, and orchestrates verify/settle calls.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use crate::error::X402Error;
use crate::facilitator::FacilitatorClient;
use crate::types::{
    PaymentPayload, PaymentRequirements, ResourceInfo, SchemeNetworkServer, SettleRequest,
    SettleResponse, SupportedResponse, VerifyRequest, VerifyResponse,
};

/// Core x402 Resource Server.
///
/// Manages scheme registrations and orchestrates payment verification/settlement.
/// Framework-agnostic — does not depend on Axum, Express, or any HTTP framework.
pub struct X402ResourceServer {
    facilitator: Arc<dyn FacilitatorClient>,
    /// network → scheme → SchemeNetworkServer
    schemes: HashMap<String, HashMap<String, Arc<dyn SchemeNetworkServer>>>,
    supported: Option<SupportedResponse>,
    /// Review test-wallet addresses (lowercased) exempt from verify+settle.
    exempt_payers: HashSet<String>,
}

impl X402ResourceServer {
    /// Create a new resource server with the given facilitator client.
    pub fn new(facilitator: impl FacilitatorClient + 'static) -> Self {
        Self {
            facilitator: Arc::new(facilitator),
            schemes: HashMap::new(),
            supported: None,
            exempt_payers: HashSet::new(),
        }
    }

    /// Register a scheme implementation for a specific network.
    pub fn register(mut self, network: &str, scheme: impl SchemeNetworkServer + 'static) -> Self {
        let scheme_name = scheme.scheme().to_string();
        self.schemes
            .entry(network.to_string())
            .or_insert_with(HashMap::new)
            .insert(scheme_name, Arc::new(scheme));
        self
    }

    /// Register review test-wallet addresses exempt from verify+settle (the
    /// review-wallet no-charge flow). A request whose payload `from` is one of
    /// these addresses, once the facilitator confirms the signature is valid,
    /// is served with HTTP 200 while verify AND settle are skipped (no on-chain
    /// charge). Case-insensitive; additive across calls; empty = feature off.
    /// The address list is the seller's to control — never derive it from
    /// caller-supplied input.
    pub fn exempt_payers<I, S>(mut self, payers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.exempt_payers.extend(
            payers
                .into_iter()
                .map(|p| p.into().trim().to_lowercase())
                .filter(|p| !p.is_empty()),
        );
        self
    }

    /// Try the settlement-exemption bypass for a payment. First gates on the
    /// payload's declared `from` being on the exempt allowlist (so the endpoint
    /// runs only for exempt candidates), then confirms the signature via the
    /// facilitator's signature-only verify. Returns `true` only when the payer
    /// is exempt AND the signature is valid — the caller then serves the
    /// resource and skips both verify and settle (no on-chain charge). Returns
    /// `false` in every other case (no allowlist, non-exempt payer, unsupported
    /// facilitator, invalid signature, or error) so the caller runs the normal
    /// flow. The declared `from` only gates the lookup; trust comes from the
    /// signature check.
    #[doc(hidden)]
    pub async fn try_exempt_signature_bypass(
        &self,
        payment_payload: &PaymentPayload,
        payment_requirements: &PaymentRequirements,
    ) -> bool {
        if self.exempt_payers.is_empty() {
            return false;
        }
        // Declared payer: exact+EIP-3009 / aggr_deferred use `authorization.from`;
        // exact+Permit2 / upto use `permit2Authorization.from`.
        let payer = Self::claimed_from(payment_payload);
        if !self.is_exempt(payer.as_deref()) {
            return false;
        }
        let request = VerifyRequest {
            x402_version: payment_payload.x402_version,
            payment_payload: payment_payload.clone(),
            payment_requirements: payment_requirements.clone(),
        };
        match self.facilitator.verify_signature_only(&request).await {
            Ok(resp) => resp.is_valid,
            Err(e) => {
                tracing::warn!("[x402] exempt verify-signature failed; normal flow: {e}");
                false
            }
        }
    }

    /// True when `addr` is a configured exempt payer. Case-insensitive.
    fn is_exempt(&self, addr: Option<&str>) -> bool {
        match addr {
            Some(a) => self.exempt_payers.contains(&a.to_lowercase()),
            None => false,
        }
    }

    /// The buyer-claimed payer address in a payment payload, if present:
    /// `authorization.from` (EIP-3009 / aggr_deferred) or
    /// `permit2Authorization.from` (Permit2 / upto). `None` for other shapes.
    fn claimed_from(payment_payload: &PaymentPayload) -> Option<String> {
        let payload = &payment_payload.payload;
        for key in ["authorization", "permit2Authorization"] {
            if let Some(from) = payload
                .get(key)
                .and_then(|a| a.get("from"))
                .and_then(|f| f.as_str())
            {
                return Some(from.to_string());
            }
        }
        None
    }

    /// Initialize by fetching supported kinds from the facilitator.
    pub async fn initialize(&mut self) -> Result<(), X402Error> {
        let supported = self.facilitator.get_supported().await?;
        let kinds: Vec<String> = supported
            .kinds
            .iter()
            .map(|k| format!("{}@{}", k.scheme, k.network))
            .collect();
        tracing::info!("[x402] /supported kinds: [{}]", kinds.join(", "));
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
        // Try JSON AssetAmount first (e.g. {"asset":"0x...","amount":"1000"}), fall back to Money string
        let price_obj = serde_json::from_str::<crate::types::AssetAmount>(price)
            .map(crate::types::Price::Asset)
            .unwrap_or_else(|_| crate::types::Price::Money(price.to_string()));
        let asset_amount = scheme_impl
            .parse_price(&price_obj, &network.to_string())
            .await?;

        // Merge extra: start with parsed price extras, then overlay config extras
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

        // MUST have supported data — error if `initialize()` wasn't called.
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
    /// Should be called after `initialize()` to ensure all routes are valid
    /// before the server starts accepting requests.
    pub fn validate_routes(&self, routes: &crate::http::RoutesConfig) -> Result<(), X402Error> {
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
    /// If `settlement_overrides` is provided with an amount, the effective
    /// `payment_requirements.amount` is replaced before settlement (partial
    /// settlement for upto scheme).
    pub async fn settle_payment(
        &self,
        payment_payload: &PaymentPayload,
        payment_requirements: &PaymentRequirements,
        sync_settle: Option<bool>,
        settlement_overrides: Option<&crate::http::SettlementOverrides>,
    ) -> Result<SettleResponse, X402Error> {
        // Apply settlement overrides (e.g., partial settlement for upto scheme)
        let effective_requirements = if let Some(overrides) = settlement_overrides {
            if let Some(ref raw_amount) = overrides.amount {
                let resolved = crate::http::resolve_settlement_override_amount(
                    raw_amount,
                    payment_requirements,
                )?;
                let mut reqs = payment_requirements.clone();
                reqs.amount = resolved;
                reqs
            } else {
                payment_requirements.clone()
            }
        } else {
            payment_requirements.clone()
        };

        let request = SettleRequest {
            x402_version: payment_payload.x402_version,
            payment_payload: payment_payload.clone(),
            payment_requirements: effective_requirements,
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
            tx_hash,
            poll_interval,
            poll_deadline
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
                                tracing::debug!(
                                    "[x402] poll: tx={} → pending, retrying...",
                                    tx_hash
                                );
                            }
                            Some(other) => {
                                tracing::warn!(
                                    "[x402] poll: tx={} → unknown status '{}', treating as pending",
                                    tx_hash,
                                    other
                                );
                            }
                            None => {
                                tracing::debug!(
                                    "[x402] poll: tx={} → no status, retrying...",
                                    tx_hash
                                );
                            }
                        }
                    }
                    Err(e) => {
                        // API error — continue polling if still within deadline
                        tracing::warn!(
                            "[x402] poll: tx={} → API error: {}, retrying...",
                            tx_hash,
                            e
                        );
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

#[cfg(test)]
mod exempt_tests {
    use super::*;
    use crate::types::SettleStatusResponse;
    use async_trait::async_trait;

    // Facilitator whose signature-only verify returns a fixed `is_valid`.
    // `sig_valid = None` leaves verify_signature_only unsupported (trait default).
    struct StubFacilitator {
        sig_valid: Option<bool>,
    }

    #[async_trait]
    impl FacilitatorClient for StubFacilitator {
        async fn get_supported(&self) -> Result<SupportedResponse, X402Error> {
            Err(X402Error::Other("stub".into()))
        }
        async fn verify(&self, _: &VerifyRequest) -> Result<VerifyResponse, X402Error> {
            Err(X402Error::Other("stub".into()))
        }
        async fn verify_signature_only(
            &self,
            _: &VerifyRequest,
        ) -> Result<VerifyResponse, X402Error> {
            match self.sig_valid {
                Some(is_valid) => Ok(VerifyResponse {
                    is_valid,
                    invalid_reason: None,
                    invalid_message: None,
                    payer: Some("0xrev".into()),
                    extensions: None,
                }),
                None => Err(X402Error::Other("unsupported".into())),
            }
        }
        async fn settle(&self, _: &SettleRequest) -> Result<SettleResponse, X402Error> {
            Err(X402Error::Other("stub".into()))
        }
        async fn get_settle_status(&self, _: &str) -> Result<SettleStatusResponse, X402Error> {
            Err(X402Error::Other("stub".into()))
        }
    }

    fn server(sig_valid: Option<bool>, exempt: &[&str]) -> X402ResourceServer {
        X402ResourceServer::new(StubFacilitator { sig_valid }).exempt_payers(exempt.to_vec())
    }

    fn payload(inner: serde_json::Value) -> PaymentPayload {
        serde_json::from_value(serde_json::json!({
            "x402Version": 2,
            "accepted": {
                "scheme": "exact", "network": "eip155:196", "asset": "0xa",
                "amount": "1", "payTo": "0xr", "maxTimeoutSeconds": 60
            },
            "payload": inner
        }))
        .expect("valid PaymentPayload")
    }

    fn reqs() -> PaymentRequirements {
        serde_json::from_value(serde_json::json!({
            "scheme": "exact", "network": "eip155:196", "asset": "0xa",
            "amount": "1", "payTo": "0xr", "maxTimeoutSeconds": 60
        }))
        .expect("valid PaymentRequirements")
    }

    #[test]
    fn exempt_payers_accumulate_and_normalize() {
        let s = server(None, &[])
            .exempt_payers(vec!["0xAbC", "  0xDEF  ", ""]) // mixed case, padded, blank
            .exempt_payers(vec!["0xGhi"]); // additive across calls
        assert!(s.is_exempt(Some("0xabc"))); // case-insensitive
        assert!(s.is_exempt(Some("0XABC")));
        assert!(s.is_exempt(Some("0xdef"))); // trimmed
        assert!(s.is_exempt(Some("0xghi"))); // accumulated
        assert!(!s.is_exempt(Some("0xzzz")));
        assert!(!s.is_exempt(None));
    }

    #[test]
    fn claimed_from_reads_both_shapes() {
        // EIP-3009 / aggr_deferred
        let a = payload(serde_json::json!({ "authorization": { "from": "0xEoa" } }));
        assert_eq!(X402ResourceServer::claimed_from(&a).as_deref(), Some("0xEoa"));
        // Permit2 / upto: authorization null, from under permit2Authorization
        let p = payload(serde_json::json!({
            "authorization": serde_json::Value::Null,
            "permit2Authorization": { "from": "0xP2" }
        }));
        assert_eq!(X402ResourceServer::claimed_from(&p).as_deref(), Some("0xP2"));
        // Neither
        let n = payload(serde_json::json!({ "signature": "0xsig" }));
        assert_eq!(X402ResourceServer::claimed_from(&n), None);
    }

    #[tokio::test]
    async fn bypass_true_when_exempt_and_signature_valid() {
        let s = server(Some(true), &["0xRev"]);
        let p = payload(serde_json::json!({ "authorization": { "from": "0xrev" } }));
        assert!(s.try_exempt_signature_bypass(&p, &reqs()).await);
    }

    #[tokio::test]
    async fn bypass_false_when_signature_invalid() {
        let s = server(Some(false), &["0xrev"]);
        let p = payload(serde_json::json!({ "authorization": { "from": "0xrev" } }));
        assert!(!s.try_exempt_signature_bypass(&p, &reqs()).await);
    }

    #[tokio::test]
    async fn bypass_false_when_payer_not_exempt() {
        // Non-exempt `from`: endpoint must not even be consulted (would be valid).
        let s = server(Some(true), &["0xrev"]);
        let p = payload(serde_json::json!({ "authorization": { "from": "0xother" } }));
        assert!(!s.try_exempt_signature_bypass(&p, &reqs()).await);
    }

    #[tokio::test]
    async fn bypass_false_when_no_exempt_or_unsupported() {
        // Empty allowlist -> false.
        let empty = server(Some(true), &[]);
        let p = payload(serde_json::json!({ "authorization": { "from": "0xrev" } }));
        assert!(!empty.try_exempt_signature_bypass(&p, &reqs()).await);
        // Facilitator without verify_signature_only -> false.
        let unsupported = server(None, &["0xrev"]);
        assert!(!unsupported.try_exempt_signature_bypass(&p, &reqs()).await);
    }
}
