//! EvmSessionMethod —— SessionMethod 的 OKX SA API 实现。
//!
//! 与上一版（mpp-rs TempoSessionMethod 改造）不同的核心点：
//! 1. **Voucher 本地化**：ACTION_VOUCHER 不再转发 SA API，而是 SDK 本地验签 +
//!    存入本地 Store。SA API `/session/voucher` 已废弃。
//! 2. **Settle / Close 商户主动**：商户调 `settle_with_authorization()` /
//!    `close_with_authorization()`，SDK 本地签 SettleAuth/CloseAuth 后组装
//!    扁平 payload（无 challenge wrapper）→ POST SA API。
//! 3. **去掉 idle timer**：商户自管关闭时机，SDK 不再做 5 分钟自动 settle。
//! 4. **Payee 一致性校验**：ACTION_OPEN 时校验
//!    `signer.address() == challenge.recipient`，不一致拒绝写 store。
//! 5. **本地 store 无回源**：`store.get` miss 时直接返 None；不调 SA
//!    `session_status` 自动回源（Q15 - 回源拿不到 cumulativeAmount，重启后中
//!    间未 settle 的 voucher 必然丢失，由商户决定是否实现持久化 store）。
//!
//! Signer 注入：首版 `with_signer` 限定 [`PrivateKeySigner`]（对齐 Tempo）。
//! KMS / Ledger 等场景留给 V2 抽象成 `dyn Signer`。

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex as StdMutex};

use alloy_primitives::{hex, Address, B256, Bytes, U256};
use alloy_signer_local::PrivateKeySigner;
use mpp::protocol::core::{PaymentCredential, Receipt};
use mpp::protocol::intents::SessionRequest;
use mpp::protocol::traits::{SessionMethod, VerificationError};
use tokio::sync::Mutex as AsyncMutex;

use crate::eip712::{sign_close_authorization, sign_settle_authorization, verify_voucher};
use crate::error::SaApiError;
use crate::nonce::{NonceProvider, UuidNonceProvider};
use crate::sa_client::SaApiClient;
use crate::store::{
    ChannelRecord, ChannelUpdater, InMemorySessionStore, SessionStore,
};
use crate::types::{
    ChannelStatus, CloseRequestPayload, SessionMethodDetails, SessionReceipt, SettleRequestPayload,
    DEFAULT_CHAIN_ID,
};

/// Session credential action names (spec §8.3).
const ACTION_OPEN: &str = "open";
const ACTION_VOUCHER: &str = "voucher";
const ACTION_TOPUP: &str = "topUp";
const ACTION_CLOSE: &str = "close";

// ===================== ChannelLocks =====================

/// Per-channel 互斥锁池。`submit_voucher` / `settle` / `close` 同 channelId 串行，
/// 防止并发 voucher 的 lost update。不同 channelId 完全独立。
#[derive(Default)]
struct ChannelLocks {
    inner: StdMutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl ChannelLocks {
    /// 拿 per-channelId 锁；持锁期间内的所有读写都串行。
    async fn lock(&self, channel_id: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let mutex = {
            let mut map = self.inner.lock().unwrap();
            map.entry(channel_id.to_string())
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        };
        mutex.lock_owned().await
    }
}

// ===================== EvmSessionMethod =====================

/// Default deadline = `U256::MAX` —— 等同永不过期（按问题一会议决议）。
fn default_deadline() -> U256 {
    U256::MAX
}

/// EVM Session Method backed by OKX SA API.
#[derive(Clone)]
pub struct EvmSessionMethod {
    sa_client: Arc<dyn SaApiClient>,
    store: Arc<dyn SessionStore>,
    /// Method details for challenge generation (chainId, escrowContract, ...).
    method_details: Option<serde_json::Value>,

    // ---- 新增 ----
    signer: Option<Arc<PrivateKeySigner>>,
    /// `signer.address()`，缓存避免反复算。`None` 表示未注入 signer。
    payee_address: Option<Address>,
    nonce_provider: Arc<dyn NonceProvider>,
    /// Settle / Close Authorization 签名的 deadline。默认 `U256::MAX`，可配。
    default_deadline: U256,
    /// Per-channelId 互斥锁。
    channel_locks: Arc<ChannelLocks>,
}

impl EvmSessionMethod {
    /// 用默认内存 store 创建。
    pub fn new(sa_client: Arc<dyn SaApiClient>) -> Self {
        Self::with_store(sa_client, Arc::new(InMemorySessionStore::new()))
    }

