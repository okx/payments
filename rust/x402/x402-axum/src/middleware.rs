//! Axum Tower middleware for x402 payment protocol.
//!
//! Implements the full payment flow:
//! 1. Check if route requires payment (via route key matching)
//! 2. If no payment header → return 402 with PAYMENT-REQUIRED header
//! 3. If payment header present → decode, verify via facilitator
//! 4. If verified → pass through to handler, buffer response, settle
//! 5. Add PAYMENT-RESPONSE header with settlement result

use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use tower::{Layer, Service};

use x402_core::http::{
    decode_payment_signature_header, encode_payment_required_header,
    encode_payment_response_header, OnAfterSettleHook, OnAfterVerifyHook, OnBeforeSettleHook,
    OnBeforeVerifyHook, OnProtectedRequestHook, OnSettleFailureHook, OnSettlementTimeoutHook,
    OnVerifyFailureHook, PaymentResolverFn, PollResult, ResolvedAccept, RoutePaymentConfig,
    RoutesConfig, SettleContext, SettleResultContext, SettlementOverrides, SubscriptionOperation,
    VerifyContext, VerifyResultContext, DEFAULT_POLL_DEADLINE, DEFAULT_POLL_INTERVAL,
    PAYMENT_REQUIRED_HEADER, PAYMENT_RESPONSE_HEADER, SETTLEMENT_OVERRIDES_HEADER,
};
use x402_core::server::X402ResourceServer;
use x402_core::subscription::{CancelAuth, PendingChangeCancelAuth};
use x402_core::types::{PaymentRequired, ResourceInfo};

use crate::adapter;

/// Pre-matched route config injected via request extensions by an outer
/// router (e.g. `payment-router-axum`). When present, [`PaymentMiddleware`]
/// uses this config directly and **skips its own path matching** —
/// closing the bypass where the outer router matches by pattern /
/// normalization but x402's internal exact-string match misses.
///
/// Standalone `payment_middleware(...)` users never set this; the
/// middleware falls back to looking up `state.routes` by request path.
#[derive(Debug, Clone)]
pub struct PreMatchedRoute(pub RoutePaymentConfig);

/// Shared state between the layer and middleware instances.
struct PaymentState {
    server: X402ResourceServer,
    routes: RoutesConfig,
    timeout_hook: Option<OnSettlementTimeoutHook>,
    poll_deadline: Duration,
    resolver: Option<PaymentResolverFn>,
    // Lifecycle hooks
    on_protected_request: Option<OnProtectedRequestHook>,
    on_before_verify: Option<OnBeforeVerifyHook>,
    on_after_verify: Option<OnAfterVerifyHook>,
    on_verify_failure: Option<OnVerifyFailureHook>,
    on_before_settle: Option<OnBeforeSettleHook>,
    on_after_settle: Option<OnAfterSettleHook>,
    on_settle_failure: Option<OnSettleFailureHook>,
    subscription: Option<crate::subscription::SubscriptionSupport>,
}

/// Tower Layer that wraps services with x402 payment checking.
#[derive(Clone)]
pub struct PaymentLayer {
    state: Arc<PaymentState>,
}

impl PaymentLayer {
    /// Create a new PaymentLayer.
    fn new(
        server: X402ResourceServer,
        routes: RoutesConfig,
        timeout_hook: Option<OnSettlementTimeoutHook>,
        poll_deadline: Duration,
        resolver: Option<PaymentResolverFn>,
    ) -> Self {
        Self {
            state: Arc::new(PaymentState {
                server,
                routes,
                timeout_hook,
                poll_deadline,
                resolver,
                on_protected_request: None,
                on_before_verify: None,
                on_after_verify: None,
                on_verify_failure: None,
                on_before_settle: None,
                on_after_settle: None,
                on_settle_failure: None,
                subscription: None,
            }),
        }
    }

    /// Create a new PaymentLayer from a builder state.
    fn from_state(state: PaymentState) -> Self {
        Self {
            state: Arc::new(state),
        }
    }
}

impl<S> Layer<S> for PaymentLayer {
    type Service = PaymentMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PaymentMiddleware {
            inner,
            state: Arc::clone(&self.state),
        }
    }
}

/// Tower Service that performs x402 payment verification and settlement.
#[derive(Clone)]
pub struct PaymentMiddleware<S> {
    inner: S,
    state: Arc<PaymentState>,
}

