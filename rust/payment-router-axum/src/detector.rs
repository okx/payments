//! Spec §3 / §10 detect dispatcher.
//!
//! Adapters are tried in priority order (smallest number first). First
//! `detect() -> true` wins; subsequent adapters are not queried.
//!
//! Per spec §3 #6: adapter.detect panicking is treated as miss. We use
//! `catch_unwind` to enforce this without letting a buggy adapter take down
//! the request.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use http::request::Parts;

use crate::adapter::ProtocolAdapter;

/// Find the first adapter (by priority ascending) whose `detect` returns true.
///
/// The `adapters` slice is expected to be pre-sorted at construction time
/// (`PaymentRouterService::new`). Returns the index into the slice, which is
/// parallel to the adapter-services Vec.
pub(crate) fn detect(adapters: &[Arc<dyn ProtocolAdapter>], parts: &Parts) -> Option<usize> {
    for (i, adapter) in adapters.iter().enumerate() {
        // catch panics to fulfill spec §3 #6 ("detect should not throw; a
        // panic is treated as miss"). AssertUnwindSafe is safe here because we
        // only touch the parts borrow and immediately discard any result.
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| adapter.detect(parts)));
        match result {
            Ok(true) => return Some(i),
            Ok(false) => continue,
            Err(_) => {
                tracing::warn!(
                    adapter = adapter.name(),
                    "adapter.detect panicked; treating as miss"
                );
                continue;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ChallengeFuture, InnerService};
    use http::{HeaderMap, Request};
    use serde_json::Value;

    struct FakeAdapter {
        name: String,
        priority: u32,
        detect_result: bool,
    }

    impl ProtocolAdapter for FakeAdapter {
        fn name(&self) -> &str {
            &self.name
        }
        fn priority(&self) -> u32 {
            self.priority
        }
        fn detect(&self, _parts: &Parts) -> bool {
            self.detect_result
        }
        fn get_challenge<'a>(
            &'a self,
            _parts: &'a Parts,
            _route_cfg: &'a Value,
        ) -> ChallengeFuture<'a> {
            Box::pin(async {
                Ok::<Option<crate::adapter::ChallengeResponse>, String>(None)
            })
        }
        fn make_service(&self, inner: InnerService) -> InnerService {
            inner
        }
    }

    fn fake(name: &str, priority: u32, detect_result: bool) -> Arc<dyn ProtocolAdapter> {
        Arc::new(FakeAdapter {
            name: name.into(),
            priority,
            detect_result,
        })
    }

    fn parts() -> Parts {
        let (parts, _) = Request::new(axum::body::Body::empty()).into_parts();
        parts
    }

    #[test]
    fn picks_first_matching() {
        let adapters = vec![fake("mpp", 10, true), fake("x402", 20, true)];
        assert_eq!(detect(&adapters, &parts()), Some(0));
    }

    #[test]
    fn skips_miss_to_next() {
        let adapters = vec![fake("mpp", 10, false), fake("x402", 20, true)];
        assert_eq!(detect(&adapters, &parts()), Some(1));
    }

    #[test]
    fn all_miss_returns_none() {
        let adapters = vec![fake("mpp", 10, false), fake("x402", 20, false)];
        assert_eq!(detect(&adapters, &parts()), None);
    }

    #[test]
    fn empty_returns_none() {
        let adapters: Vec<Arc<dyn ProtocolAdapter>> = vec![];
        assert_eq!(detect(&adapters, &parts()), None);
    }
}
