//! EvmSessionMethod —— SessionMethod 的 OKX SA API 实现。
//!
//! ## 设计要点
//!
//! 1. **Voucher 本地化**：ACTION_VOUCHER 在 SDK 本地验签 + 写入本地 Store,
//!    不转发 SA API。每次 voucher action 都会走 `deduct_from_channel` 一次;
//!    字节级幂等仅跳过验签 + `highest_voucher_*` 更新,**deduct 不跳**。
//!    Client 可以一次签大额 voucher,多次重发同一份字节复用余额,spent
//!    持续累加直到顶到 highest 才返 70015 让 client 升级。
//! 2. **Settle / Close 商户主动**：商户调 `settle_with_authorization()` /
//!    `close_with_authorization()`,SDK 本地签 SettleAuth/CloseAuth 后组装
//!    扁平 payload(无 challenge wrapper)→ POST SA API。
//! 3. **无 idle timer**：商户自管关闭时机。
//! 4. **Payee 一致性校验**：ACTION_OPEN 时校验
//!    `signer.address() == challenge.recipient`,不一致拒绝写 store。
//! 5. **持久化职责在商户**：`store.get` miss 时直接返 70010,SDK 不调
//!    `session_status` 自动回源 —— 回源能拿到的字段只是子集(没有
//!    `cumulativeAmount` 和 `highest_voucher_signature`),无法重建 voucher 状态。
//!    商户跨进程稳定运行需自行实现持久化 [`SessionStore`](crate::SessionStore)
//!    (SQLite / Redis 等),SDK 仅提供进程内 [`InMemorySessionStore`] 默认实现。
//!
//! Signer 注入:`with_signer` 接受任何实现 [`alloy::signers::Signer`](Signer)
//! 的类型 —— 本地私钥(`PrivateKeySigner`)、AWS KMS、Ledger / Trezor 硬件钱包、
//! WalletConnect 桥接、或商户自定义 wrapper 都可以,内部存为
//! `Arc<dyn Signer + Send + Sync>` 共享。

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex as StdMutex};

use alloy_primitives::{hex, Address, B256, Bytes, U256};
use alloy_signer::Signer;
use mpp::protocol::core::{PaymentCredential, Receipt};
use mpp::protocol::intents::SessionRequest;
use mpp::protocol::traits::{SessionMethod, VerificationError};
use tokio::sync::Mutex as AsyncMutex;

use crate::eip712::{
    sign_close_authorization, sign_settle_authorization, verify_voucher, DomainMeta,
};
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

/// Default deadline = `U256::MAX` —— 等同永不过期。
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
    /// `dyn Signer` 让商户接 KMS / Ledger / WalletConnect 等远程签名,而不止
    /// 本地 PrivateKeySigner。任何实现 `alloy::signers::Signer` 的类型都可注入。
    signer: Option<Arc<dyn Signer + Send + Sync>>,
    /// `signer.address()`，缓存避免反复算。`None` 表示未注入 signer。
    payee_address: Option<Address>,
    nonce_provider: Arc<dyn NonceProvider>,
    /// Settle / Close Authorization 签名的 deadline。默认 `U256::MAX`，可配。
    default_deadline: U256,
    /// Per-channelId 互斥锁。
    channel_locks: Arc<ChannelLocks>,
    /// 临时存储 voucher action 的扣费结果（spent / units），由 respond() 读取写入响应。
    /// 键为 challenge_id（与 verify_session / respond 入参的 credential.challenge.id 一致）。
    /// `respond()` 读取后立刻移除，避免无限增长。
    voucher_deduct_results: Arc<StdMutex<HashMap<String, (u128, u64)>>>,
    /// EIP-712 domain `name` / `version` 元数据。默认 OKX EvmPaymentChannel
    /// 标准值,商户 fork 合约改了 domain 时通过 `with_domain_meta(...)` 覆盖。
    domain_meta: DomainMeta,
}

impl EvmSessionMethod {
    /// 用默认内存 store 创建。
    pub fn new(sa_client: Arc<dyn SaApiClient>) -> Self {
        Self::with_store(sa_client, Arc::new(InMemorySessionStore::new()))
    }