impl<S> Service<Request<Body>> for PaymentMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let state = Arc::clone(&self.state);
        let mut inner = self.inner.clone();

        // Extract all needed info from the request before entering the async block
        // to satisfy Send bounds (Request<Body> is not Sync).
        let method = adapter::extract_method(&req);
        let path = adapter::extract_path(&req);
        let payment_header = adapter::extract_payment_header(&req);
        // Full URL `scheme://host/path?query` (fallback to path+query when
        // host headers are missing). Used as the auto-extraction half of the
        // dual-strategy `ResourceInfo.url`; `route_config.resource` (when
        // set) overrides this inside `build_402_response`.
        let uri_string = adapter::extract_url(&req);
        // Only collect headers when a resolver is configured (avoids per-request allocation)
        let headers_map: Option<std::collections::HashMap<String, String>> = if state
            .resolver
            .is_some()
        {
            Some(
                req.headers()
                    .iter()
                    .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect(),
            )
        } else {
            None
        };

        Box::pin(async move {
            // 1. Determine the route config.
            //
            // Outer router (e.g. `payment-router-axum`) may have already
            // matched the route and pinned the resolved `RoutePaymentConfig`
            // into `req.extensions()` via `PreMatchedRoute`. When present we
            // use it verbatim and SKIP our own path matching — this avoids
            // the double-matching bypass where the outer router accepts a
            // request (via pattern / normalization) that this middleware's
            // exact-string lookup would miss.
            //
            // Standalone callers (no outer router) don't set the extension;
            // we fall back to `find_route_config` against `state.routes`.
            let route_config: RoutePaymentConfig = if let Some(pre) =
                req.extensions().get::<PreMatchedRoute>()
            {
                pre.0.clone()
            } else {
                match find_route_config(&state.routes, &method, &path) {
                    Some(config) => config.clone(),
                    None => {
                        // No payment required for this route, pass through
                        return inner.call(req).await;
                    }
                }
            };

            // 1b. Subscription cancel / cancel-pending-change operation routes:
            // the buyer POSTs a signed `CancelAuth` / `PendingChangeCancelAuth`
            // (JSON body); relay it to the facilitator. No 402 / payment flow.
            if let Some(support) = &state.subscription {
                match route_config.operation {
                    Some(SubscriptionOperation::Cancel) => {
                        let bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024).await {
                            Ok(b) => b,
                            Err(_) => {
                                return Ok(error_response(
                                    StatusCode::BAD_REQUEST,
                                    "failed to read cancel body",
                                ))
                            }
                        };
                        let auth: CancelAuth = match serde_json::from_slice(&bytes) {
                            Ok(a) => a,
                            Err(e) => {
                                return Ok(error_response(
                                    StatusCode::BAD_REQUEST,
                                    &format!("invalid CancelAuth: {e}"),
                                ))
                            }
                        };
                        if let Err(e) = support.verify_cancel(&auth) {
                            return Ok(error_response(StatusCode::BAD_REQUEST, &e));
                        }
                        return Ok(match support.settle_cancel(auth).await {
                            Ok(r) => json_ok(serde_json::json!({
                                "subId": r.sub_id, "txHash": r.tx_hash, "state": r.state
                            })),
                            Err(e) => error_response(StatusCode::BAD_GATEWAY, &e),
                        });
                    }
                    Some(SubscriptionOperation::CancelPendingChange) => {
                        let bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024).await {
                            Ok(b) => b,
                            Err(_) => {
                                return Ok(error_response(
                                    StatusCode::BAD_REQUEST,
                                    "failed to read cancel-pending body",
                                ))
                            }
                        };
                        let auth: PendingChangeCancelAuth = match serde_json::from_slice(&bytes) {
                            Ok(a) => a,
                            Err(e) => {
                                return Ok(error_response(
                                    StatusCode::BAD_REQUEST,
                                    &format!("invalid PendingChangeCancelAuth: {e}"),
                                ))
                            }
                        };
                        if let Err(e) = support.verify_cancel_pending(&auth) {
                            return Ok(error_response(StatusCode::BAD_REQUEST, &e));
                        }
                        return Ok(match support.settle_cancel_pending(auth).await {
                            Ok(r) => json_ok(serde_json::json!({
                                "subId": r.sub_id, "txHash": r.tx_hash, "state": r.state
                            })),
                            Err(e) => error_response(StatusCode::BAD_GATEWAY, &e),
                        });
                    }
                    _ => {}
                }
            }

            // 2. Hook: onProtectedRequest — can grant access or abort
            if let Some(hook) = &state.on_protected_request {
                let ctx = x402_core::http::RequestContext {
                    method: method.clone(),
                    path: path.clone(),
                    headers: headers_map.clone().unwrap_or_default(),
                };
                let hook_result = (hook)(ctx).await;
                if hook_result.grant_access {
                    // Bypass payment — grant free access
                    return inner.call(req).await;
                }
                if hook_result.abort {
                    let reason = hook_result
                        .reason
                        .unwrap_or_else(|| "access denied".to_string());
                    return Ok(error_response(StatusCode::FORBIDDEN, &reason));
                }
            }

            // 3a. Subscription subsequent-access: an `APP-Access` proof bypasses
            // the payment flow — verify it, confirm the sub is active, then serve.
            if let Some(support) = &state.subscription {
                if let Some(app_access) = adapter::extract_app_access_header(&req) {
                    // Change-plan route: an APP-Access proof means "show me the
                    // plans I can switch to". Return the candidate plans annotated
                    // with `extra.changeFrom` (current plan dropped).
                    if route_config.operation == Some(SubscriptionOperation::Change) {
                        let request_context = x402_core::http::RequestContext {
                            method: method.clone(),
                            path: path.clone(),
                            headers: headers_map.clone().unwrap_or_default(),
                        };
                        return match crate::subscription::verify_access(
                            support,
                            &app_access,
                            crate::subscription::now_unix(),
                        )
                        .await
                        {
                            Ok((sub_id, plan_id, plan_tier)) => {
                                let accepts = crate::subscription::build_change_accepts(
                                    &route_config,
                                    &sub_id,
                                    &plan_id,
                                    plan_tier,
                                );
                                if accepts.is_empty() {
                                    return Ok(error_response(
                                        StatusCode::PAYMENT_REQUIRED,
                                        "no change-plan options available for the current plan",
                                    ));
                                }
                                let mut rc = route_config.clone();
                                rc.accepts = accepts;
                                Ok(build_402_response(
                                    &state.server,
                                    &rc,
                                    &uri_string,
                                    &request_context,
                                    &state.resolver,
                                )
                                .await)
                            }
                            // Merchant policy veto → hard deny, no fallback offer.
                            Err(crate::subscription::AccessError::Denied(msg)) => {
                                Ok(error_response(StatusCode::PAYMENT_REQUIRED, &msg))
                            }
                            // No active sub / unverifiable proof → degrade to a
                            // normal subscribe offer (no changeFrom).
                            Err(_) => Ok(build_402_response(
                                &state.server,
                                &route_config,
                                &uri_string,
                                &request_context,
                                &state.resolver,
                            )
                            .await),
                        };
                    }

                    use crate::subscription::AccessError;
                    let access = crate::subscription::verify_access(
                        support,
                        &app_access,
                        crate::subscription::now_unix(),
                    )
                    .await;

                    // Decide the 402 reason warranting an offer. Plan-gating
                    // failures and `NotActive` get the offer (buyer can
                    // subscribe/upgrade); auth failures get a bare 402.
                    let offer_reason: Option<String> = match &access {
                        Ok((sub_id, sub_plan_id, _)) => {
                            // Fail-closed plan gating: planId must be known AND accepted.
                            let accepted =
                                crate::subscription::accepted_plan_ids(&route_config);
                            if !accepted.is_empty() && sub_plan_id.is_empty() {
                                tracing::warn!(
                                    "[x402-sub] access: subId={} planId unknown, route accepts {:?} → 402",
                                    sub_id, accepted
                                );
                                Some(
                                    "subscription plan could not be determined for this resource"
                                        .to_string(),
                                )
                            } else if !accepted.is_empty()
                                && !crate::subscription::plan_id_accepted(&accepted, sub_plan_id)
                            {
                                tracing::warn!(
                                    "[x402-sub] access: subId={} planId={} not in route plans {:?} → 402",
                                    sub_id, sub_plan_id, accepted
                                );
                                Some(format!(
                                    "subscription plan {sub_plan_id} is not accepted for this \
                                     resource (accepts {accepted:?})"
                                ))
                            } else {
                                None
                            }
                        }
                        Err(AccessError::NotActive(msg)) => Some(msg.clone()),
                        // Auth failure or merchant-policy veto → bare 402, no offer.
                        Err(AccessError::Unauthorized(msg)) | Err(AccessError::Denied(msg)) => {
                            return Ok(error_response(StatusCode::PAYMENT_REQUIRED, msg));
                        }
                    };

                    return match offer_reason {
                        // 402 with the PAYMENT-REQUIRED offer header + reason body.
                        Some(reason) => {
                            let request_context = x402_core::http::RequestContext {
                                method: method.clone(),
                                path: path.clone(),
                                headers: headers_map.clone().unwrap_or_default(),
                            };
                            Ok(build_402_error_with_offer(
                                &state.server,
                                &route_config,
                                &uri_string,
                                &request_context,
                                &state.resolver,
                                &reason,
                            )
                            .await)
                        }
                        // All checks passed → serve the protected resource.
                        None => inner.call(req).await,
                    };
                }
            }

            // 3. Check for payment header
            let payment_header = payment_header;

            if payment_header.is_none() {
                // No payment provided → return 402
                // Build RequestContext only when needed (402 response with resolver)
                let request_context = x402_core::http::RequestContext {
                    method: method.clone(),
                    path: path.clone(),
                    headers: headers_map.unwrap_or_default(),
                };
                let response = build_402_response(
                    &state.server,
                    &route_config,
                    &uri_string,
                    &request_context,
                    &state.resolver,
                )
                .await;
                return Ok(response);
            }

            let payment_header = payment_header.unwrap();

            // 4. Decode payment payload
            let payment_payload = match decode_payment_signature_header(&payment_header) {
                Ok(payload) => payload,
                Err(_) => {
                    return Ok(error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid payment signature header",
                    ));
                }
            };

            // 5. Find matching payment requirements
            let accepted = &payment_payload.accepted;
            let payment_requirements = match find_matching_requirements(&route_config, accepted) {
                Some(req) => req,
                None => {
                    return Ok(error_response(
                        StatusCode::BAD_REQUEST,
                        "payment does not match any accepted payment option",
                    ));
                }
            };

            // 5a. Subscription create / change (settle-before-serve): for `period`,
            // submit the payload to the facilitator (create or change by
            // terms.changeFromSubId), then serve with a `PAYMENT-RESPONSE` carrying
            // the subId. Replaces the verify→serve→settle path for this scheme.
            if let Some(support) = &state.subscription {
                if crate::subscription::is_subscription_scheme(&payment_requirements.scheme) {
                    let chain_index = match crate::subscription::chain_index_from_network(
                        &payment_requirements.network,
                    ) {
                        Some(ci) => ci,
                        None => {
                            return Ok(error_response(
                                StatusCode::BAD_REQUEST,
                                "unsupported network for subscription",
                            ))
                        }
                    };
                    let sync_settle = route_config.sync_settle.unwrap_or(true);
                    // Verify (local: unpack + bind terms to the advertised offer),
                    // then settle (facilitator create/change by terms.changeFromSubId).
                    let verified = match crate::subscription::verify_subscription(
                        &payment_payload,
                        &route_config.accepts,
                        &payment_requirements.asset,
                    ) {
                        Ok(v) => v,
                        Err(reason) => {
                            return Ok(error_response(
                                StatusCode::PAYMENT_REQUIRED,
                                &format!("subscription verify failed: {reason}"),
                            ))
                        }
                    };
                    return match crate::subscription::settle_subscription(
                        support,
                        verified,
                        chain_index,
                        sync_settle,
                    )
                    .await
                    {
                        Ok(outcome) => {
                            let mut inner_response = inner.call(req).await?;
                            if let Some(encoded) =
                                crate::subscription::encode_subscription_response_header(&outcome)
                            {
                                if let Ok(value) = encoded.parse() {
                                    inner_response
                                        .headers_mut()
                                        .insert(PAYMENT_RESPONSE_HEADER, value);
                                }
                            }
                            Ok(inner_response)
                        }
                        Err(reason) => {
                            tracing::warn!(
                                "[x402-sub] settle failed (scheme={}): {}",
                                payment_requirements.scheme, reason
                            );
                            Ok(error_response(
                                StatusCode::PAYMENT_REQUIRED,
                                &format!("subscription settle failed: {reason}"),
                            ))
                        }
                    };
                }
            }

            // 5b. Review-wallet exemption (short-circuit BEFORE verify). When the
            // payload's declared payer is on the server's exempt allowlist and
            // the facilitator confirms the signature, serve the resource and skip
            // both verify and settle (no on-chain charge). Any other case falls
            // through to the normal flow.
            if state
                .server
                .try_exempt_signature_bypass(&payment_payload, &payment_requirements)
                .await
            {
                return inner.call(req).await;
            }

            // 6. Hook: onBeforeVerify — can abort before verification
            if let Some(hook) = &state.on_before_verify {
                let ctx = VerifyContext {
                    payment_payload: payment_payload.clone(),
                    payment_requirements: payment_requirements.clone(),
                };
                let hook_result = (hook)(ctx).await;
                if hook_result.abort {
                    let reason = hook_result
                        .reason
                        .unwrap_or_else(|| "verification aborted".to_string());
                    return Ok(error_response(StatusCode::PAYMENT_REQUIRED, &reason));
                }
            }

            // 7. Verify payment via facilitator
            let verify_result = state
                .server
                .verify_payment(&payment_payload, &payment_requirements)
                .await;

            match verify_result {
                Ok(verify_response) if !verify_response.is_valid => {
                    let reason = verify_response
                        .invalid_reason
                        .unwrap_or_else(|| "unknown".to_string());
                    if let Some(hook) = &state.on_verify_failure {
                        let ctx = VerifyContext {
                            payment_payload: payment_payload.clone(),
                            payment_requirements: payment_requirements.clone(),
                        };
                        let recovery = (hook)(ctx, reason.clone()).await;
                        match recovery {
                            Some(r) if r.recovered => {
                                // Recovered — call onAfterVerify with recovered result if available
                                if let (Some(after_hook), Some(recovered_resp)) =
                                    (&state.on_after_verify, r.result)
                                {
                                    let ctx = VerifyResultContext {
                                        payment_payload: payment_payload.clone(),
                                        payment_requirements: payment_requirements.clone(),
                                        verify_response: recovered_resp,
                                    };
                                    (after_hook)(ctx).await;
                                }
                            }
                            _ => {
                                return Ok(error_response(
                                    StatusCode::PAYMENT_REQUIRED,
                                    &format!("payment verification failed: {}", reason),
                                ));
                            }
                        }
                    } else {
                        return Ok(error_response(
                            StatusCode::PAYMENT_REQUIRED,
                            &format!("payment verification failed: {}", reason),
                        ));
                    }
                }
                Err(e) => {
                    let error_msg = format!("{}", e);
                    if let Some(hook) = &state.on_verify_failure {
                        let ctx = VerifyContext {
                            payment_payload: payment_payload.clone(),
                            payment_requirements: payment_requirements.clone(),
                        };
                        let recovery = (hook)(ctx, error_msg.clone()).await;
                        match recovery {
                            Some(r) if r.recovered => {
                                if let (Some(after_hook), Some(recovered_resp)) =
                                    (&state.on_after_verify, r.result)
                                {
                                    let ctx = VerifyResultContext {
                                        payment_payload: payment_payload.clone(),
                                        payment_requirements: payment_requirements.clone(),
                                        verify_response: recovered_resp,
                                    };
                                    (after_hook)(ctx).await;
                                }
                            }
                            _ => {
                                return Ok(error_response(
                                    StatusCode::BAD_GATEWAY,
                                    &format!("facilitator verify error: {}", error_msg),
                                ));
                            }
                        }
                    } else {
                        return Ok(error_response(
                            StatusCode::BAD_GATEWAY,
                            &format!("facilitator verify error: {}", error_msg),
                        ));
                    }
                }
                Ok(ref verify_response) => {
                    // Hook: onAfterVerify — side-effect only
                    if let Some(hook) = &state.on_after_verify {
                        let ctx = VerifyResultContext {
                            payment_payload: payment_payload.clone(),
                            payment_requirements: payment_requirements.clone(),
                            verify_response: verify_response.clone(),
                        };
                        (hook)(ctx).await;
                    }
                }
            }

            // 8. Call the inner handler (pass through)
            let mut inner_response = match inner.call(req).await {
                Ok(resp) => resp,
                Err(e) => return Err(e),
            };

            // If the route handler returned an error (>= 400), don't settle
            if inner_response.status().is_client_error()
                || inner_response.status().is_server_error()
            {
                return Ok(inner_response);
            }

            // 9. Extract settlement overrides from response header (set by route handler)
            let settlement_overrides: Option<SettlementOverrides> = inner_response
                .headers()
                .get(SETTLEMENT_OVERRIDES_HEADER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| serde_json::from_str(s).ok());

            // Remove the internal header so clients don't see it
            if settlement_overrides.is_some() {
                inner_response
                    .headers_mut()
                    .remove(SETTLEMENT_OVERRIDES_HEADER);
            }

            // 10. Hook: onBeforeSettle — can abort before settlement
            if let Some(hook) = &state.on_before_settle {
                let ctx = SettleContext {
                    payment_payload: payment_payload.clone(),
                    payment_requirements: payment_requirements.clone(),
                };
                let hook_result = (hook)(ctx).await;
                if hook_result.abort {
                    let reason = hook_result
                        .reason
                        .unwrap_or_else(|| "settlement aborted".to_string());
                    return Ok(error_response(StatusCode::PAYMENT_REQUIRED, &reason));
                }
            }

            // 11. Settle payment via facilitator
            let settle_result = state
                .server
                .settle_payment(
                    &payment_payload,
                    &payment_requirements,
                    route_config.sync_settle,
                    settlement_overrides.as_ref(),
                )
                .await;

            // 11. Handle settle result per OKX spec:
            //     - success=false → 402 (all modes)
            //     - success=true + status="success" → 200 (exact syncSettle=true, confirmed)
            //     - success=true + status="pending" or no status → 200 (async/aggr_deferred, trust facilitator)
            //     - success=true + status="timeout" → poll /settle/status → hook → 200/402
            match settle_result {
                Ok(settle_response) if !settle_response.success => {
                    // All modes: success=false → try onSettleFailure hook
                    let reason = settle_response
                        .error_reason
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string());
                    if let Some(hook) = &state.on_settle_failure {
                        let ctx = SettleContext {
                            payment_payload: payment_payload.clone(),
                            payment_requirements: payment_requirements.clone(),
                        };
                        let recovery = (hook)(ctx, reason.clone()).await;
                        if let Some(r) = recovery {
                            if r.recovered {
                                if let Some(recovered_response) = r.result {
                                    // Hook: onAfterSettle with recovered response
                                    if let Some(after_hook) = &state.on_after_settle {
                                        let ctx = SettleResultContext {
                                            payment_payload: payment_payload.clone(),
                                            payment_requirements: payment_requirements.clone(),
                                            settle_response: recovered_response.clone(),
                                        };
                                        (after_hook)(ctx).await;
                                    }
                                    let (mut parts, body) = inner_response.into_parts();
                                    if let Ok(encoded) =
                                        encode_payment_response_header(&recovered_response)
                                    {
                                        if let Ok(value) = encoded.parse() {
                                            parts.headers.insert(PAYMENT_RESPONSE_HEADER, value);
                                        }
                                    }
                                    return Ok(Response::from_parts(parts, body));
                                }
                            }
                        }
                    }
                    Ok(error_response(
                        StatusCode::PAYMENT_REQUIRED,
                        &format!("settlement failed: {}", reason),
                    ))
                }
                Ok(settle_response) => {
                    // success=true — branch on status
                    match settle_response.status.as_deref() {
                        Some("timeout") => {
                            // exact + syncSettle=true: facilitator timed out waiting for chain confirmation.
                            // Step 1: Poll /settle/status (interval 1s, deadline from config)
                            let tx_hash = settle_response.transaction.clone();
                            let poll_result = if !tx_hash.is_empty() {
                                state
                                    .server
                                    .poll_settle_status(
                                        &tx_hash,
                                        DEFAULT_POLL_INTERVAL,
                                        state.poll_deadline,
                                    )
                                    .await
                            } else {
                                PollResult::Failed
                            };

                            match poll_result {
                                PollResult::Success => {
                                    // Poll confirmed on-chain → deliver resource
                                    let (mut parts, body) = inner_response.into_parts();
                                    let mut recovered = settle_response.clone();
                                    recovered.status = Some("success".into());
                                    if let Ok(encoded) = encode_payment_response_header(&recovered)
                                    {
                                        if let Ok(value) = encoded.parse() {
                                            parts.headers.insert(PAYMENT_RESPONSE_HEADER, value);
                                        }
                                    }
                                    Ok(Response::from_parts(parts, body))
                                }
                                PollResult::Failed | PollResult::Timeout => {
                                    // Step 2: Poll unsuccessful → try developer's timeout hook
                                    if let Some(hook) = &state.timeout_hook {
                                        let tx = settle_response.transaction.clone();
                                        let network = settle_response.network.clone();
                                        let hook_result = (hook)(tx, network).await;
                                        if hook_result.confirmed {
                                            // Hook confirmed → deliver resource
                                            let (mut parts, body) = inner_response.into_parts();
                                            let mut recovered = settle_response.clone();
                                            recovered.status = Some("success".into());
                                            if let Ok(encoded) =
                                                encode_payment_response_header(&recovered)
                                            {
                                                if let Ok(value) = encoded.parse() {
                                                    parts
                                                        .headers
                                                        .insert(PAYMENT_RESPONSE_HEADER, value);
                                                }
                                            }
                                            return Ok(Response::from_parts(parts, body));
                                        }
                                    }
                                    // No hook or hook returned not confirmed → 402
                                    Ok(error_response(
                                        StatusCode::PAYMENT_REQUIRED,
                                        "settlement timed out waiting for on-chain confirmation",
                                    ))
                                }
                            }
                        }
                        _ => {
                            // status="success", "pending", or no status → deliver resource
                            // Covers: exact async (pending), exact sync confirmed (success),
                            //         aggr_deferred (success), Coinbase standard (no status)

                            // Hook: onAfterSettle — side-effect only
                            if let Some(hook) = &state.on_after_settle {
                                let ctx = SettleResultContext {
                                    payment_payload: payment_payload.clone(),
                                    payment_requirements: payment_requirements.clone(),
                                    settle_response: settle_response.clone(),
                                };
                                (hook)(ctx).await;
                            }

                            let (mut parts, body) = inner_response.into_parts();
                            if let Ok(encoded) = encode_payment_response_header(&settle_response) {
                                if let Ok(value) = encoded.parse() {
                                    parts.headers.insert(PAYMENT_RESPONSE_HEADER, value);
                                }
                            }
                            Ok(Response::from_parts(parts, body))
                        }
                    }
                }
                Err(e) => {
                    let error_msg = format!("{}", e);
                    // Hook: onSettleFailure — can recover from facilitator error
                    if let Some(hook) = &state.on_settle_failure {
                        let ctx = SettleContext {
                            payment_payload: payment_payload.clone(),
                            payment_requirements: payment_requirements.clone(),
                        };
                        let recovery = (hook)(ctx, error_msg.clone()).await;
                        if let Some(r) = recovery {
                            if r.recovered {
                                if let Some(recovered_response) = r.result {
                                    if let Some(after_hook) = &state.on_after_settle {
                                        let ctx = SettleResultContext {
                                            payment_payload: payment_payload.clone(),
                                            payment_requirements: payment_requirements.clone(),
                                            settle_response: recovered_response.clone(),
                                        };
                                        (after_hook)(ctx).await;
                                    }
                                    let (mut parts, body) = inner_response.into_parts();
                                    if let Ok(encoded) =
                                        encode_payment_response_header(&recovered_response)
                                    {
                                        if let Ok(value) = encoded.parse() {
                                            parts.headers.insert(PAYMENT_RESPONSE_HEADER, value);
                                        }
                                    }
                                    return Ok(Response::from_parts(parts, body));
                                }
                            }
                        }
                    }
                    Ok(error_response(
                        StatusCode::BAD_GATEWAY,
                        &format!("facilitator settle error: {}", error_msg),
                    ))
                }
            }
        })
    }
}

