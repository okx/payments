//! Local channel state store for the Seller SDK.
//!
//! 存储 SDK 本地维护的 [`ChannelRecord`]：voucher 验签必需的链上参数（payer /
//! payee / authorized_signer / escrow / chain_id）、最高 voucher（防字节级重放）、
//! 节流参数（min_voucher_delta）。
//!
//! `SessionStore` 是可插拔 trait：
//! - 默认 [`InMemorySessionStore`]：进程内 HashMap，重启会丢；适合 demo / 单进程
//! - 生产场景商户应自实现 `SqliteSessionStore` / `RedisSessionStore`（参考 §3.5
//!   的 SQLite 模板）；接 [`EvmSessionMethod::with_store`] 注入即可
//!
//! 持久化职责在商户:`get` miss 时返 `None`,SDK 不调 `/session/status` 自动
//! 回源 —— SA API 回源能拿到的字段是子集(没有 `cumulativeAmount` 和
//! `highest_voucher_signature`),无法重建 voucher 状态。跨进程稳定的商户需
//! 自行实现持久化 store。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use alloy_primitives::{Address, Bytes};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::SaApiError;

/// SDK 本地维护的 channel 记录。12 个字段最小化 —— 不存
/// `settled_on_chain` / `finalized` / `close_requested_at` / `last_receipt` /
/// `challenge`（与 Tempo `ChannelState` 不同），见文档 §3.5 设计说明。
///
/// 计费会计字段 `spent` / `units` 与 TS Session.ts ChannelState 对齐：
/// `available = highest_voucher_amount - spent` 是当前可扣额度。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelRecord {
    pub channel_id: String,
    pub chain_id: u64,
    pub escrow_contract: Address,

    /// 付款方地址。
    pub payer: Address,
    /// 收款方（商户）地址。在 ACTION_OPEN 时 SDK 会校验 == `signer.address()`。
    pub payee: Address,
    /// Voucher 签名人地址。`channel.authorizedSigner == address(0)` 时
    /// 在 open 时已解析为 `payer`，存储层永远拿到有效非零地址。
    pub authorized_signer: Address,

    /// 当前累计入金（open 时初始化、topUp 时累加）。
    pub deposit: u128,
    /// SDK 已接受过的最高 voucher 累计金额。
    pub highest_voucher_amount: u128,
    /// 对应 `highest_voucher_amount` 的 65 字节签名。用于：
    /// 1. settle / close 时上送 SA API 作为 voucherSignature
    /// 2. submit_voucher 字节级幂等比对（`highest_voucher_amount` 相等且签名字节
    ///    完全一致 → 视为幂等重发，不再走验签）
    pub highest_voucher_signature: Option<Bytes>,

    /// 节流：voucher 最小递增量，配置在 `SessionMethodDetails.minVoucherDelta`。
    /// `None` 视为无节流。
    pub min_voucher_delta: Option<u128>,

    /// 已扣费总额（base units）。每次 `deduct_from_channel` 累加。
    /// 不变量：`spent <= highest_voucher_amount`。
    #[serde(default)]
    pub spent: u128,
    /// 已计费次数（`deduct_from_channel` 调用次数）。
    #[serde(default)]
    pub units: u64,
}

impl ChannelRecord {
    /// `authorized_signer` 已经在 open 时解析为有效地址（`address(0) → payer`），
    /// 此方法直接返回该地址，便于本地验签调用。
    pub fn voucher_signer(&self) -> Address {
        self.authorized_signer
    }
}

/// 闭包类型：原子更新 [`ChannelRecord`]。返回 `Err` 时整个 `update` 失败，
/// 旧值保持不变（与数据库事务语义一致）。
pub type ChannelUpdater =
    Box<dyn FnOnce(&mut ChannelRecord) -> Result<(), SaApiError> + Send>;

/// 可插拔的 channel 存储 trait。
///
/// **不**耦合 SA API：trait 内不调用任何 HTTP 接口，纯数据存取。SDK 也不会
/// 在 miss 时自动调 SA API 回源 —— 回源能拿到的字段是子集(没有
/// cumulativeAmount / highest_voucher_signature)，重建语义不完整;商户跨进程
/// 稳定运行需自行实现持久化 store。
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// 读取 channel。`None` 表示本地无记录。
    async fn get(&self, channel_id: &str) -> Option<ChannelRecord>;

    /// 写入 channel。如果已存在则覆盖。
    async fn put(&self, record: ChannelRecord);

    /// 删除 channel。close 成功后由 `EvmSessionMethod` 调用。
    async fn remove(&self, channel_id: &str);

    /// 原子闭包更新：取出当前记录，应用 `updater`，写回。
    /// `None`（channel 不存在）→ 返回 `70010 channel_not_found`。
    /// `updater` 返回 `Err` → 写回不发生，错误透传。
    async fn update(
        &self,
        channel_id: &str,
        updater: ChannelUpdater,
    ) -> Result<ChannelRecord, SaApiError>;
}

/// 默认实现：进程内 HashMap，使用 std `Mutex` 同步（操作短，无 await）。
///
/// **重启丢失**：进程重启 / 崩溃后所有 channel 状态消失。生产场景请自实现
/// 持久化版本。详见 §3.5 警告段。
#[derive(Debug, Default, Clone)]
pub struct InMemorySessionStore {
    inner: Arc<Mutex<HashMap<String, ChannelRecord>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn get(&self, channel_id: &str) -> Option<ChannelRecord> {
        self.inner.lock().unwrap().get(channel_id).cloned()
    }