    /// 注入自定义 [`SessionStore`]。SDK 默认 [`InMemorySessionStore`] 是
    /// 进程内 HashMap,重启即丢,只适合开发 / 测试。生产部署必须接持久化
    /// store —— 商户实现 [`SessionStore`] 4 个 async 方法即可挂任意后端
    /// (SQLite / Redis / Postgres / DynamoDB ...)。
    ///
    /// `update` 是**原子闭包契约**(事务 / `WATCH` / `SELECT FOR UPDATE` 等),
    /// 同 channel 并发由 SDK 内部锁串行化,跨进程并发由商户 store 自带。
    ///
    /// SQLite / Redis / Postgres / decorator 等完整接入示例见
    /// [README → Custom store integration](https://github.com/okx/payments/blob/main/rust/mpp/README.md#custom-store-integration)。
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
            voucher_deduct_results: Arc::new(StdMutex::new(HashMap::new())),
            domain_meta: DomainMeta::default(),
        }
    }

    /// 注入 signer。接受任何实现 [`alloy::signers::Signer`](Signer) 的类型 ——
    /// 本地私钥、AWS KMS、Ledger / Trezor 硬件钱包、WalletConnect 桥接、
    /// 商户自封装远程签名服务等。内部存为 `Arc<dyn Signer + Send + Sync>`。
    ///
    /// ```ignore
    /// use alloy_signer_local::PrivateKeySigner;
    ///
    /// let signer: PrivateKeySigner = std::env::var("MERCHANT_PK")?.parse()?;
    /// let method = EvmSessionMethod::new(sa_client).with_signer(signer);
    /// ```
    ///
    /// KMS / Ledger / 自定义 wrapper 等远程签名场景见
    /// [README → Custom signer integration](https://github.com/okx/payments/blob/main/rust/mpp/README.md#custom-signer-integration)。
    pub fn with_signer<S: Signer + Send + Sync + 'static>(mut self, signer: S) -> Self {
        self.payee_address = Some(signer.address());
        self.signer = Some(Arc::new(signer));
        self
    }

    /// 启动期 fast-fail 校验:确保已注入的 signer 地址等于商户配置的 payee 地址。
    /// 不一致直接报 8000,避免商户配置错了之后等到第一个 open 请求才发现
    /// `challenge.recipient != signer.address()` 拒绝。
    ///
    /// 链式用法:
    /// ```ignore
    /// let method = EvmSessionMethod::new(sa)
    ///     .with_signer(signer)
    ///     .verify_payee(expected_payee_addr)?;
    /// ```
    pub fn verify_payee(self, expected: Address) -> Result<Self, SaApiError> {
        match self.payee_address {
            Some(actual) if actual == expected => Ok(self),
            Some(actual) => Err(SaApiError::new(
                8000,
                format!(
                    "payee mismatch: signer.address={actual:#x} but expected={expected:#x}; \
                     SDK signer must be merchant's receiving address"
                ),
            )),
            None => Err(SaApiError::new(
                8000,
                "no signer configured (call .with_signer before .verify_payee)",
            )),
        }
    }

    /// 注入自定义 nonce 分配器（默认 [`UuidNonceProvider`]）。
    pub fn with_nonce_provider(mut self, p: Arc<dyn NonceProvider>) -> Self {
        self.nonce_provider = p;
        self
    }

    /// 自定义 EIP-712 domain 的 `name` / `version`。默认 OKX 标准值
    /// (`"EVM Payment Channel"` / `"1"`)。
    ///
    /// 商户 fork 合约时若 domain 改了 `name` 或 `version`,**必须**用本方法
    /// 设置成跟合约部署时完全一致的值,否则所有 voucher 验签 / SettleAuth /
    /// CloseAuth 签名都会跟链上对不上。
    pub fn with_domain_meta(
        mut self,
        name: impl Into<std::borrow::Cow<'static, str>>,
        version: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Self {
        self.domain_meta = DomainMeta::new(name, version);
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
    pub async fn status(&self, channel_id: &str) -> Result<ChannelStatus, SaApiError> {
        self.sa_client.session_status(channel_id).await
    }

    // ===================== submit_voucher（本地处理，9 步守卫）=====================

    /// 本地处理 voucher：验签 + 原子更新 highest_voucher。**业务层不直接调**，
    /// 由 [`SessionMethod::verify_session`] 的 `ACTION_VOUCHER` 分支触发。
    ///
    /// 字节级幂等(channelId / cum / signature 三元组完全相等)只跳过验签和
    /// `highest_voucher_*` 的更新 —— 调用方仍会调 `deduct_from_channel`
    /// 扣本次费用，与 mppx / OKX TS Session 行为一致。这样 client 一次签
    /// 大额 voucher 后多次复用同一份字节，server spent 持续上升，直到顶到
    /// highest 才返 70015 让 client 升级 cum 重签。
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

        // D. 字节级幂等（cum + signature 都精确相等 → 跳过验签 + 跳过升 highest）
        if cumulative_amount <= channel.highest_voucher_amount {
            let exact_replay = channel.highest_voucher_signature.as_ref().is_some_and(|s| {
                s == &signature && cumulative_amount == channel.highest_voucher_amount
            });
            if exact_replay {
                tracing::debug!(
                    channel_id,
                    cumulative_amount,
                    "voucher byte-level replay — skipping verify+highest update, deduct still applies"
                );
                return Ok(());
            }
            // 协议表(70000-70014)无独立"not increasing"码;
            // delta ≤ 0 是 70013 voucher_delta_too_small 的极端形式,统一归到 70013。
            return Err(SaApiError::new(
                70013,
                "voucher cumulative not strictly increasing (delta <= 0)",
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
            &self.domain_meta,
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

    // ===================== deduct_from_channel（计费扣费）=====================

    /// 原子扣费：`available = highest_voucher_amount - spent`，若 `available <
    /// amount` 返回 `70015 insufficient-balance`，否则 `spent += amount;
    /// units += 1`。返回更新后的 [`ChannelRecord`] 快照。
    ///
    /// 与 TS Session.ts `deduct(challengeId, amount)` 行为对齐。商户业务可在
    /// `verify_session` 之外手动调用（例如基于实际服务消耗的非线性计费）。
    /// `voucher` action 内部已自动调用一次（金额取自 `SessionRequest.amount`）。
    pub async fn deduct_from_channel(
        &self,
        channel_id: &str,
        amount: u128,
    ) -> Result<ChannelRecord, SaApiError> {
        let _guard = self.channel_locks.lock(channel_id).await;
        let updater: ChannelUpdater = Box::new(move |c: &mut ChannelRecord| {
            let available = c
                .highest_voucher_amount
                .checked_sub(c.spent)
                .ok_or_else(|| SaApiError::new(8000, "spent exceeds highest voucher"))?;
            if available < amount {
                return Err(SaApiError::new(
                    70015,
                    format!(
                        "insufficient balance: requested {amount} but available {available}"
                    ),
                ));
            }
            c.spent = c
                .spent
                .checked_add(amount)
                .ok_or_else(|| SaApiError::new(8000, "spent overflow"))?;
            c.units = c
                .units
                .checked_add(1)
                .ok_or_else(|| SaApiError::new(8000, "units overflow"))?;
            Ok(())
        });
        self.store.update(channel_id, updater).await
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
            &self.domain_meta,
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
        // waiver 分支:无 voucher 时上送空串。Server 同时认 cum ≤ settledOnChain
        // 或 voucherSignature == "" 触发 waiver,SDK 只透传调用方意图,不做本地判断。
        let voucher_sig_bytes = provided_voucher_sig
            .or_else(|| channel.highest_voucher_signature.clone());

        let channel_id_b256 = parse_b256(channel_id)?;

        // 当 voucher 是 payer 通过 ACTION_CLOSE 提供的 / 或者本地 highest 但
        // 已经被 submit_voucher 验过 — 这里不再重复验。验签责任在 ACTION_CLOSE
        // 入口完成。

        let nonce = self.nonce_provider.allocate(payee, channel_id_b256).await?;
        let deadline = self.default_deadline;

        let signed = sign_close_authorization(
            &self.domain_meta,
            signer.as_ref(),
            channel.escrow_contract,
            channel.chain_id,
            channel_id_b256,
            cumulative,
            nonce,
            deadline,
        )
        .await?;

        let voucher_signature = match voucher_sig_bytes {
            Some(b) => hex_with_prefix(&b),
            None => String::new(), // waiver 路径:server 见空串走 waiver
        };
        let payload = CloseRequestPayload {
            action: Some("close".into()),
            channel_id: channel_id.to_string(),
            cumulative_amount: cumulative.to_string(),
            voucher_signature,
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

/// 严格按 spec 解析 `did:pkh:eip155:<chainId>:<address>` 格式 DID,返回末段地址。
///
/// 校验项(对齐 mpp-rs `parse_proof_source`):
/// - 前缀必须 `did:pkh:eip155:`(method 必须 pkh,namespace 必须 eip155)
/// - chainId 段必须能 parse 成 u64,且无前导零(`"0"` 本身合法,`"01"` 拒)
/// - 地址段不能再含冒号(防伪造扩展)
/// - 地址必须是合法 0x40hex
/// - 额外:解析出来的 chainId 必须等于 `expected_chain_id`(防客户端把 mainnet
///   DID 拿到 testnet 来用之类的混链事故)
///
/// 任何不通过都返 `70000 invalid source DID`。
fn parse_did_pkh_eip155(did: &str, expected_chain_id: u64) -> Result<Address, SaApiError> {
    let rest = did
        .strip_prefix("did:pkh:eip155:")
        .ok_or_else(|| SaApiError::new(70000, format!("source DID must start with did:pkh:eip155: ({did})")))?;
    // 用 split_once 而不是 rsplit,确保地址段没有冗余冒号
    let (chain_id_str, address_str) = rest
        .split_once(':')
        .ok_or_else(|| SaApiError::new(70000, format!("source DID missing address segment ({did})")))?;
    // 拒前导零(只有 "0" 本身合法)
    if chain_id_str.len() > 1 && chain_id_str.starts_with('0') {
        return Err(SaApiError::new(70000, format!("source DID chainId has leading zero: {chain_id_str}")));
    }
    let chain_id: u64 = chain_id_str
        .parse()
        .map_err(|e| SaApiError::new(70000, format!("invalid chainId in source DID: {e}")))?;
    if chain_id != expected_chain_id {
        return Err(SaApiError::new(
            70000,
            format!("source DID chainId {chain_id} != expected {expected_chain_id}"),
        ));
    }
    // 地址段不能再含冒号
    if address_str.contains(':') {
        return Err(SaApiError::new(
            70000,
            format!("source DID address segment has invalid chars: {address_str}"),
        ));
    }
    parse_address(address_str)
}

/// 按 credential `payload.type` 分支提取 (payer, authorized_signer)。
///
/// - **transaction 模式**:`payer = payload.authorization.from`。SDK 不与 `source` DID
///   做交叉校验 — `source` 在 transaction 模式下是可选的辅助字段,authorization.from
///   是签名捆绑的权威值。
/// - **hash 模式**:`payer = parse_did_pkh_eip155(source, chain_id)`(spec 强制 hash 模式必须有 source)
/// - **authorized_signer**:优先取 `payload.authorizedSigner`(非 0x0),否则 fallback 到 payer。
///   client 显式发了 `authorizedSigner == payer`(冗余但合规)→ 静默接受,跟 mpp-rs 行为一致。
///
/// 出错全部映射 70000 invalid_payload。
fn extract_payer_and_signer(
    payload: &serde_json::Value,
    source: Option<&str>,
    chain_id: u64,
) -> Result<(Address, Address), SaApiError> {
    let payload_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let payer = match payload_type {
        "transaction" => parse_address(extract_str(
            payload.get("authorization").unwrap_or(&serde_json::Value::Null),
            "from",
        ))?,
        "hash" => {
            let did = source
                .filter(|s| !s.is_empty())
                .ok_or_else(|| SaApiError::new(70000, "hash mode credential missing source"))?;
            parse_did_pkh_eip155(did, chain_id)?
        }
        other => {
            return Err(SaApiError::new(
                70000,
                format!("unsupported payload type {other:?} (expected transaction|hash)"),
            ))
        }
    };

    // authorizedSigner: 显式非 0x0 → 用它;0x0 / 缺失 / 空串 → fallback payer
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
    Ok((payer, authorized_signer))
}

/// 转发 credential 给 SA `/session/open` 时,把 SDK-only 字段从 payload 里剥掉。
///
/// `cumulativeAmount` 和 `voucherSignature` 是 client 给 SDK 用作初始 voucher
/// baseline 的,SA spec 不列这两字段,所以转发前必须清掉(避免 SA strict-schema
/// reject + 减少冗余传输)。`challenge` / `source` 顶层字段保留(open 接口
/// 仍需 challenge)。
fn strip_sdk_only_open_fields(
    credential: &PaymentCredential,
) -> Result<serde_json::Value, SaApiError> {
    let mut credential_json = serde_json::to_value(credential)
        .map_err(|e| SaApiError::new(8000, format!("serialize credential: {e}")))?;
    if let Some(payload_obj) = credential_json
        .get_mut("payload")
        .and_then(|v| v.as_object_mut())
    {
        payload_obj.remove("cumulativeAmount");
        payload_obj.remove("voucherSignature");
    }
    Ok(credential_json)
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
        request: &SessionRequest,
    ) -> impl Future<Output = Result<Receipt, VerificationError>> + Send {
        let credential = credential.clone();
        let request = request.clone();
        let challenge_id = credential.challenge.id.clone();
        let this = self.clone();

        async move {
            let action = extract_str(&credential.payload, "action");

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
        // 管理动作(open/topUp/close)回个简单响应;voucher action 返扣费快照(spent/units)
        // reference 字段是 SA API 返的链上 tx hash 或 channel_id(fallback)。
        let action = extract_str(&credential.payload, "action");
        let channel_id = extract_str(&credential.payload, "channelId");
        match action {
            ACTION_OPEN | ACTION_TOPUP | ACTION_CLOSE => Some(serde_json::json!({
                "action":     action,
                "status":     "ok",
                "channelId":  channel_id,
                "reference":  receipt.reference,
            })),
            ACTION_VOUCHER => {
                // 取出 handle_voucher 写入的扣费结果，读完即移除避免无限增长
                let challenge_id = &credential.challenge.id;
                let deduct = self
                    .voucher_deduct_results
                    .lock()
                    .unwrap()
                    .remove(challenge_id);
                deduct.map(|(spent, units)| {
                    serde_json::json!({
                        "action":    action,
                        "status":    "ok",
                        "channelId": channel_id,
                        "spent":     spent.to_string(),
                        "units":     units,
                    })
                })
            }
            _ => None,
        }
    }
}

// ===================== Action handlers =====================

impl EvmSessionMethod {
    async fn handle_open(&self, credential: &PaymentCredential) -> Result<Receipt, SaApiError> {
        // 1. payee 一致性校验:challenge.recipient == signer.address()
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

        // 2. 解析 method_details 拿 chain_id / escrow_contract / min_voucher_delta
        let method_details = decode_method_details(self.method_details.as_ref())?;

        // 3. 解析 credential 里 SDK 自用的字段(SA 不消费,转发前会 strip 掉):
        //    - cumulativeAmount(初始 voucher 金额,默认 0)
        //    - voucherSignature(初始 voucher EIP-712 签名)
        //    两种模式统一用 voucherSignature key,不再区分 hash 模式 fallback 到 signature。
        let payload = &credential.payload;
        let payload_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let initial_voucher_sig = parse_optional_hex_bytes(payload.get("voucherSignature"))?;
        let cumulative_amount = parse_u128_default_zero(payload.get("cumulativeAmount"))?;

        // 4. 解析 channel_id(从 client 的 payload,不依赖 SA 返回 — fail-fast 用)
        let channel_id_str = extract_str(payload, "channelId");
        if channel_id_str.is_empty() {
            return Err(SaApiError::new(70000, "open payload missing channelId"));
        }
        let channel_id_b256 = parse_b256(channel_id_str)?;
        let escrow_contract = parse_address(&method_details.escrow_contract)?;

        // 5. 解析 payer / authorized_signer
        // - transaction: payer = payload.authorization.from
        // - hash:        payer = parse(source DID 末段地址,严格 did:pkh:eip155 格式)
        let (payer, authorized_signer) = extract_payer_and_signer(
            payload,
            credential.source.as_deref(),
            method_details.chain_id,
        )?;

        // 6. ★ 本地 fail-fast 校验初始 voucher 签名(转 SA 调链前先验)
        //    transaction 模式可省 gas:验签失败就不去 SA broadcast open tx。
        //    hash 模式 client 已自己上链,gas 已花,但仍先验签语义更清晰。
        if let Some(sig) = initial_voucher_sig.as_ref() {
            // 6a. transaction 模式可以本地校验 cum 上限:
            //     cumulativeAmount 不能超过 client 自报的 authorization.value(deposit)
            //     hash 模式 deposit 要等 SA 调用后从 receipt 拿,本地校验放后面
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
            // 6b. EIP-712 ecrecover 严格校验
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

        // 7. 转发 credential 给 SA。SDK-only 字段(cumulativeAmount / voucherSignature)
        //    要 strip 掉再发 — SA spec 不列这两字段,我们只在 SDK 内部用作 baseline。
        let credential_for_sa = strip_sdk_only_open_fields(credential)?;
        let receipt = self.sa_client.session_open(&credential_for_sa).await?;

        // 8. SA 链上 sanity:返回的 channelId 必须跟 client payload 自报的一致
        if !receipt.channel_id.eq_ignore_ascii_case(channel_id_str) {
            return Err(SaApiError::new(
                8000,
                format!(
                    "channelId mismatch: client claimed {} but SA returned {}",
                    channel_id_str, receipt.channel_id
                ),
            ));
        }

        // 9. deposit 来源(到这里两种模式都能拿到):
        //    - transaction:client payload.authorization.value(已经在 6a 用过)
        //    - hash:        SA receipt.deposit(链上权威值)
        let deposit = if payload_type == "hash" {
            let dep_str = receipt
                .deposit
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| SaApiError::new(
                    70000,
                    "hash mode: SA session_open response missing deposit",
                ))?;
            parse_u128_str(dep_str)?
        } else {
            parse_u128_str(extract_str(
                payload.get("authorization").unwrap_or(&serde_json::Value::Null),
                "value",
            ))?
        };

        // 9b. hash 模式补一次 cum vs deposit 校验(transaction 已在 6a 校验过)
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

        // 10. 写 store
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

    async fn handle_topup(&self, credential: &PaymentCredential) -> Result<Receipt, SaApiError> {
        // session/topUp 不需要 challenge,只发 { source, payload }。
        let mut body = serde_json::json!({ "payload": credential.payload });
        if let Some(s) = credential.source.as_deref() {
            body["source"] = serde_json::Value::String(s.to_string());
        }
        let receipt = self.sa_client.session_top_up(&body).await?;

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

    async fn handle_voucher(
        &self,
        credential: &PaymentCredential,
        request: &SessionRequest,
    ) -> Result<Receipt, SaApiError> {
        let payload = &credential.payload;
        let channel_id = extract_str(payload, "channelId");
        let cum = parse_u128_str(extract_str(payload, "cumulativeAmount"))?;
        let sig = parse_optional_hex_bytes(payload.get("signature"))?
            .ok_or_else(|| SaApiError::new(70000, "voucher missing signature"))?;
        // 字节级重放只跳过验签 + highest 更新；deduct 仍照扣。对齐 mppx /
        // OKX TS Session：client 一次签大额 voucher 后可重复发送同一份字节复用余额。
        // 网络重传双扣的保护建议放到 challenge.id 维度做幂等（TS 也未做，留作后续）。
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

// ===================== Tests =====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eip712::Voucher;
    use alloy_primitives::{address, b256};
    use alloy_signer::SignerSync;
    use alloy_signer_local::PrivateKeySigner;
    use alloy_sol_types::SolStruct;
    use async_trait::async_trait;
    use mpp::protocol::core::{Base64UrlJson, ChallengeEcho};
    use std::sync::Mutex as StdMutex;

    fn fixture_signer() -> PrivateKeySigner {
        "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318"
            .parse()
            .unwrap()
    }

    // ===================== parse_did_pkh_eip155 =====================

    #[test]
    fn parse_did_pkh_happy_path_returns_address() {
        let did = "did:pkh:eip155:196:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3";
        let addr = parse_did_pkh_eip155(did, 196).unwrap();
        assert_eq!(addr, address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3"));
    }

    #[test]
    fn parse_did_pkh_chain_id_zero_is_valid() {
        let did = "did:pkh:eip155:0:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3";
        let addr = parse_did_pkh_eip155(did, 0).unwrap();
        assert_eq!(addr, address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3"));
    }

    #[test]
    fn parse_did_pkh_wrong_prefix_rejected() {
        // did:ethr:... 不是 pkh
        let did = "did:ethr:eip155:196:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3";
        let err = parse_did_pkh_eip155(did, 196).unwrap_err();
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("did:pkh:eip155:"));
    }

    #[test]
    fn parse_did_pkh_chain_id_leading_zero_rejected() {
        let did = "did:pkh:eip155:0196:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3";
        let err = parse_did_pkh_eip155(did, 196).unwrap_err();
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("leading zero"));
    }

    #[test]
    fn parse_did_pkh_extra_colon_in_address_rejected() {
        // 地址段含冒号 → 拒(防伪造扩展)
        let did = "did:pkh:eip155:196:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3:bonus";
        let err = parse_did_pkh_eip155(did, 196).unwrap_err();
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("invalid chars"));
    }

    #[test]
    fn parse_did_pkh_wrong_chain_id_rejected() {
        let did = "did:pkh:eip155:1:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3";
        let err = parse_did_pkh_eip155(did, 196).unwrap_err();
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("!= expected"));
    }

    #[test]
    fn parse_did_pkh_invalid_address_rejected() {
        let did = "did:pkh:eip155:196:not-an-address";
        let err = parse_did_pkh_eip155(did, 196).unwrap_err();
        assert_eq!(err.code, 70000);
    }

    // ===================== extract_payer_and_signer =====================

    #[test]
    fn extract_transaction_mode_uses_authorization_from() {
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "from": "0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3",
            },
        });
        let (payer, signer) = extract_payer_and_signer(&payload, None, 196).unwrap();
        let expected = address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3");
        assert_eq!(payer, expected);
        assert_eq!(signer, expected); // authorizedSigner 缺失 → fallback payer
    }

    #[test]
    fn extract_transaction_mode_ignores_source() {
        // transaction 模式不交叉校验 source 与 from:即使 source 末段地址
        // 跟 from 不同,SDK 也以 from 为准(authorization.from 是签名捆绑的权威值)。
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "from": "0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3",
            },
        });
        let source = Some("did:pkh:eip155:196:0xaaaabbbbccccddddeeeeffff0000000011112222");
        let (payer, _) = extract_payer_and_signer(&payload, source, 196).unwrap();
        assert_eq!(payer, address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3"));
    }

    #[test]
    fn extract_hash_mode_parses_payer_from_source() {
        let payload = serde_json::json!({
            "type": "hash",
            "channelId": format!("0x{}", "ab".repeat(32)),
            "salt": format!("0x{}", "01".repeat(32)),
            "hash": format!("0x{}", "02".repeat(32)),
        });
        let source = Some("did:pkh:eip155:196:0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3");
        let (payer, signer) = extract_payer_and_signer(&payload, source, 196).unwrap();
        let expected = address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3");
        assert_eq!(payer, expected);
        assert_eq!(signer, expected);
    }

    #[test]
    fn extract_hash_mode_missing_source_returns_70000() {
        let payload = serde_json::json!({ "type": "hash" });
        let err = extract_payer_and_signer(&payload, None, 196).unwrap_err();
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("missing source"));
    }

    #[test]
    fn extract_explicit_authorized_signer_takes_priority() {
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "from": "0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3",
            },
            "authorizedSigner": "0xaaaabbbbccccddddeeeeffff0000000011112222",
        });
        let (payer, signer) = extract_payer_and_signer(&payload, None, 196).unwrap();
        assert_eq!(payer, address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3"));
        assert_eq!(signer, address!("aaaabbbbccccddddeeeeffff0000000011112222"));
    }

    #[test]
    fn extract_authorized_signer_zero_falls_back_to_payer() {
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "from": "0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3",
            },
            "authorizedSigner": "0x0000000000000000000000000000000000000000",
        });
        let (payer, signer) = extract_payer_and_signer(&payload, None, 196).unwrap();
        assert_eq!(payer, signer);
        assert_eq!(payer, address!("76a5a6ef2a5bd42b22de258994ff792d27c08ec3"));
    }

    #[test]
    fn extract_authorized_signer_equals_payer_silently_accepted() {
        // client 显式发了 authorizedSigner == from(冗余但合规)→ 静默接受,不归一化
        let payer_str = "0x76a5a6ef2a5bd42b22de258994ff792d27c08ec3";
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": { "from": payer_str },
            "authorizedSigner": payer_str,
        });
        let (payer, signer) = extract_payer_and_signer(&payload, None, 196).unwrap();
        assert_eq!(payer, signer);
    }

    #[test]
    fn extract_unsupported_type_rejected() {
        let payload = serde_json::json!({ "type": "magic" });
        let err = extract_payer_and_signer(&payload, None, 196).unwrap_err();
        assert_eq!(err.code, 70000);
        assert!(err.msg.contains("unsupported payload type"));
    }

    // ===================== strip_sdk_only_open_fields =====================

    fn fixture_credential_with_initial_voucher(
        payload_type: &str,
    ) -> PaymentCredential {
        PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-strip".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "session".into(),
                request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
                expires: Some("2026-04-29T10:00:00Z".into()),
                digest: None,
                opaque: None,
            },
            source: Some("did:pkh:eip155:196:0xabc".into()),
            payload: serde_json::json!({
                "action": "open",
                "type": payload_type,
                "channelId": "0xchan",
                "salt": "0xsalt",
                "cumulativeAmount": "0",                       // ← SDK-only
                "voucherSignature": format!("0x{}", "ab".repeat(65)), // ← SDK-only
            }),
        }
    }

    #[test]
    fn strip_removes_cumulative_and_voucher_sig_keeps_others() {
        let cred = fixture_credential_with_initial_voucher("transaction");
        let stripped = strip_sdk_only_open_fields(&cred).unwrap();
        let payload = stripped.get("payload").and_then(|v| v.as_object()).unwrap();
        assert!(!payload.contains_key("cumulativeAmount"), "cumulativeAmount must be stripped");
        assert!(!payload.contains_key("voucherSignature"), "voucherSignature must be stripped");
        // 其他字段保留
        assert_eq!(payload.get("action").and_then(|v| v.as_str()), Some("open"));
        assert_eq!(payload.get("type").and_then(|v| v.as_str()), Some("transaction"));
        assert_eq!(payload.get("channelId").and_then(|v| v.as_str()), Some("0xchan"));
        assert_eq!(payload.get("salt").and_then(|v| v.as_str()), Some("0xsalt"));
        // 顶层 challenge / source 也保留
        assert!(stripped.get("challenge").is_some());
        assert_eq!(stripped.get("source").and_then(|v| v.as_str()), Some("did:pkh:eip155:196:0xabc"));
    }

    #[test]
    fn strip_works_when_sdk_only_fields_absent() {
        // CLI 不发这两字段时,strip 是 no-op,不应崩
        let mut cred = fixture_credential_with_initial_voucher("transaction");
        if let Some(obj) = cred.payload.as_object_mut() {
            obj.remove("cumulativeAmount");
            obj.remove("voucherSignature");
        }
        let stripped = strip_sdk_only_open_fields(&cred).unwrap();
        let payload = stripped.get("payload").and_then(|v| v.as_object()).unwrap();
        assert_eq!(payload.get("action").and_then(|v| v.as_str()), Some("open"));
    }

    /// 用 fixture_signer 在 test channel/escrow 下签 voucher。
    fn fixture_voucher_sig(cum: u128) -> Bytes {
        let signer = fixture_signer();
        let escrow = address!("eb18025208061781a287fFc2c1F31C03A24a24c0");
        let channel_id = b256!("6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f");
        let domain = crate::eip712::build_domain(&DomainMeta::default(), 196, escrow);
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
            spent: 0,
            units: 0,
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

        // 较低 cum 但合法签名 → 70013 (delta <= 0,统一归到 voucher_delta_too_small)
        let err = method.submit_voucher(&cid, 50, sig50).await.unwrap_err();
        assert_eq!(err.code, 70013);
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
    async fn deduct_from_channel_increments_spent_and_units() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let mut record = fixture_channel_record();
        record.highest_voucher_amount = 1000;
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let r1 = method.deduct_from_channel(&cid, 300).await.unwrap();
        assert_eq!(r1.spent, 300);
        assert_eq!(r1.units, 1);

        let r2 = method.deduct_from_channel(&cid, 200).await.unwrap();
        assert_eq!(r2.spent, 500);
        assert_eq!(r2.units, 2);
    }

    #[tokio::test]
    async fn deduct_from_channel_insufficient_balance_returns_70015() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let mut record = fixture_channel_record();
        record.highest_voucher_amount = 100;
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let err = method.deduct_from_channel(&cid, 200).await.unwrap_err();
        assert_eq!(err.code, 70015);

        // store 不应被修改
        let r = method.store.get(&cid).await.unwrap();
        assert_eq!(r.spent, 0);
        assert_eq!(r.units, 0);
    }

    #[tokio::test]
    async fn deduct_from_channel_missing_returns_70010() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());
        let err = method
            .deduct_from_channel("0xnonexistent", 1)
            .await
            .unwrap_err();
        assert_eq!(err.code, 70010);
    }

    /// 验证 `with_signer` 接受任何 `Signer` 实现,包括用户提供的远程签名器。
    /// 这里用一个最小 mock(本地包了一层 PrivateKeySigner,语义上等价于 KMS /
    /// Ledger / WalletConnect 等远程 signer 的 wrapper),验证泛型 trait bound
    /// 真的能满足、`dyn Signer + Send + Sync` 字段类型在 `signer.address()` /
    /// `signer.sign_hash()` 调用路径上都跑得通。
    #[tokio::test]
    async fn with_signer_accepts_arbitrary_signer_impl() {
        use alloy_signer::Signature;

        struct WrappedSigner {
            inner: PrivateKeySigner,
        }

        #[async_trait]
        impl Signer for WrappedSigner {
            async fn sign_hash(&self, hash: &B256) -> alloy_signer::Result<Signature> {
                self.inner.sign_hash(hash).await
            }
            fn address(&self) -> Address {
                self.inner.address()
            }
            fn chain_id(&self) -> Option<alloy_primitives::ChainId> {
                self.inner.chain_id()
            }
            fn set_chain_id(&mut self, chain_id: Option<alloy_primitives::ChainId>) {
                self.inner.set_chain_id(chain_id);
            }
        }

        let wrapped = WrappedSigner {
            inner: fixture_signer(),
        };
        let expected_address = wrapped.address();

        // 关键验证:`with_signer` 接受任意 Signer 实现,而不只是 PrivateKeySigner。
        let method = EvmSessionMethod::new(Arc::new(StubSa::default())).with_signer(wrapped);

        // payee_address 来自 signer.address(),应该等于内部 PrivateKeySigner 地址。
        assert_eq!(method.payee_address, Some(expected_address));
        // verify_payee 链式 fast-fail 也走 trait method,无 dyn 调用问题。
        let method = method
            .verify_payee(expected_address)
            .expect("payee match must pass for self-wrapped signer");
        assert!(method.signer.is_some());
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
    async fn voucher_action_auto_deducts_from_request_amount() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let record = fixture_channel_record();
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let cum = 200u128;
        let sig = fixture_voucher_sig(cum);

        let cred = PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-voucher-1".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "session".into(),
                request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
                expires: None,
                digest: None,
                opaque: None,
            },
            source: None,
            payload: serde_json::json!({
                "action": "voucher",
                "channelId": cid,
                "cumulativeAmount": cum.to_string(),
                "signature": format!("0x{}", hex::encode(&sig)),
            }),
        };
        let mut req = dummy_request();
        req.amount = "150".into();

        let receipt = method.verify_session(&cred, &req).await.unwrap();
        assert_eq!(receipt.reference, cid);

        // store 应记录 spent=150 / units=1
        let r = method.store.get(&cid).await.unwrap();
        assert_eq!(r.spent, 150);
        assert_eq!(r.units, 1);
        assert_eq!(r.highest_voucher_amount, 200);

        // respond() 应回 spent / units
        let body = method.respond(&cred, &receipt).expect("respond body for voucher");
        assert_eq!(body.get("spent").and_then(|v| v.as_str()), Some("150"));
        assert_eq!(body.get("units").and_then(|v| v.as_u64()), Some(1));

        // 二次 respond 同 challenge_id 应返 None（已 take 走）
        assert!(method.respond(&cred, &receipt).is_none());
    }

    #[tokio::test]
    async fn voucher_byte_replay_keeps_deducting_until_balance_exhausted() {
        // 对齐 mppx / OKX TS Session:同一份 voucher 字节可以多次复用,每次
        // 都走 deduct,直到 spent 顶到 highest 才返 70015。client 由此可以
        // 一次签大额 voucher,后续多次请求复用同一份字节而不必重新签名。
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let record = fixture_channel_record();
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let cum = 200u128;
        let sig = fixture_voucher_sig(cum);
        let make_cred = || PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-replay".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "session".into(),
                request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
                expires: None,
                digest: None,
                opaque: None,
            },
            source: None,
            payload: serde_json::json!({
                "action": "voucher",
                "channelId": cid,
                "cumulativeAmount": cum.to_string(),
                "signature": format!("0x{}", hex::encode(&sig)),
            }),
        };
        let mut req = dummy_request();
        req.amount = "80".into();

        // 第一次:扣 80 → spent=80 units=1
        let r1 = method.verify_session(&make_cred(), &req).await.unwrap();
        let body1 = method.respond(&make_cred(), &r1).expect("body1");
        assert_eq!(body1.get("spent").and_then(|v| v.as_str()), Some("80"));
        assert_eq!(body1.get("units").and_then(|v| v.as_u64()), Some(1));

        // 第二次:同 voucher 字节级重发 → 仍 deduct → spent=160 units=2
        let r2 = method.verify_session(&make_cred(), &req).await.unwrap();
        let body2 = method.respond(&make_cred(), &r2).expect("body2");
        assert_eq!(body2.get("spent").and_then(|v| v.as_str()), Some("160"));
        assert_eq!(body2.get("units").and_then(|v| v.as_u64()), Some(2));

        // 第三次:available=200-160=40 < 80 → 70015,不再扣
        let err = method
            .verify_session(&make_cred(), &req)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("insufficient"), "expected insufficient balance, got: {err}");

        // store 仍是 spent=160 units=2(第三次失败不写)
        let s = method.store.get(&cid).await.unwrap();
        assert_eq!(s.spent, 160);
        assert_eq!(s.units, 2);
    }

    #[tokio::test]
    async fn submit_voucher_byte_replay_is_idempotent() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let record = fixture_channel_record();
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let cum = 100u128;
        let sig = fixture_voucher_sig(cum);

        // 第一次:新 voucher 接受
        method.submit_voucher(&cid, cum, sig.clone()).await.unwrap();
        let r1 = method.store.get(&cid).await.unwrap();
        assert_eq!(r1.highest_voucher_amount, cum);

        // 第二次:同字节重发 → 仍 Ok,store 状态不变(submit_voucher 不再
        // 暴露 replay 信号给调用方;deduct 责任在 handle_voucher 那一层)
        method.submit_voucher(&cid, cum, sig).await.unwrap();
        let r2 = method.store.get(&cid).await.unwrap();
        assert_eq!(r2.highest_voucher_amount, cum);
        assert_eq!(r2.spent, r1.spent, "submit_voucher itself never deducts");
    }

    #[tokio::test]
    async fn voucher_action_insufficient_balance_after_overspend() {
        let method = EvmSessionMethod::new(Arc::new(StubSa::default()))
            .with_signer(fixture_signer());

        let record = fixture_channel_record();
        let cid = record.channel_id.clone();
        method.store.put(record).await;

        let cum = 100u128;
        let sig = fixture_voucher_sig(cum);
        let cred = PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-voucher-2".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "session".into(),
                request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
                expires: None,
                digest: None,
                opaque: None,
            },
            source: None,
            payload: serde_json::json!({
                "action": "voucher",
                "channelId": cid,
                "cumulativeAmount": cum.to_string(),
                "signature": format!("0x{}", hex::encode(&sig)),
            }),
        };
        // 请求 200 但 voucher 只到 100 → insufficient
        let mut req = dummy_request();
        req.amount = "200".into();

        let err = method.verify_session(&cred, &req).await.unwrap_err();
        assert!(err.to_string().contains("insufficient balance"));
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