/// Canonicalize a path so lookup is invariant to slash / query noise.
/// Mirrors `payment_router_axum::router::normalize_path` so that requests
/// matched by the outer router (which normalizes) cannot bypass this
/// middleware via path variants like `/api/x/` or `//api/x` when the
/// registered key is the canonical `/api/x`.
fn normalize_path(path: &str) -> String {
    let core = path.split(['?', '#']).next().unwrap_or("");
    let mut out = String::with_capacity(core.len());
    let mut prev_slash = false;
    for ch in core.chars() {
        if ch == '/' {
            if !prev_slash {
                out.push('/');
            }
            prev_slash = true;
        } else {
            out.push(ch);
            prev_slash = false;
        }
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    if out.is_empty() {
        "/".to_string()
    } else {
        out
    }
}

/// Find a route config by trying "METHOD /path" key, then wildcard patterns.
/// The input path is normalized (strip query/fragment, collapse `//`, strip
/// trailing `/`) before lookup so requests with non-canonical paths can't
/// bypass payment by mismatching the registered (canonical) key.
fn find_route_config<'a>(
    routes: &'a RoutesConfig,
    method: &str,
    path: &str,
) -> Option<&'a RoutePaymentConfig> {
    let path = normalize_path(path);

    // Try exact "METHOD /path"
    let key = format!("{} {}", method, path);
    if let Some(config) = routes.get(&key) {
        return Some(config);
    }

    // Try wildcard patterns: "* /path" or just "/path"
    let wildcard_key = format!("* {}", path);
    if let Some(config) = routes.get(&wildcard_key) {
        return Some(config);
    }
    if let Some(config) = routes.get(&path) {
        return Some(config);
    }

    None
}