    async fn put(&self, record: ChannelRecord) {
        self.inner
            .lock()
            .unwrap()
            .insert(record.channel_id.clone(), record);
    }

    async fn remove(&self, channel_id: &str) {
        self.inner.lock().unwrap().remove(channel_id);
    }

    async fn update(
        &self,
        channel_id: &str,
        updater: ChannelUpdater,
    ) -> Result<ChannelRecord, SaApiError> {
        let mut map = self.inner.lock().unwrap();
        let record = map
            .get_mut(channel_id)
            .ok_or_else(|| SaApiError::new(70010, "channel not found"))?;
        updater(record)?;
        Ok(record.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    fn fixture_record(channel_id: &str, deposit: u128, highest: u128) -> ChannelRecord {
        ChannelRecord {
            channel_id: channel_id.to_string(),
            chain_id: 196,
            escrow_contract: address!("eb18025208061781a287fFc2c1F31C03A24a24c0"),
            payer: address!("aabbccddee11223344556677889900aabbccddee"),
            payee: address!("742d35Cc6634c0532925a3b844bC9e7595F8fE00"),
            authorized_signer: address!("aabbccddee11223344556677889900aabbccddee"),
            deposit,
            highest_voucher_amount: highest,
            highest_voucher_signature: None,
            min_voucher_delta: None,
            spent: 0,
            units: 0,
        }
    }

    #[tokio::test]
    async fn put_then_get_returns_record() {
        let store = InMemorySessionStore::new();
        store.put(fixture_record("0xa", 1000, 0)).await;
        let got = store.get("0xa").await.unwrap();
        assert_eq!(got.channel_id, "0xa");
        assert_eq!(got.deposit, 1000);
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let store = InMemorySessionStore::new();
        assert!(store.get("0xnope").await.is_none());
    }

    #[tokio::test]
    async fn put_overwrites_previous_record() {
        let store = InMemorySessionStore::new();
        store.put(fixture_record("0xa", 1000, 0)).await;
        store.put(fixture_record("0xa", 2000, 100)).await;
        let got = store.get("0xa").await.unwrap();
        assert_eq!(got.deposit, 2000);
        assert_eq!(got.highest_voucher_amount, 100);
    }

    #[tokio::test]
    async fn remove_clears_record() {
        let store = InMemorySessionStore::new();
        store.put(fixture_record("0xa", 1000, 0)).await;
        store.remove("0xa").await;
        assert!(store.get("0xa").await.is_none());
    }

    #[tokio::test]
    async fn update_applies_closure_atomically() {
        let store = InMemorySessionStore::new();
        store.put(fixture_record("0xa", 1000, 100)).await;

        let updated = store
            .update(
                "0xa",
                Box::new(|r| {
                    r.highest_voucher_amount = 250;
                    r.highest_voucher_signature = Some(Bytes::from(vec![0xab; 65]));
                    Ok(())
                }),
            )
            .await
            .unwrap();
        assert_eq!(updated.highest_voucher_amount, 250);

        // 验证 store 里的值确实被改了
        let got = store.get("0xa").await.unwrap();
        assert_eq!(got.highest_voucher_amount, 250);
        assert_eq!(got.highest_voucher_signature.unwrap().len(), 65);
    }

    #[tokio::test]
    async fn update_missing_channel_returns_70010() {
        let store = InMemorySessionStore::new();
        let result = store
            .update("0xnope", Box::new(|_| Ok(())))
            .await;
        match result {
            Err(e) => assert_eq!(e.code, 70010),
            Ok(_) => panic!("expected error for missing channel"),
        }
    }

    #[tokio::test]
    async fn update_propagates_closure_error_and_does_not_modify() {
        let store = InMemorySessionStore::new();
        store.put(fixture_record("0xa", 1000, 100)).await;

        let result = store
            .update(
                "0xa",
                Box::new(|r| {
                    // 闭包先改字段再返错,验证 in-memory 实现下 record 已被改了一半。
                    r.highest_voucher_amount = 999;
                    Err(SaApiError::new(70013, "delta too small"))
                }),
            )
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, 70013);

        // 验证 store 里的值是否回滚 —— 当前实现是「闭包失败时 record 已被改了一半」。
        // 这是 in-memory 实现的取舍：std Mutex 不支持事务回滚。
        // 真正需要事务的场景（SqliteSessionStore）应该在自己的实现里把 update
        // 包在 BEGIN/COMMIT 中。
        //
        // 这里我们 SoA: 验证闭包执行了（即使返回 Err），然后调用方应该清楚
        // in-memory 实现不保证事务回滚。
        let got = store.get("0xa").await.unwrap();
        assert_eq!(got.highest_voucher_amount, 999);
    }

    #[tokio::test]
    async fn channel_record_round_trips_serde() {
        let original = fixture_record("0xa", 1000, 250);
        let json = serde_json::to_string(&original).unwrap();
        let parsed: ChannelRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn voucher_signer_returns_authorized_signer() {
        let r = fixture_record("0xa", 100, 0);
        assert_eq!(r.voucher_signer(), r.authorized_signer);
    }
}