    /// 自定义 store。
    pub fn with_store(sa_client: Arc<dyn SaApiClient>, store: Arc<dyn SessionStore>) -> Self {
        Self {
            sa_client,
            store,
            method_details: None,
            signer: None,
            payee_address: None,
            nonce_provider: Arc::new(UuidNonceProvider),
            default_deadline: default_deadline(),
            channel_locks: Arc::new(ChannelLocks::default()),
        }
    }

    /// 注入 signer（首版限定 `PrivateKeySigner`，KMS/Ledger 留给 V2）。
    pub fn with_signer(mut self, signer: PrivateKeySigner) -> Self {
        self.payee_address = Some(signer.address());
        self.signer = Some(Arc::new(signer));
        self
    }

    /// 注入自定义 nonce 分配器（默认 [`UuidNonceProvider`]）。
    pub fn with_nonce_provider(mut self, p: Arc<dyn NonceProvider>) -> Self {
        self.nonce_provider = p;
        self
    }

    /// 自定义签名 deadline（默认 `U256::MAX`，永不过期）。
    pub fn with_deadline(mut self, d: U256) -> Self {
        self.default_deadline = d;
        self
    }

    /// 设置 challenge `methodDetails`（chainId / escrowContract / ...）。
    pub fn with_method_details(mut self, details: serde_json::Value) -> Self {
        self.method_details = Some(details);
        self
    }

    /// 类型化 builder：用 [`SessionMethodDetails`] 直接设置。
    pub fn with_typed_method_details(
        mut self,
        details: SessionMethodDetails,
    ) -> Result<Self, serde_json::Error> {
        self.method_details = Some(serde_json::to_value(&details)?);
        Ok(self)
    }

    /// 极简 builder：只填 escrow，chain_id 用 X Layer 默认值。
    pub fn with_escrow(self, escrow_contract: impl Into<String>) -> Self {
        let details = SessionMethodDetails {
            chain_id: DEFAULT_CHAIN_ID,
            escrow_contract: escrow_contract.into(),
            channel_id: None,
            min_voucher_delta: None,
            fee_payer: None,
            splits: None,
        };
        self.with_typed_method_details(details).unwrap()
    }

    /// Store 引用，handler 集成用。
    pub fn store(&self) -> Arc<dyn SessionStore> {
        self.store.clone()
    }

    /// 只读：channel 状态查询（透传 SA API）。
    pub async fn status(&self, channel_id: &str) -> Result<ChannelStatus, VerificationError> {
        self.sa_client
            .session_status(channel_id)
            .await
            .map_err(to_verification_error)
    }

    // ===================== submit_voucher（本地处理，9 步守卫）=====================

    /// 本地处理 voucher：验签 + 原子更新 highest_voucher。**业务层不直接调**，
    /// 由 [`SessionMethod::verify_session`] 的 `ACTION_VOUCHER` 分支触发。
    pub async fn submit_voucher(
        &self,
        channel_id: &str,
        cumulative_amount: u128,
        signature: Bytes,
    ) -> Result<(), SaApiError> {
        // A. per-channel lock
        let _guard = self.channel_locks.lock(channel_id).await;

        // B. 取本地 record（miss 直接 70010，不自动回源 — 见模块注释 #5）
        let channel = self
            .store
            .get(channel_id)
            .await
            .ok_or_else(|| SaApiError::new(70010, "channel not found in local store"))?;

        // C. 金额上限守卫（≤ deposit）
        if cumulative_amount > channel.deposit {
            return Err(SaApiError::new(70012, "amount exceeds deposit"));
        }

        // D. 字节级幂等（cum + signature 都精确相等 → 视为重发，不再验签）
        if cumulative_amount <= channel.highest_voucher_amount {
            let exact_replay = channel.highest_voucher_signature.as_ref().is_some_and(|s| {
                s == &signature && cumulative_amount == channel.highest_voucher_amount
            });
            if exact_replay {
                return Ok(());
            }
            // API doc 没定义 "not increasing" 错误码，暂用 70000 invalid_params（Q17）
            return Err(SaApiError::new(
                70000,
                "voucher cumulative not strictly increasing",
            ));
        }

        // E. min_delta 节流
        if let Some(min_delta) = channel.min_voucher_delta {
            if cumulative_amount - channel.highest_voucher_amount < min_delta {
                return Err(SaApiError::new(70013, "delta too small"));
            }
        }

        // F. EIP-712 验签（本地）
        let channel_id_b256 = parse_b256(channel_id)?;
        verify_voucher(
            channel.escrow_contract,
            channel.chain_id,
            channel_id_b256,
            cumulative_amount,
            &signature,
            channel.voucher_signer(),
        )
        .map_err(|e| SaApiError::new(70004, format!("verify voucher: {e}")))?;

        // G. 原子更新本地存储
        let updater: ChannelUpdater = Box::new(move |c: &mut ChannelRecord| {
            c.highest_voucher_amount = cumulative_amount;
            c.highest_voucher_signature = Some(signature);
            Ok(())
        });
        self.store.update(channel_id, updater).await?;

        Ok(())
    }