/// Find payment requirements that match the client's accepted payment.
/// Uses superset matching: the buyer's accepted.extra can contain additional
/// scheme-specific fields (e.g., sessionCert for deferred scheme) beyond
/// what the server declares.
fn find_matching_requirements(
    route_config: &RoutePaymentConfig,
    accepted: &x402_core::types::PaymentRequirements,
) -> Option<x402_core::types::PaymentRequirements> {
    for accept in &route_config.accepts {
        if accept.scheme == accepted.scheme && accept.network == accepted.network {
            // Superset match: buyer's accepted is valid as long as scheme/network match.
            // Buyer's accepted.extra may contain additional fields (e.g., sessionCert)
            // that the server doesn't declare — this is expected for deferred scheme.
            return Some(accepted.clone());
        }
    }
    None
}

/// Build the `PaymentRequired` (402 offer) from a route config, resolving
/// dynamic price/payTo via the resolver when present. `None` if no accept could
/// be built. `error` is left unset — callers carry the human reason in the body.
async fn build_payment_required(
    server: &X402ResourceServer,
    route_config: &RoutePaymentConfig,
    url: &str,
    request_context: &x402_core::http::RequestContext,
    resolver: &Option<PaymentResolverFn>,
) -> Option<PaymentRequired> {
    // Merchant override wins over the middleware-extracted URL.
    let url = route_config
        .resource
        .clone()
        .unwrap_or_else(|| url.to_string());

    // Build accepts list from route config, resolving dynamic price/payTo if resolver is set
    let mut accepts = Vec::new();
    for accept in &route_config.accepts {
        let resolved = match resolver {
            Some(resolve_fn) => resolve_fn(request_context, accept).await,
            None => ResolvedAccept {
                scheme: accept.scheme.clone(),
                price: accept.price.clone(),
                network: accept.network.clone(),
                pay_to: accept.pay_to.clone(),
                max_timeout_seconds: accept.max_timeout_seconds,
                extra: accept.extra.clone(),
            },
        };
        match server
            .build_payment_requirements(
                &resolved.scheme,
                &resolved.network,
                &resolved.price,
                &resolved.pay_to,
                resolved.max_timeout_seconds.unwrap_or(300), // default 5 min
                &ResourceInfo {
                    url: url.clone(),
                    description: Some(route_config.description.clone()),
                    mime_type: Some(route_config.mime_type.clone()),
                },
                resolved.extra.as_ref(),
            )
            .await
        {
            Ok(req) => accepts.push(req),
            Err(e) => {
                tracing::warn!("failed to build payment requirements: {}", e);
            }
        }
    }

    if accepts.is_empty() {
        return None;
    }

    Some(PaymentRequired {
        x402_version: 2,
        error: None,
        resource: ResourceInfo {
            url,
            description: Some(route_config.description.clone()),
            mime_type: Some(route_config.mime_type.clone()),
        },
        accepts,
        extensions: None,
    })
}

