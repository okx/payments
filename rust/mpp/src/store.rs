//! Minimal channel state store for the Seller SDK.
//!
//! Scope: non-SSE session flows. Keeps per-channel metadata needed to drive
//! idle-timeout cleanup (via `/session/settle`) and to return receipts in
//! management responses. Does NOT maintain `spent` counters — that lives on
//! SA API for this SDK iteration.
//!
//! Pluggable: `SessionStore` is a trait with an `InMemorySessionStore` default.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::types::SessionReceipt;

/// Per-channel state snapshot remembered by the Seller SDK.
#[derive(Debug, Clone)]
pub struct ChannelRecord {
    /// Canonical channel identifier (hex bytes32, 0x-prefixed).
    pub channel_id: String,
    /// Highest cumulative voucher amount SA API has confirmed for this channel.
    pub accepted_cumulative: String,
    /// Last receipt returned by SA API for this channel. Used to surface
    /// consistent management responses and to diagnose stuck channels.
    pub last_receipt: SessionReceipt,
}

impl ChannelRecord {
    pub fn from_receipt(receipt: SessionReceipt) -> Self {
        Self {
            channel_id: receipt.channel_id.clone(),
            accepted_cumulative: receipt.accepted_cumulative.clone(),
            last_receipt: receipt,
        }
    }
}

/// Pluggable channel state store.
///
/// Implementations MUST provide sequentially-consistent reads/writes per
/// channel. No ordering guarantees across channels are required.
#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn get(&self, channel_id: &str) -> Option<ChannelRecord>;
    async fn put(&self, record: ChannelRecord);
    async fn remove(&self, channel_id: &str);
}

/// In-memory default implementation.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(channel_id: &str, accepted: &str) -> SessionReceipt {
        SessionReceipt {
            method: "evm".into(),
            intent: "session".into(),
            status: "success".into(),
            timestamp: "2026-04-01T12:00:00Z".into(),
            chain_id: 196,
            challenge_id: "ch-1".into(),
            channel_id: channel_id.into(),
            accepted_cumulative: accepted.into(),
            spent: None,
            reference: None,
            confirmations: None,
            units: None,
        }
    }

    #[tokio::test]
    async fn put_then_get_returns_record() {
        let store = InMemorySessionStore::new();
        store
            .put(ChannelRecord::from_receipt(receipt("0xa", "100")))
            .await;
        let got = store.get("0xa").await.unwrap();
        assert_eq!(got.channel_id, "0xa");
        assert_eq!(got.accepted_cumulative, "100");
    }

    #[tokio::test]
    async fn put_overwrites_previous_record() {
        let store = InMemorySessionStore::new();
        store
            .put(ChannelRecord::from_receipt(receipt("0xa", "100")))
            .await;
        store
            .put(ChannelRecord::from_receipt(receipt("0xa", "200")))
            .await;
        assert_eq!(store.get("0xa").await.unwrap().accepted_cumulative, "200");
    }

    #[tokio::test]
    async fn remove_clears_record() {
        let store = InMemorySessionStore::new();
        store
            .put(ChannelRecord::from_receipt(receipt("0xa", "100")))
            .await;
        store.remove("0xa").await;
        assert!(store.get("0xa").await.is_none());
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let store = InMemorySessionStore::new();
        assert!(store.get("0xnope").await.is_none());
    }
}