    // ===================== settle / close 商户主动调用 =====================

    /// 主动结算：取本地最新 voucher → 本地签 SettleAuth → 调 SA `/session/settle`。
    pub async fn settle_with_authorization(
        &self,
        channel_id: &str,
    ) -> Result<SessionReceipt, SaApiError> {
        let _guard = self.channel_locks.lock(channel_id).await;
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| SaApiError::new(8000, "no signer configured (call .with_signer)"))?;
        let payee = self
            .payee_address
            .ok_or_else(|| SaApiError::new(8000, "payee address missing"))?;

        let channel = self
            .store
            .get(channel_id)
            .await
            .ok_or_else(|| SaApiError::new(70010, "channel not found in local store"))?;

        let cumulative = channel.highest_voucher_amount;
        let voucher_sig_bytes = channel
            .highest_voucher_signature
            .clone()
            .ok_or_else(|| SaApiError::new(70000, "no voucher to settle"))?;

        let channel_id_b256 = parse_b256(channel_id)?;
        let nonce = self.nonce_provider.allocate(payee, channel_id_b256).await?;
        let deadline = self.default_deadline;

        let signed = sign_settle_authorization(
            signer.as_ref(),
            channel.escrow_contract,
            channel.chain_id,
            channel_id_b256,
            cumulative,
            nonce,
            deadline,
        )
        .await?;

        let payload = SettleRequestPayload {
            action: Some("settle".into()),
            channel_id: channel_id.to_string(),
            cumulative_amount: cumulative.to_string(),
            voucher_signature: hex_with_prefix(&voucher_sig_bytes),
            payee_signature: hex_with_prefix(&signed.signature),
            nonce: nonce.to_string(),
            deadline: deadline.to_string(),
        };
        self.sa_client.session_settle(&payload).await
    }

    /// 主动关闭：取本地最新 voucher → 本地签 CloseAuth → 调 SA `/session/close`，
    /// 成功后从 store 删除 ChannelRecord。
    ///
    /// `cumulative_amount = None` 表示用本地 highest（典型场景）；
    /// `Some(amt)` 表示由调用方指定（B-1 路径，payer 提供最终 voucher 时使用）。
    pub async fn close_with_authorization(
        &self,
        channel_id: &str,
        cumulative_amount: Option<u128>,
        provided_voucher_sig: Option<Bytes>,
    ) -> Result<SessionReceipt, SaApiError> {
        let _guard = self.channel_locks.lock(channel_id).await;
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| SaApiError::new(8000, "no signer configured (call .with_signer)"))?;
        let payee = self
            .payee_address
            .ok_or_else(|| SaApiError::new(8000, "payee address missing"))?;

        let channel = self
            .store
            .get(channel_id)
            .await
            .ok_or_else(|| SaApiError::new(70010, "channel not found in local store"))?;

        let cumulative = cumulative_amount.unwrap_or(channel.highest_voucher_amount);
        let voucher_sig_bytes = provided_voucher_sig
            .or_else(|| channel.highest_voucher_signature.clone())
            .ok_or_else(|| SaApiError::new(70000, "no voucher to close"))?;

        let channel_id_b256 = parse_b256(channel_id)?;

        // 当 voucher 是 payer 通过 ACTION_CLOSE 提供的 / 或者本地 highest 但
        // 已经被 submit_voucher 验过 — 这里不再重复验。验签责任在 ACTION_CLOSE
        // 入口完成。

        let nonce = self.nonce_provider.allocate(payee, channel_id_b256).await?;
        let deadline = self.default_deadline;

