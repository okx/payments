//! Per-action handlers (`handle_open` / `handle_topup` / `handle_voucher` /
//! `handle_close`) — invoked by the `verify_session` dispatcher in
//! [`super::trait_impl`].

use mpp::protocol::core::{PaymentCredential, Receipt};
use mpp::protocol::intents::SessionRequest;

use super::decode::{
    decode_challenge_request_recipient, decode_method_details, extract_payer_and_signer,
    extract_required_str, parse_address, parse_b256, parse_optional_hex_bytes,
    parse_u128_default_zero, parse_u128_str, strip_sdk_only_open_fields,
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
        let challenge_recipient =
            decode_challenge_request_recipient(&credential.challenge.request)?;
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
        //
        // Use an explicit match (not `if hash else default`) so a future
        // payload type or a missing `type` field surfaces as a 70000
        // error here rather than silently mapping to "voucherSignature".
        // (Review #6)
        let payload = &credential.payload;
        // Require `type` explicitly so a missing field surfaces as
        // "missing required field type" rather than the confusing
        // "unsupported type \"\"" we'd get from `unwrap_or("")`.
        let payload_type = payload
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                SaApiError::new(
                    70000,
                    "open payload missing required field type (expected transaction|hash)",
                )
            })?;
        let voucher_sig_key = match payload_type {
            "transaction" => "voucherSignature",
            "hash" => "signature",
            other => {
                return Err(SaApiError::new(
                    70000,
                    format!("unsupported open payload type {other:?} (expected transaction|hash)"),
                ));
            }
        };
        let initial_voucher_sig = parse_optional_hex_bytes(payload.get(voucher_sig_key))?;
        let cumulative_amount = parse_u128_default_zero(payload.get("cumulativeAmount"))?;

        // 4. Parse channel_id from the client's payload (don't wait for SA's
        //    response; this enables fail-fast).
        let channel_id_str = extract_required_str(payload, "channelId")?;
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
                let authorization = payload.get("authorization").ok_or_else(|| {
                    SaApiError::new(70000, "transaction mode missing authorization object")
                })?;
                let claimed_deposit =
                    parse_u128_str(extract_required_str(authorization, "value")?)?;
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

        // 9. Resolve deposit. **Prefer SA's authoritative `receipt.deposit`
        //    in both modes**; fall back to the client-claimed
        //    `payload.authorization.value` only for transaction mode when
        //    SA omits the field. This keeps the source-of-truth consistent
        //    across modes — earlier transaction mode trusted only the
        //    client value, which would silently store a wrong cap if SA's
        //    deposit ever differed (fee-on-transfer tokens, future
        //    rounding).  Hash mode has no client-side deposit to fall back
        //    to, so SA must populate it. (Review #6)
        let deposit = match (
            receipt.deposit.as_deref().filter(|s| !s.is_empty()),
            payload_type,
        ) {
            (Some(dep), _) => parse_u128_str(dep)?,
            (None, "hash") => {
                return Err(SaApiError::new(
                    70000,
                    "hash mode: SA session_open response missing deposit",
                ));
            }
            (None, _) => {
                let authorization = payload.get("authorization").ok_or_else(|| {
                    SaApiError::new(70000, "transaction mode missing authorization object")
                })?;
                parse_u128_str(extract_required_str(authorization, "value")?)?
            }
        };

        // 9b. Re-check cum vs the resolved deposit. For transaction mode
        //     this is normally redundant with 6a (which used the
        //     client-claimed value), but if SA returned a smaller
        //     authoritative deposit (e.g. fee-on-transfer), this catches it
        //     before the local store is written. For hash mode this is the
        //     first-and-only chance to enforce the bound.
        if cumulative_amount > deposit {
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
        let additional = parse_u128_str(extract_required_str(
            &credential.payload,
            "additionalDeposit",
        )?)?;
        if additional == 0 {
            return Err(SaApiError::new(
                70000,
                "topUp additionalDeposit must be greater than 0",
            ));
        }

        // Hold the per-channel lock for the whole topup so we can't race
        // `handle_close` (which removes the record). Without this guard,
        // a close could win the lock between SA's on-chain accept and the
        // local store update, leaving on-chain funds in escrow that the
        // SDK no longer tracks. (H3.b)
        let channel_id_for_lock =
            extract_required_str(&credential.payload, "channelId")?.to_string();
        let _guard = self.channel_locks.lock(&channel_id_for_lock).await;

        // **Pre-flight existence check.** Refuse the topup outright if we
        // don't already hold a `ChannelRecord` for this channel — typically
        // because the SDK restarted with an in-memory store, or the
        // merchant is running multiple SDK instances and this one didn't
        // open the channel. Without this, SA would broadcast the on-chain
        // top-up and return 200, but the post-SA local update would fail
        // with 70010, leaving on-chain deposit ahead of local cap → all
        // subsequent vouchers blocked by the local `cum > deposit` guard.
        // Catching this BEFORE the SA call keeps on-chain state and local
        // state aligned. (Review #4)
        if self.store.get(&channel_id_for_lock).await.is_none() {
            return Err(SaApiError::new(
                70010,
                "channel not found in local store; refusing topUp before SA broadcast \
                 to avoid on-chain/local divergence",
            ));
        }

        // session/topUp body shape: `{ source?, payload }` — no challenge
        // wrapper. Per the [Pay] MPP EVM API plan §8.3 ACTION_TOPUP, the
        // topUp endpoint accepts the credential payload directly: SA
        // re-derives the channel-binding from `payload.channelId` plus the
        // on-chain authorization, so a stand-alone challenge isn't needed.
        // Re-verify against the latest spec when upgrading SA contracts.
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
        // Residual race: pre-flight passed (with the per-channel lock held)
        // but the record vanished before we could update it. Under the
        // default SDK API path this is unreachable — `handle_close` holds
        // the same lock — but a custom `SessionStore` impl that mutates
        // out-of-band could trigger it. SA already broadcast on-chain, so
        // we cannot rewind the deposit; we instead surface the divergence
        // as 8000 with the on-chain reference and a reconcile hint, so the
        // merchant's caller can act (call `session_status`, refresh local
        // state, decide whether to retry vouchers). Silently returning Ok
        // would let further vouchers get rejected by the (stale) local
        // `cum > deposit` guard with no signal to recover.
        if let Err(e) = self.store.update(&receipt.channel_id, updater).await {
            tracing::warn!(
                channel_id = %receipt.channel_id,
                reference = receipt.reference.as_deref().unwrap_or(""),
                error = %e,
                "topup on-chain ok but local store update failed",
            );
            return Err(SaApiError::new(
                8000,
                format!(
                    "topup on-chain succeeded (reference={}) but local store update failed: \
                     {e}; call session_status to reconcile before sending further vouchers",
                    receipt.reference.as_deref().unwrap_or("(none)"),
                ),
            ));
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
        let channel_id = extract_required_str(payload, "channelId")?;
        let cum = parse_u128_str(extract_required_str(payload, "cumulativeAmount")?)?;
        let sig = parse_optional_hex_bytes(payload.get("signature"))?
            .ok_or_else(|| SaApiError::new(70000, "voucher missing signature"))?;
        let amount = parse_u128_str(&request.amount)?;

        // Hold the per-channel lock for the entire submit + deduct pair.
        // Without this, a future drop (client disconnect) between the two
        // public-API calls would leave `highest_voucher_amount` advanced
        // but `spent` un-incremented — i.e. merchant under-bills. (H3.c)
        //
        // Byte-level replay only skips verify + highest update; deduct
        // still runs. Matches mppx / OKX TS Session: a client can sign one
        // large voucher and replay the same bytes to drain the balance.
        let _guard = self.channel_locks.lock(channel_id).await;
        self.submit_voucher_locked(channel_id, cum, sig).await?;
        let updated = self.deduct_from_channel_locked(channel_id, amount).await?;
        let (spent, units) = (updated.spent, updated.units);
        drop(_guard);

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
        let channel_id = extract_required_str(payload, "channelId")?;
        let cum = parse_u128_str(extract_required_str(payload, "cumulativeAmount")?)?;
        let voucher_sig = parse_optional_hex_bytes(payload.get("signature"))?;

        // Hold the per-channel lock across verify + close so a concurrent
        // voucher cannot land between our verify (which reads the store
        // unlocked) and `close_with_authorization`'s own store read.
        // Mirrors the H3.c handle_voucher pattern. (Review #3)
        let _guard = self.channel_locks.lock(channel_id).await;

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
            .close_with_authorization_locked(channel_id, Some(cum), voucher_sig)
            .await?;
        drop(_guard);
        Ok(Receipt::success(
            "evm",
            receipt.reference.clone().unwrap_or(receipt.channel_id),
        ))
    }
}
