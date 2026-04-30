//! Per-action handlers (`handle_open` / `handle_topup` / `handle_voucher` /
//! `handle_close`) — invoked by the `verify_session` dispatcher in
//! [`super::trait_impl`].

use mpp::protocol::core::{PaymentCredential, Receipt};
use mpp::protocol::intents::SessionRequest;

use super::decode::{
    decode_challenge_request_recipient, decode_method_details, extract_payer_and_signer,
    extract_str, parse_address, parse_b256, parse_optional_hex_bytes, parse_u128_default_zero,
    parse_u128_str, strip_sdk_only_open_fields,
};
use super::EvmSessionMethod;
use crate::eip712::verify_voucher;
use crate::error::SaApiError;
use crate::store::{ChannelRecord, ChannelUpdater};

impl EvmSessionMethod {
    pub(super) async fn handle_open(
        &self,
        credential: &PaymentCredential,
    ) -> Result<Receipt, SaApiError> {
        // 1. Payee consistency: challenge.recipient == signer.address().
        let challenge_recipient = decode_challenge_request_recipient(&credential.challenge.request)?;
        let signer_addr = self
            .payee_address
            .ok_or_else(|| SaApiError::new(8000, "no signer configured (call .with_signer)"))?;
        if challenge_recipient != signer_addr {
            return Err(SaApiError::new(
                8000,
                format!(
                    "payee mismatch: challenge.recipient={} but signer.address={}; \
                     SDK signer must be merchant's receiving address",
                    challenge_recipient, signer_addr
                ),
            ));
        }

        // 2. Read method_details for chain_id / escrow_contract / min_voucher_delta.
        let method_details = decode_method_details(self.method_details.as_ref())?;

        // 3. Extract SDK-only fields from credential (SA does not consume
        //    them; they get stripped before forwarding):
        //    - cumulativeAmount (initial voucher amount; defaults to 0)
        //    - initial voucher EIP-712 signature: in transaction mode it
        //      lives in `voucherSignature` (so it doesn't collide with the
        //      EIP-3009 deposit `signature`); in hash mode it occupies
        //      `signature` directly (there is no deposit signature).
        let payload = &credential.payload;
        let payload_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let voucher_sig_key = if payload_type == "hash" {
            "signature"
        } else {
            "voucherSignature"
        };
        let initial_voucher_sig = parse_optional_hex_bytes(payload.get(voucher_sig_key))?;
        let cumulative_amount = parse_u128_default_zero(payload.get("cumulativeAmount"))?;

        // 4. Parse channel_id from the client's payload (don't wait for SA's
        //    response; this enables fail-fast).
        let channel_id_str = extract_str(payload, "channelId");
        if channel_id_str.is_empty() {
            return Err(SaApiError::new(70000, "open payload missing channelId"));
        }
        let channel_id_b256 = parse_b256(channel_id_str)?;
        let escrow_contract = parse_address(&method_details.escrow_contract)?;

        // 5. Resolve payer / authorized_signer:
        // - transaction: payer = payload.authorization.from
        // - hash:        payer = parse(source DID address segment; strict did:pkh:eip155 format)
        let (payer, authorized_signer) = extract_payer_and_signer(
            payload,
            credential.source.as_deref(),
            method_details.chain_id,
        )?;

        // 6. Local fail-fast verify of the initial voucher signature
        //    (before forwarding to SA). In transaction mode this saves gas:
        //    a bad signature means we never broadcast the open tx via SA.
        //    In hash mode the client already paid gas, but doing it first
        //    is still semantically cleaner.
        if let Some(sig) = initial_voucher_sig.as_ref() {
            // 6a. Transaction mode: locally enforce cum <= client-claimed
            //     authorization.value (the deposit). In hash mode the
            //     deposit only becomes known after SA returns the receipt,
            //     so the check is deferred.
            if payload_type != "hash" {
                let claimed_deposit = parse_u128_str(extract_str(
                    payload.get("authorization").unwrap_or(&serde_json::Value::Null),
                    "value",
                ))?;
                if cumulative_amount > claimed_deposit {
                    return Err(SaApiError::new(
                        70012,
                        format!(
                            "initial voucher cumulativeAmount {cumulative_amount} exceeds claimed deposit {claimed_deposit}"
                        ),
                    ));
                }
            }
            // 6b. Strict EIP-712 ecrecover.
            verify_voucher(
                &self.domain_meta,
                escrow_contract,
                method_details.chain_id,
                channel_id_b256,
                cumulative_amount,
                sig,
                authorized_signer,
            )
            .map_err(|e| SaApiError::new(70004, format!("initial voucher: {e}")))?;
        }

        // 7. Forward credential to SA — SDK-only fields must be stripped
        //    first (SA spec doesn't list them).
        //    Transaction mode strips cumulativeAmount + voucherSignature
        //    (keeps EIP-3009 signature). Hash mode strips cumulativeAmount
        //    + signature (the entire `signature` is the SDK-only voucher sig).
        let credential_for_sa = strip_sdk_only_open_fields(credential)?;
        let receipt = self.sa_client.session_open(&credential_for_sa).await?;

        // 8. SA on-chain sanity: returned channelId must match the client-claimed one.
        if !receipt.channel_id.eq_ignore_ascii_case(channel_id_str) {
            return Err(SaApiError::new(
                8000,
                format!(
                    "channelId mismatch: client claimed {} but SA returned {}",
                    channel_id_str, receipt.channel_id
                ),
            ));
        }

        // 9. Resolve deposit (both modes have one by this point):
        //    - transaction: client payload.authorization.value (already used in 6a).
        //    - hash:        SA receipt.deposit (authoritative on-chain value).
        let deposit = if payload_type == "hash" {
            let dep_str = receipt
                .deposit
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    SaApiError::new(70000, "hash mode: SA session_open response missing deposit")
                })?;
            parse_u128_str(dep_str)?
        } else {
            parse_u128_str(extract_str(
                payload.get("authorization").unwrap_or(&serde_json::Value::Null),
                "value",
            ))?
        };

        // 9b. Hash mode: deferred cum-vs-deposit check (transaction already did this in 6a).
        if payload_type == "hash" && cumulative_amount > deposit {
            return Err(SaApiError::new(
                70012,
                format!(
                    "initial voucher cumulativeAmount {cumulative_amount} exceeds on-chain deposit {deposit}"
                ),
            ));
        }

        let min_voucher_delta = method_details
            .min_voucher_delta
            .as_deref()
            .map(parse_u128_str)
            .transpose()?;

        // 10. Write store.
        let channel_id = receipt.channel_id.clone();
        let record = ChannelRecord {
            channel_id: channel_id.clone(),
            chain_id: method_details.chain_id,
            escrow_contract,
            payer,
            payee: signer_addr,
            authorized_signer,
            deposit,
            highest_voucher_amount: cumulative_amount,
            highest_voucher_signature: initial_voucher_sig,
            min_voucher_delta,
            spent: 0,
            units: 0,
        };
        self.store.put(record).await;

        Ok(Receipt::success(
            "evm",
            receipt.reference.clone().unwrap_or(channel_id),
        ))
    }

    pub(super) async fn handle_topup(
        &self,
        credential: &PaymentCredential,
    ) -> Result<Receipt, SaApiError> {
        // Pre-flight: reject `additionalDeposit == 0` before hitting SA. Saves
        // a wasted round-trip and prevents no-op records from polluting state.
        let additional = parse_u128_str(extract_str(&credential.payload, "additionalDeposit"))?;
        if additional == 0 {
            return Err(SaApiError::new(
                70000,
                "topUp additionalDeposit must be greater than 0",
            ));
        }

        // session/topUp doesn't need challenge — send { source, payload }.
        let mut body = serde_json::json!({ "payload": credential.payload });
        if let Some(s) = credential.source.as_deref() {
            body["source"] = serde_json::Value::String(s.to_string());
        }
        let receipt = self.sa_client.session_top_up(&body).await?;

        // Accumulate deposit.
        let updater: ChannelUpdater = Box::new(move |r: &mut ChannelRecord| {
            r.deposit = r
                .deposit
                .checked_add(additional)
                .ok_or_else(|| SaApiError::new(8000, "deposit overflow"))?;
            Ok(())
        });
        // If the local record is missing (e.g. topUp arriving after an SDK
        // restart), `update` returns 70010 — but SA already succeeded on-chain,
        // so we only log a warning instead of blocking. Local state will be
        // inconsistent until the merchant either restarts a clean session or
        // implements a `session/status`-based recovery path.
        // TODO: auto-recover by calling `session_status` and rebuilding the
        // ChannelRecord from on-chain truth (FR-recover, gap A in lifecycle audit).
        if let Err(e) = self.store.update(&receipt.channel_id, updater).await {
            tracing::warn!(channel_id = %receipt.channel_id, error = %e, "topup local update skipped");
        }
        Ok(Receipt::success(
            "evm",
            receipt.reference.clone().unwrap_or(receipt.channel_id),
        ))
    }

    pub(super) async fn handle_voucher(
        &self,
        credential: &PaymentCredential,
        request: &SessionRequest,
    ) -> Result<Receipt, SaApiError> {
        let payload = &credential.payload;
        let channel_id = extract_str(payload, "channelId");
        let cum = parse_u128_str(extract_str(payload, "cumulativeAmount"))?;
        let sig = parse_optional_hex_bytes(payload.get("signature"))?
            .ok_or_else(|| SaApiError::new(70000, "voucher missing signature"))?;
        // Byte-level replay only skips verify + highest update; deduct
        // still runs. Matches mppx / OKX TS Session: a client can sign one
        // large voucher and replay the same bytes to drain the balance.
        // Double-deduct protection on network retries belongs at the
        // challenge.id level (TS also doesn't do this; future work).
        self.submit_voucher(channel_id, cum, sig).await?;
        let amount = parse_u128_str(&request.amount)?;
        let updated = self.deduct_from_channel(channel_id, amount).await?;
        let (spent, units) = (updated.spent, updated.units);

        self.voucher_deduct_results
            .lock()
            .unwrap()
            .insert(credential.challenge.id.clone(), (spent, units));

        Ok(Receipt::success("evm", channel_id.to_string()))
    }

    pub(super) async fn handle_close(
        &self,
        credential: &PaymentCredential,
    ) -> Result<Receipt, SaApiError> {
        let payload = &credential.payload;
        let channel_id = extract_str(payload, "channelId");
        let cum = parse_u128_str(extract_str(payload, "cumulativeAmount"))?;
        let voucher_sig = parse_optional_hex_bytes(payload.get("signature"))?;

        // The payer-provided final voucher must be locally verified first (B-1 path).
        if let Some(sig) = voucher_sig.as_ref() {
            let channel_id_b256 = parse_b256(channel_id)?;
            let channel = self
                .store
                .get(channel_id)
                .await
                .ok_or_else(|| SaApiError::new(70010, "channel not found in local store"))?;
            verify_voucher(
                &self.domain_meta,
                channel.escrow_contract,
                channel.chain_id,
                channel_id_b256,
                cum,
                sig,
                channel.voucher_signer(),
            )
            .map_err(|e| SaApiError::new(70004, format!("close voucher: {e}")))?;
        }

        let receipt = self
            .close_with_authorization(channel_id, Some(cum), voucher_sig)
            .await?;
        Ok(Receipt::success(
            "evm",
            receipt.reference.clone().unwrap_or(receipt.channel_id),
        ))
    }
}