        let signed = sign_close_authorization(
            signer.as_ref(),
            channel.escrow_contract,
            channel.chain_id,
            channel_id_b256,
            cumulative,
            nonce,
            deadline,
        )
        .await?;

        let payload = CloseRequestPayload {
            action: Some("close".into()),
            channel_id: channel_id.to_string(),
            cumulative_amount: cumulative.to_string(),
            // 首版统一传非空（Q20 待 Michael 确认是否区分 waiver 分支）
            voucher_signature: hex_with_prefix(&voucher_sig_bytes),
            payee_signature: hex_with_prefix(&signed.signature),
            nonce: nonce.to_string(),
            deadline: deadline.to_string(),
        };

        let receipt = self.sa_client.session_close(&payload).await?;
        // close 成功后直接从 store 删除（不是置 finalized）
        self.store.remove(channel_id).await;
        Ok(receipt)
    }
}

// ===================== Helpers =====================

fn to_verification_error(err: SaApiError) -> VerificationError {
    let problem = err.to_problem_details(None);
    VerificationError::new(problem.detail)
}

fn extract_str<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn parse_b256(s: &str) -> Result<B256, SaApiError> {
    s.parse::<B256>()
        .map_err(|e| SaApiError::new(70000, format!("invalid bytes32 channelId {s}: {e}")))
}

fn parse_address(s: &str) -> Result<Address, SaApiError> {
    s.parse::<Address>()
        .map_err(|e| SaApiError::new(70000, format!("invalid address {s}: {e}")))
}

fn parse_u128_str(s: &str) -> Result<u128, SaApiError> {
    s.parse::<u128>()
        .map_err(|e| SaApiError::new(70000, format!("invalid u128 {s}: {e}")))
}

/// 解析可选 u128 字段：缺失 / 空串 / null 视为 0。
fn parse_u128_default_zero(v: Option<&serde_json::Value>) -> Result<u128, SaApiError> {
    match v.and_then(|x| x.as_str()) {
        None | Some("") => Ok(0),
        Some(s) => parse_u128_str(s),
    }
}

/// 解析可选 hex bytes 字段（"0x..." 或 ""/null）。
fn parse_optional_hex_bytes(v: Option<&serde_json::Value>) -> Result<Option<Bytes>, SaApiError> {
    match v.and_then(|x| x.as_str()) {
        None | Some("") => Ok(None),
        Some(s) => {
            let stripped = s.strip_prefix("0x").unwrap_or(s);
            let bytes = hex::decode(stripped)
                .map_err(|e| SaApiError::new(70000, format!("invalid hex {s}: {e}")))?;
            Ok(Some(Bytes::from(bytes)))
        }
    }
}