/// Attach the base64 `PAYMENT-REQUIRED` offer header to a response.
fn attach_payment_required_header(response: &mut Response<Body>, payment_required: &PaymentRequired) {
    if let Ok(encoded) = encode_payment_required_header(payment_required) {
        if let Ok(value) = encoded.parse() {
            response
                .headers_mut()
                .insert(PAYMENT_REQUIRED_HEADER, value);
        }
    }
}

/// 402 whose body is the full `PaymentRequired` JSON + the base64 PAYMENT-REQUIRED
/// header. Used when the buyer sent no payment at all.
async fn build_402_response(
    server: &X402ResourceServer,
    route_config: &RoutePaymentConfig,
    url: &str,
    request_context: &x402_core::http::RequestContext,
    resolver: &Option<PaymentResolverFn>,
) -> Response<Body> {
    let payment_required =
        match build_payment_required(server, route_config, url, request_context, resolver).await {
            Some(pr) => pr,
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to build payment requirements",
                )
            }
        };

    let body_json = serde_json::to_string(&payment_required).unwrap_or_default();
    let mut response = Response::builder()
        .status(StatusCode::PAYMENT_REQUIRED)
        .header("Content-Type", "application/json")
        .body(Body::from(body_json))
        .unwrap();
    attach_payment_required_header(&mut response, &payment_required);
    response
}

