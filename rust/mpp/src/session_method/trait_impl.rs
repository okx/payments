//! `impl SessionMethod for EvmSessionMethod` — dispatcher for `verify_session`
//! and the `respond` hook returning management responses.

use std::future::Future;

use mpp::protocol::core::{PaymentCredential, Receipt};
use mpp::protocol::intents::SessionRequest;
use mpp::protocol::traits::{SessionMethod, VerificationError};

use super::decode::{
    extract_str_or_empty, ACTION_CLOSE, ACTION_OPEN, ACTION_TOPUP, ACTION_VOUCHER,
};
use super::EvmSessionMethod;
use crate::error::SaApiError;

impl SessionMethod for EvmSessionMethod {
    fn method(&self) -> &str {
        "evm"
    }

    fn challenge_method_details(&self) -> Option<serde_json::Value> {
        self.method_details.clone()
    }

    fn verify_session(
        &self,
        credential: &PaymentCredential,
        request: &SessionRequest,
    ) -> impl Future<Output = Result<Receipt, VerificationError>> + Send {
        let credential = credential.clone();
        let request = request.clone();
        let challenge_id = credential.challenge.id.clone();
        let this = self.clone();

        async move {
            let action = extract_str_or_empty(&credential.payload, "action");

            let result: Result<Receipt, SaApiError> = match action {
                ACTION_OPEN => this.handle_open(&credential).await,
                ACTION_TOPUP => this.handle_topup(&credential).await,
                ACTION_VOUCHER => this.handle_voucher(&credential, &request).await,
                ACTION_CLOSE => this.handle_close(&credential).await,
                other => {
                    return Err(VerificationError::new(format!(
                        "unknown session action: {:?}",
                        other
                    )));
                }
            };

            result.map_err(|e| {
                let problem = e.to_problem_details(Some(&challenge_id));
                VerificationError::new(problem.detail)
            })
        }
    }

    fn respond(
        &self,
        credential: &PaymentCredential,
        receipt: &Receipt,
    ) -> Option<serde_json::Value> {
        // Management actions (open/topUp/close) return a minimal response.
        // The voucher action returns the deduct snapshot (spent/units).
        // The `reference` field is SA's on-chain tx hash, falling back to channelId.
        let action = extract_str_or_empty(&credential.payload, "action");
        let channel_id = extract_str_or_empty(&credential.payload, "channelId");
        match action {
            ACTION_OPEN | ACTION_TOPUP | ACTION_CLOSE => Some(serde_json::json!({
                "action":     action,
                "status":     "ok",
                "channelId":  channel_id,
                "reference":  receipt.reference,
            })),
            ACTION_VOUCHER => {
                // Read the deduct result `handle_voucher` stashed; remove
                // immediately so the map can't grow unbounded.
                let challenge_id = &credential.challenge.id;
                let deduct = self
                    .voucher_deduct_results
                    .lock()
                    .unwrap()
                    .remove(challenge_id);
                match deduct {
                    Some((spent, units)) => Some(serde_json::json!({
                        "action":    action,
                        "status":    "ok",
                        "channelId": channel_id,
                        "spent":     spent.to_string(),
                        "units":     units,
                    })),
                    None => {
                        // Should not happen on a successful voucher path:
                        // `handle_voucher` records the deduct snapshot
                        // before returning Ok, and `respond` is only called
                        // after `verify_session` returns Ok. A None here
                        // indicates an internal bug or a duplicate
                        // `respond` invocation for the same challenge_id.
                        tracing::warn!(
                            target: "mpp_evm::session",
                            %challenge_id,
                            %channel_id,
                            "voucher deduct snapshot missing in respond — \
                             possible internal bug or duplicate respond call",
                        );
                        None
                    }
                }
            }
            _ => None,
        }
    }
}