fn hex_with_prefix(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

/// 解析 challenge.request（base64url JSON）→ SessionRequest。
fn decode_challenge_request_recipient(
    request: &mpp::protocol::core::Base64UrlJson,
) -> Result<Address, SaApiError> {
    // Base64UrlJson decoded as serde_json::Value
    let value = request
        .decode_value()
        .map_err(|e| SaApiError::new(70000, format!("decode challenge request: {e}")))?;
    let recipient = value
        .get("recipient")
        .and_then(|r| r.as_str())
        .ok_or_else(|| SaApiError::new(70000, "challenge.request missing recipient"))?;
    parse_address(recipient)
}

/// 从 method_details JSON 解出 SessionMethodDetails。
fn decode_method_details(
    method_details: Option<&serde_json::Value>,
) -> Result<SessionMethodDetails, SaApiError> {
    let v = method_details.ok_or_else(|| SaApiError::new(8000, "method_details not configured"))?;
    serde_json::from_value(v.clone())
        .map_err(|e| SaApiError::new(70000, format!("invalid method_details: {e}")))
}

// ===================== SessionMethod trait impl =====================

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
        _request: &SessionRequest,
    ) -> impl Future<Output = Result<Receipt, VerificationError>> + Send {
        let credential = credential.clone();
        let challenge_id = credential.challenge.id.clone();
        let this = self.clone();

        async move {
            let action = extract_str(&credential.payload, "action");

            let result: Result<Receipt, SaApiError> = match action {
                ACTION_OPEN => this.handle_open(&credential).await,
                ACTION_TOPUP => this.handle_topup(&credential).await,
                ACTION_VOUCHER => this.handle_voucher(&credential).await,
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
        _receipt: &Receipt,
    ) -> Option<serde_json::Value> {
        // 管理动作（open/topUp/close）回个简单响应；voucher 是内容请求，不回响应体。
        let action = extract_str(&credential.payload, "action");
        if matches!(action, ACTION_OPEN | ACTION_TOPUP | ACTION_CLOSE) {
            let channel_id = extract_str(&credential.payload, "channelId");
            Some(serde_json::json!({
                "action": action,
                "status": "ok",
                "channelId": channel_id,
            }))
        } else {
            None
        }
    }
}

// ===================== Action handlers =====================

impl EvmSessionMethod {
    async fn handle_open(&self, credential: &PaymentCredential) -> Result<Receipt, SaApiError> {
        // 1. payee 一致性校验（Q18 歧义 1）：challenge.recipient == signer.address()
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

        // 2. 透传 credential 给 SA API
        let credential_json = serde_json::to_value(credential)
            .map_err(|e| SaApiError::new(8000, format!("serialize credential: {e}")))?;
        let receipt = self.sa_client.session_open(&credential_json).await?;

        // 3. 解析 method_details 拿 chain_id / escrow_contract / min_voucher_delta
        let method_details = decode_method_details(self.method_details.as_ref())?;

        // 4. SDK 本地验证初始 voucher（如有）
        //    DRAFT 2 之后 SA API 不再验初始 voucher，但 SDK 自己验。
        //    transaction 模式：payload.signature 是 EIP-3009（不是 voucher）
        //    hash 模式：payload.signature 是初始 voucher 签名（按 §8.3）
        let payload = &credential.payload;
        let payload_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let initial_voucher_sig = if payload_type == "hash" {
            parse_optional_hex_bytes(payload.get("signature"))?
        } else {
            // transaction 模式：voucherSignature 字段可能存在（旧版 doc）
            // DRAFT 2 删除了 voucherSignature 字段，但 SDK 解析时容错读取
            parse_optional_hex_bytes(payload.get("voucherSignature"))?
        };

        // 5. 组装 ChannelRecord
        let channel_id = receipt.channel_id.clone();
        let channel_id_b256 = parse_b256(&channel_id)?;
        let escrow_contract = parse_address(&method_details.escrow_contract)?;
        let payer = parse_address(extract_str(
            payload.get("authorization").unwrap_or(&serde_json::Value::Null),
            "from",
        ))?;
        // authorizedSigner: address(0) 或缺失 → 回落 payer
        let raw_signer = payload
            .get("authorizedSigner")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<Address>())
            .transpose()
            .map_err(|e| SaApiError::new(70000, format!("invalid authorizedSigner: {e}")))?;
        let authorized_signer = match raw_signer {
            Some(a) if a != Address::ZERO => a,
            _ => payer,
        };
        let deposit = parse_u128_str(extract_str(
            payload.get("authorization").unwrap_or(&serde_json::Value::Null),
            "value",
        ))?;
        let cumulative_amount = parse_u128_default_zero(payload.get("cumulativeAmount"))?;

        // 6. 如果有初始 voucher 签名，本地验签
        if let Some(sig) = initial_voucher_sig.as_ref() {
            verify_voucher(
                escrow_contract,
                method_details.chain_id,
                channel_id_b256,
                cumulative_amount,
                sig,
                authorized_signer,
            )
            .map_err(|e| SaApiError::new(70004, format!("initial voucher: {e}")))?;
        }

        let min_voucher_delta = method_details
            .min_voucher_delta
            .as_deref()
            .map(parse_u128_str)
            .transpose()?;

        // 7. 写 store
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
        };
        self.store.put(record).await;

        Ok(Receipt::success(
            "evm",
            receipt.reference.clone().unwrap_or(channel_id),
        ))
    }

    async fn handle_topup(&self, credential: &PaymentCredential) -> Result<Receipt, SaApiError> {
        let credential_json = serde_json::to_value(credential)
            .map_err(|e| SaApiError::new(8000, format!("serialize credential: {e}")))?;
        let receipt = self.sa_client.session_top_up(&credential_json).await?;

        // 累加 deposit
        let additional = parse_u128_str(extract_str(&credential.payload, "additionalDeposit"))?;
        let updater: ChannelUpdater = Box::new(move |r: &mut ChannelRecord| {
            r.deposit = r
                .deposit
                .checked_add(additional)
                .ok_or_else(|| SaApiError::new(8000, "deposit overflow"))?;
            Ok(())
        });
        // 如果本地没有 record（比如 SDK 重启后 topUp 直接来），update 会返 70010
        // —— 上层 SA API 已成功，这里只记 warning 不阻断（虽然本地状态会不一致，
        // 但下次 submit_voucher 也会因 miss 而报错引导）。
        if let Err(e) = self.store.update(&receipt.channel_id, updater).await {
            tracing::warn!(channel_id = %receipt.channel_id, error = %e, "topup local update skipped");
        }
        Ok(Receipt::success(
            "evm",
            receipt.reference.clone().unwrap_or(receipt.channel_id),
        ))
    }

    async fn handle_voucher(&self, credential: &PaymentCredential) -> Result<Receipt, SaApiError> {
        let payload = &credential.payload;
        let channel_id = extract_str(payload, "channelId");
        let cum = parse_u128_str(extract_str(payload, "cumulativeAmount"))?;
        let sig = parse_optional_hex_bytes(payload.get("signature"))?
            .ok_or_else(|| SaApiError::new(70000, "voucher missing signature"))?;
        self.submit_voucher(channel_id, cum, sig).await?;
        Ok(Receipt::success("evm", channel_id.to_string()))
    }

    async fn handle_close(&self, credential: &PaymentCredential) -> Result<Receipt, SaApiError> {
        let payload = &credential.payload;
        let channel_id = extract_str(payload, "channelId");
        let cum = parse_u128_str(extract_str(payload, "cumulativeAmount"))?;
        let voucher_sig = parse_optional_hex_bytes(payload.get("signature"))?;

        // payer 提供的最终 voucher 必须先在本地验签（B-1 方案）
        if let Some(sig) = voucher_sig.as_ref() {
            let channel_id_b256 = parse_b256(channel_id)?;
            let channel = self
                .store
                .get(channel_id)
                .await
                .ok_or_else(|| SaApiError::new(70010, "channel not found in local store"))?;
            verify_voucher(
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

// ===================== Tests =====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eip712::Voucher;
    use alloy_primitives::{address, b256};
    use alloy_signer::SignerSync;
    use alloy_sol_types::SolStruct;
    use async_trait::async_trait;
    use mpp::protocol::core::{Base64UrlJson, ChallengeEcho};
    use std::sync::Mutex as StdMutex;

    fn fixture_signer() -> PrivateKeySigner {
        "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318"
            .parse()
            .unwrap()
    }

    /// 用 fixture_signer 在 test channel/escrow 下签 voucher。
    fn fixture_voucher_sig(cum: u128) -> Bytes {
        let signer = fixture_signer();
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
        let channel_id = b256!("6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f");
        let domain = crate::eip712::build_domain(196, escrow);
        let voucher = Voucher {
            channelId: channel_id,
            cumulativeAmount: cum,
        };
        let digest = voucher.eip712_signing_hash(&domain);
        let sig = signer.sign_hash_sync(&digest).unwrap();
        Bytes::from(sig.as_bytes().to_vec())
    }

    fn fixture_channel_record() -> ChannelRecord {
        let signer = fixture_signer();
        ChannelRecord {
            channel_id: "0x6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f"
                .into(),
            chain_id: 196,
            escrow_contract: address!("eb18025208061781a287fFc2c1F31C03A24a24c0"),
            payer: signer.address(),
            payee: signer.address(),
            authorized_signer: signer.address(),
            deposit: 1_000_000,
            highest_voucher_amount: 0,
            highest_voucher_signature: None,
            min_voucher_delta: None,
        }
    }

    #[derive(Default)]
    struct StubSa {
        next_error: StdMutex<Option<SaApiError>>,
    }

    #[async_trait]
    impl SaApiClient for StubSa {
        async fn charge_settle(
            &self,
            _: &serde_json::Value,
        ) -> Result<crate::types::ChargeReceipt, SaApiError> {
            unreachable!()
        }
        async fn charge_verify_hash(
            &self,
            _: &serde_json::Value,
        ) -> Result<crate::types::ChargeReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_open(&self, _: &serde_json::Value) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_top_up(
            &self,
            _: &serde_json::Value,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_settle(
            &self,
            _: &SettleRequestPayload,
        ) -> Result<SessionReceipt, SaApiError> {
            if let Some(e) = self.next_error.lock().unwrap().take() {
                return Err(e);
            }
            Ok(SessionReceipt {
                method: "evm".into(),
                intent: "session".into(),
                status: "success".into(),
                timestamp: "2026-04-01T12:00:00Z".into(),
                chain_id: 196,
                channel_id: "0xabc".into(),
                reference: Some("0xtx".into()),
                deposit: Some("1000".into()),
                challenge_id: None,
                accepted_cumulative: None,
                spent: None,
                confirmations: None,
                units: None,
            })
        }
        async fn session_close(
            &self,
            _: &CloseRequestPayload,
        ) -> Result<SessionReceipt, SaApiError> {
            Ok(SessionReceipt {
                method: "evm".into(),
                intent: "session".into(),
                status: "success".into(),
                timestamp: "2026-04-01T12:00:00Z".into(),
                chain_id: 196,
                channel_id: "0xabc".into(),
                reference: Some("0xclose_tx".into()),
                deposit: Some("0".into()),
                challenge_id: None,
                accepted_cumulative: None,
                spent: None,
                confirmations: None,
                units: None,
            })
        }
        async fn session_status(&self, _: &str) -> Result<ChannelStatus, SaApiError> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn submit_voucher_round_trip() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        // 先把 fixture record 塞进 store
        let record = fixture_channel_record();
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let cum = 100u128;
        let sig = fixture_voucher_sig(cum);
        method.submit_voucher(&cid, cum, sig.clone()).await.unwrap();

        let r = method.store.get(&cid).await.unwrap();
        assert_eq!(r.highest_voucher_amount, 100);
        assert_eq!(r.highest_voucher_signature, Some(sig));
    }

    #[tokio::test]
    async fn submit_voucher_strict_increasing() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let record = fixture_channel_record();
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let sig100 = fixture_voucher_sig(100);
        let sig50 = fixture_voucher_sig(50);

        method.submit_voucher(&cid, 100, sig100.clone()).await.unwrap();

        // 同 cum + 同 sig → 幂等成功
        method.submit_voucher(&cid, 100, sig100.clone()).await.unwrap();

        // 较低 cum 但合法签名 → 70000 not increasing
        let err = method.submit_voucher(&cid, 50, sig50).await.unwrap_err();
        assert_eq!(err.code, 70000);
    }

    #[tokio::test]
    async fn submit_voucher_amount_exceeds_deposit() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let mut record = fixture_channel_record();
        record.deposit = 1000;
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let sig = fixture_voucher_sig(2000);
        let err = method.submit_voucher(&cid, 2000, sig).await.unwrap_err();
        assert_eq!(err.code, 70012);
    }

    #[tokio::test]
    async fn submit_voucher_missing_record_returns_70010() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let sig = fixture_voucher_sig(1);
        let err = method
            .submit_voucher("0xnonexistent", 1, sig)
            .await
            .unwrap_err();
        assert_eq!(err.code, 70010);
    }

    #[tokio::test]
    async fn settle_with_authorization_uses_local_highest() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let mut record = fixture_channel_record();
        record.highest_voucher_amount = 250;
        record.highest_voucher_signature = Some(Bytes::from(vec![0x01; 65]));
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let receipt = method.settle_with_authorization(&cid).await.unwrap();
        assert_eq!(receipt.reference.as_deref(), Some("0xtx"));
    }

    #[tokio::test]
    async fn settle_without_signer_fails_8000() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()));
        // 没注入 signer
        let err = method
            .settle_with_authorization("0xabc")
            .await
            .unwrap_err();
        assert_eq!(err.code, 8000);
    }

    #[tokio::test]
    async fn close_removes_channel_record() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let mut record = fixture_channel_record();
        record.highest_voucher_amount = 300;
        record.highest_voucher_signature = Some(Bytes::from(vec![0x02; 65]));
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        method
            .close_with_authorization(&cid, None, None)
            .await
            .unwrap();
        assert!(method.store.get(&cid).await.is_none());
    }

    fn dummy_request() -> SessionRequest {
        SessionRequest {
            amount: "100".into(),
            currency: "0xToken".into(),
            decimals: None,
            recipient: Some("0xPayee".into()),
            unit_type: None,
            suggested_deposit: None,
            method_details: None,
        }
    }

    #[tokio::test]
    async fn unknown_action_errors() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());
        let cred = PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-1".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "session".into(),
                request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
                expires: None,
                digest: None,
                opaque: None,
            },
            source: None,
            payload: serde_json::json!({"action": "dance", "channelId": "0xa"}),
        };
        let err = method
            .verify_session(&cred, &dummy_request())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown session action"));
    }
}