/// 402 whose body is a bare `{"error": msg}` (same shape as [`error_response`])
/// but which ALSO carries the PAYMENT-REQUIRED offer header, so the buyer can
/// act on the reason (subscribe / upgrade). Used for plan-gating / not-active
/// access denials.
async fn build_402_error_with_offer(
    server: &X402ResourceServer,
    route_config: &RoutePaymentConfig,
    url: &str,
    request_context: &x402_core::http::RequestContext,
    resolver: &Option<PaymentResolverFn>,
    error: &str,
) -> Response<Body> {
    let body = serde_json::json!({ "error": error }).to_string();
    let mut response = Response::builder()
        .status(StatusCode::PAYMENT_REQUIRED)
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap();
    if let Some(pr) =
        build_payment_required(server, route_config, url, request_context, resolver).await
    {
        attach_payment_required_header(&mut response, &pr);
    }
    response
}

/// Build a JSON error response.
fn error_response(status: StatusCode, message: &str) -> Response<Body> {
    let body = serde_json::json!({ "error": message });
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// `200 OK` JSON response (used by subscription cancel operation routes).
fn json_ok(body: serde_json::Value) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

// ---------------------------------------------------------------------------
// Public API: payment_middleware() function
// ---------------------------------------------------------------------------

/// Create an Axum Tower Layer for x402 payment middleware.
///
/// # Arguments
/// - `routes` - Route configurations mapping "METHOD /path" → payment config
/// - `server` - Pre-configured X402ResourceServer instance
///
/// # Returns
/// A Tower Layer that can be applied to an Axum Router via `.layer()`
pub fn payment_middleware(routes: RoutesConfig, server: X402ResourceServer) -> PaymentLayer {
    PaymentLayer::new(server, routes, None, DEFAULT_POLL_DEADLINE, None)
}

/// Create middleware with a custom poll deadline for timeout recovery.
///
/// When settle returns `status="timeout"`, SDK polls `/settle/status` every 1s
/// up to this deadline. Default is 5s.
pub fn payment_middleware_with_poll_deadline(
    routes: RoutesConfig,
    server: X402ResourceServer,
    poll_deadline: Duration,
) -> PaymentLayer {
    PaymentLayer::new(server, routes, None, poll_deadline, None)
}

/// Create middleware with a settlement timeout recovery hook.
///
/// When settle returns `status="timeout"`, SDK first polls `/settle/status`.
/// If polling fails, the hook is called as a fallback.
///
/// # Arguments
/// - `routes` - Route configurations
/// - `server` - Pre-configured X402ResourceServer instance
/// - `timeout_hook` - Fallback callback after polling fails
pub fn payment_middleware_with_timeout_hook(
    routes: RoutesConfig,
    server: X402ResourceServer,
    timeout_hook: OnSettlementTimeoutHook,
) -> PaymentLayer {
    PaymentLayer::new(
        server,
        routes,
        Some(timeout_hook),
        DEFAULT_POLL_DEADLINE,
        None,
    )
}

/// Create middleware with timeout hook and custom poll deadline.
pub fn payment_middleware_with_timeout_hook_and_deadline(
    routes: RoutesConfig,
    server: X402ResourceServer,
    timeout_hook: OnSettlementTimeoutHook,
    poll_deadline: Duration,
) -> PaymentLayer {
    PaymentLayer::new(server, routes, Some(timeout_hook), poll_deadline, None)
}

/// Builder for configuring payment middleware with lifecycle hooks.
///
/// Provides a fluent API for setting hooks, resolver, and other options.
/// Use this when you need hooks; for simple cases use `payment_middleware()`.
///
/// # Example
/// ```rust,no_run
/// # use x402_axum::*;
/// # use x402_core::server::X402ResourceServer;
/// # use std::collections::HashMap;
/// // let layer = PaymentMiddlewareBuilder::new(routes, server)
/// //     .on_before_verify(Box::new(|ctx| Box::pin(async move {
/// //         BeforeHookResult { abort: false, reason: None }
/// //     })))
/// //     .on_after_settle(Box::new(|ctx| Box::pin(async move { () })))
/// //     .build();
/// ```
pub struct PaymentMiddlewareBuilder {
    server: X402ResourceServer,
    routes: RoutesConfig,
    timeout_hook: Option<OnSettlementTimeoutHook>,
    poll_deadline: Duration,
    resolver: Option<PaymentResolverFn>,
    on_protected_request: Option<OnProtectedRequestHook>,
    on_before_verify: Option<OnBeforeVerifyHook>,
    on_after_verify: Option<OnAfterVerifyHook>,
    on_verify_failure: Option<OnVerifyFailureHook>,
    on_before_settle: Option<OnBeforeSettleHook>,
    on_after_settle: Option<OnAfterSettleHook>,
    on_settle_failure: Option<OnSettleFailureHook>,
    subscription: Option<crate::subscription::SubscriptionSupport>,
}

impl PaymentMiddlewareBuilder {
    /// Create a new builder with required parameters.
    pub fn new(routes: RoutesConfig, server: X402ResourceServer) -> Self {
        Self {
            server,
            routes,
            timeout_hook: None,
            poll_deadline: DEFAULT_POLL_DEADLINE,
            resolver: None,
            on_protected_request: None,
            on_before_verify: None,
            on_after_verify: None,
            on_verify_failure: None,
            on_before_settle: None,
            on_after_settle: None,
            on_settle_failure: None,
            subscription: None,
        }
    }

    /// Enable `period` support: the middleware will create
    /// subscriptions (settle-before-serve) and verify `APP-Access` on
    /// subsequent access. Pass a [`crate::subscription::SubscriptionSupport`]
    /// holding the facilitator client + AccessProof window.
    pub fn subscription(mut self, support: crate::subscription::SubscriptionSupport) -> Self {
        self.subscription = Some(support);
        self
    }

    /// Set the `onProtectedRequest` hook.
    pub fn on_protected_request(mut self, hook: OnProtectedRequestHook) -> Self {
        self.on_protected_request = Some(hook);
        self
    }

    /// Set the `onBeforeVerify` hook.
    pub fn on_before_verify(mut self, hook: OnBeforeVerifyHook) -> Self {
        self.on_before_verify = Some(hook);
        self
    }

    /// Set the `onAfterVerify` hook.
    pub fn on_after_verify(mut self, hook: OnAfterVerifyHook) -> Self {
        self.on_after_verify = Some(hook);
        self
    }

    /// Set the `onVerifyFailure` hook.
    pub fn on_verify_failure(mut self, hook: OnVerifyFailureHook) -> Self {
        self.on_verify_failure = Some(hook);
        self
    }

    /// Set the `onBeforeSettle` hook.
    pub fn on_before_settle(mut self, hook: OnBeforeSettleHook) -> Self {
        self.on_before_settle = Some(hook);
        self
    }

    /// Set the `onAfterSettle` hook.
    pub fn on_after_settle(mut self, hook: OnAfterSettleHook) -> Self {
        self.on_after_settle = Some(hook);
        self
    }

    /// Set the `onSettleFailure` hook.
    pub fn on_settle_failure(mut self, hook: OnSettleFailureHook) -> Self {
        self.on_settle_failure = Some(hook);
        self
    }

    /// Set the `onSettlementTimeout` hook.
    pub fn on_settlement_timeout(mut self, hook: OnSettlementTimeoutHook) -> Self {
        self.timeout_hook = Some(hook);
        self
    }

    /// Set the poll deadline for settlement timeout recovery.
    pub fn poll_deadline(mut self, deadline: Duration) -> Self {
        self.poll_deadline = deadline;
        self
    }

    /// Set the dynamic payment resolver.
    pub fn resolver(mut self, resolver: PaymentResolverFn) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Build the PaymentLayer.
    pub fn build(self) -> PaymentLayer {
        PaymentLayer::from_state(PaymentState {
            server: self.server,
            routes: self.routes,
            timeout_hook: self.timeout_hook,
            poll_deadline: self.poll_deadline,
            resolver: self.resolver,
            on_protected_request: self.on_protected_request,
            on_before_verify: self.on_before_verify,
            on_after_verify: self.on_after_verify,
            on_verify_failure: self.on_verify_failure,
            on_before_settle: self.on_before_settle,
            on_after_settle: self.on_after_settle,
            on_settle_failure: self.on_settle_failure,
            subscription: self.subscription,
        })
    }
}

/// Create middleware with a dynamic payment resolver.
///
/// The resolver is called per-request to override price/payTo in `AcceptConfig`.
/// If no resolver is needed, use `payment_middleware` instead.
pub fn payment_middleware_with_resolver(
    routes: RoutesConfig,
    server: X402ResourceServer,
    resolver: PaymentResolverFn,
) -> PaymentLayer {
    PaymentLayer::new(server, routes, None, DEFAULT_POLL_DEADLINE, Some(resolver))
}


#[cfg(test)]
mod path_matching_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn normalize_path_canonical_forms() {
        assert_eq!(normalize_path("/api/x"), "/api/x");
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn normalize_path_trailing_slash_stripped() {
        assert_eq!(normalize_path("/api/x/"), "/api/x");
    }

    #[test]
    fn normalize_path_double_slashes_collapsed() {
        assert_eq!(normalize_path("//api//x"), "/api/x");
        assert_eq!(normalize_path("/api///x/"), "/api/x");
    }

    #[test]
    fn normalize_path_query_and_fragment_stripped() {
        assert_eq!(normalize_path("/api/x?foo=bar"), "/api/x");
        assert_eq!(normalize_path("/api/x#section"), "/api/x");
    }

    #[test]
    fn normalize_path_empty_becomes_root() {
        assert_eq!(normalize_path(""), "/");
    }

    fn make_route_cfg() -> RoutePaymentConfig {
        RoutePaymentConfig {
            accepts: vec![],
            description: "test".into(),
            mime_type: "application/json".into(),
            sync_settle: None,
            resource: None,
            operation: None,
        }
    }

    /// Regression for the payment-router-axum ↔ x402-axum path bypass:
    /// the outer router normalizes paths; the x402 middleware must do the
    /// same on lookup, otherwise a request like `/api/x/` would miss the
    /// registered `/api/x` key and be treated as unprotected.
    #[test]
    fn find_route_config_matches_trailing_slash_variant() {
        let mut routes: RoutesConfig = HashMap::new();
        routes.insert("GET /api/x".into(), make_route_cfg());
        assert!(find_route_config(&routes, "GET", "/api/x/").is_some());
        assert!(find_route_config(&routes, "GET", "/api/x").is_some());
    }

    #[test]
    fn find_route_config_matches_double_slash_variant() {
        let mut routes: RoutesConfig = HashMap::new();
        routes.insert("GET /api/x".into(), make_route_cfg());
        assert!(find_route_config(&routes, "GET", "//api/x").is_some());
        assert!(find_route_config(&routes, "GET", "/api//x/").is_some());
    }

    #[test]
    fn find_route_config_method_wildcard_still_matches_after_normalize() {
        let mut routes: RoutesConfig = HashMap::new();
        routes.insert("* /api/x".into(), make_route_cfg());
        assert!(find_route_config(&routes, "GET", "/api/x/").is_some());
        assert!(find_route_config(&routes, "POST", "/api/x/").is_some());
    }

    #[test]
    fn find_route_config_path_only_key_still_matches_after_normalize() {
        let mut routes: RoutesConfig = HashMap::new();
        routes.insert("/api/x".into(), make_route_cfg());
        assert!(find_route_config(&routes, "GET", "/api/x/").is_some());
    }

    #[test]
    fn find_route_config_misses_for_truly_different_path() {
        let mut routes: RoutesConfig = HashMap::new();
        routes.insert("GET /api/x".into(), make_route_cfg());
        assert!(find_route_config(&routes, "GET", "/api/y").is_none());
    }
}
