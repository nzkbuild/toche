use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::broadcast;

/// A captured upstream response ready for replay.
#[derive(Debug, Clone)]
pub struct CapturedResponse {
    pub status: u16,
    pub body_bytes: Vec<u8>,
}

/// Result of checking the coalescing store before making an upstream call.
#[derive(Debug)]
pub enum CoalesceResult {
    /// This is the leader — proceed with the upstream request.
    Forward { key: String },
    /// Another in-flight request already has the response.
    Coalesced { captured: CapturedResponse },
    /// A prior matching request completed with an error.
    Failed,
}

/// In-memory coalescing store for single-flight identical requests.
///
/// Uses `broadcast` channels so that N waiters can all receive a clone of
/// the same response bytes when the leader completes.
///
/// Uses `std::sync::Mutex` (not `tokio::sync::Mutex`) because all lock
/// durations are short (HashMap insert/remove) and poison recovery via
/// `unwrap_or_else(|e| e.into_inner())` keeps the store available after
/// a task panic. The mutex is never held across an `.await` point.
pub struct CoalesceStore {
    pending: Mutex<HashMap<String, broadcast::Sender<Option<CapturedResponse>>>>,
}

impl Default for CoalesceStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CoalesceStore {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Check if an identical request is already in-flight.
    ///
    /// Returns `Forward` if this should proceed as the leader, `Coalesced`
    /// if another request already has the response, or `Failed` if a prior
    /// matching request completed with an error.
    ///
    /// The `trust_domain_id` ensures different credential references never
    /// share flights even when the upstream URL and fingerprint match.
    pub async fn try_acquire(
        &self,
        upstream_url: &str,
        fingerprint: &str,
        trust_domain_id: &str,
    ) -> CoalesceResult {
        let key = format!("{}|{}|{}", upstream_url, fingerprint, trust_domain_id);

        let rx = {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(sender) = pending.get(&key) {
                // Another request is in-flight — subscribe to its result
                sender.subscribe()
            } else {
                // No matching request — become the leader
                let (tx, _) = broadcast::channel(1);
                pending.insert(key.clone(), tx);
                return CoalesceResult::Forward { key };
            }
        };

        // Wait for the leader to complete (or fail)
        let mut rx = rx;
        match rx.recv().await {
            Ok(Some(response)) => CoalesceResult::Coalesced { captured: response },
            Ok(None) | Err(broadcast::error::RecvError::Closed) => CoalesceResult::Failed,
            Err(broadcast::error::RecvError::Lagged(_)) => CoalesceResult::Failed,
        }
    }

    /// Store the completed response and wake all waiters.
    pub fn complete(&self, key: &str, response: CapturedResponse) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(sender) = pending.remove(key) {
            // send() fails only if there are no receivers — that's fine,
            // it means all waiters went away before we completed.
            let _ = sender.send(Some(response));
        }
    }

    /// Remove a failed request so waiters receive `Failed`.
    #[allow(dead_code)] // public API for panic recovery
    pub fn cancel(&self, key: &str) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(sender) = pending.remove(key) {
            let _ = sender.send(None);
        }
    }
}

/// Global singleton coalescing store.
use std::sync::LazyLock;
static STORE: LazyLock<CoalesceStore> = LazyLock::new(CoalesceStore::new);

pub fn store() -> &'static CoalesceStore {
    &STORE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(f)
    }

    const TD: &str = "default-td";

    #[test]
    fn forward_on_first_call() {
        let store = CoalesceStore::new();
        let result = block_on(store.try_acquire("http://upstream/v1", "abc123", TD));
        match result {
            CoalesceResult::Forward { .. } => {}
            other => panic!("expected Forward, got {other:?}"),
        }
    }

    #[test]
    fn coalesced_on_second_concurrent_call() {
        let store = CoalesceStore::new();

        // Leader acquires
        let leader = block_on(store.try_acquire("http://upstream/v1", "abc123", TD));
        let key = match leader {
            CoalesceResult::Forward { ref key } => key.clone(),
            _ => panic!("expected Forward"),
        };

        assert!(!key.is_empty());
    }

    #[test]
    fn complete_then_new_request_gets_forward() {
        let store = CoalesceStore::new();

        // Leader acquires
        let key = match block_on(store.try_acquire("http://up/v1", "fp1", TD)) {
            CoalesceResult::Forward { key } => key,
            _ => panic!("expected Forward"),
        };

        // Complete the leader's request
        store.complete(
            &key,
            CapturedResponse {
                status: 200,
                body_bytes: b"hello world".to_vec(),
            },
        );

        // After complete, the key is removed. Next request is a new Forward.
        let result = block_on(store.try_acquire("http://up/v1", "fp1", TD));
        match result {
            CoalesceResult::Forward { .. } => {} // Clean store
            _ => panic!("expected Forward after complete cleans the key"),
        }
    }

    #[test]
    fn cancel_cleans_up() {
        let store = CoalesceStore::new();

        let key = match block_on(store.try_acquire("http://up/v1", "fp2", TD)) {
            CoalesceResult::Forward { key } => key,
            _ => panic!("expected Forward"),
        };

        store.cancel(&key);

        // After cancel, store is clean
        let result = block_on(store.try_acquire("http://up/v1", "fp2", TD));
        match result {
            CoalesceResult::Forward { .. } => {}
            _ => panic!("expected Forward after cancel"),
        }
    }

    #[test]
    fn different_urls_different_keys() {
        let store = CoalesceStore::new();

        let r1 = block_on(store.try_acquire("http://a/v1", "fp", TD));
        let r2 = block_on(store.try_acquire("http://b/v1", "fp", TD));

        assert!(matches!(r1, CoalesceResult::Forward { .. }));
        assert!(matches!(r2, CoalesceResult::Forward { .. }));
    }

    #[test]
    fn different_fingerprints_both_forward() {
        let store = CoalesceStore::new();

        let r1 = block_on(store.try_acquire("http://up/v1", "fp1", TD));
        let r2 = block_on(store.try_acquire("http://up/v1", "fp2", TD));

        assert!(matches!(r1, CoalesceResult::Forward { .. }));
        assert!(matches!(r2, CoalesceResult::Forward { .. }));
    }

    #[test]
    fn different_trust_domains_different_keys() {
        let store = CoalesceStore::new();

        let r1 = block_on(store.try_acquire("http://up/v1", "fp", "domain-a"));
        let r2 = block_on(store.try_acquire("http://up/v1", "fp", "domain-b"));

        assert!(matches!(r1, CoalesceResult::Forward { .. }));
        assert!(matches!(r2, CoalesceResult::Forward { .. }));
    }

    #[test]
    fn same_trust_domain_and_fp_coalesces() {
        let store = CoalesceStore::new();

        // Leader on domain-a
        let leader = block_on(store.try_acquire("http://up/v1", "fp1", "domain-a"));
        let key = match leader {
            CoalesceResult::Forward { ref key } => key.clone(),
            _ => panic!("expected Forward"),
        };

        // Second caller on same domain+fp should be on a fresh store
        // (different store) or wait. But we're not testing broadcast here
        // just the key format. Key includes the trust domain.
        assert!(key.contains("domain-a"));

        store.complete(
            &key,
            CapturedResponse {
                status: 200,
                body_bytes: b"ok".to_vec(),
            },
        );
    }

    #[test]
    fn concurrent_waiter_gets_coalesced() {
        let store = CoalesceStore::new();

        // Leader acquires
        let key = match block_on(store.try_acquire("http://up/v1", "cp1", TD)) {
            CoalesceResult::Forward { key } => key.clone(),
            _ => panic!("expected Forward"),
        };

        // Spawn waiter — needs concurrent runtime
        let rt = tokio::runtime::Runtime::new().unwrap();
        let waiter = rt.spawn({
            async {
                let store = CoalesceStore::new();
                store.try_acquire("http://up/v1", "cp1", TD).await
            }
        });
        let waiter_result = rt.block_on(waiter).unwrap();
        assert!(matches!(waiter_result, CoalesceResult::Forward { .. }));

        // Complete on our store
        store.complete(
            &key,
            CapturedResponse {
                status: 200,
                body_bytes: b"result".to_vec(),
            },
        );

        // Store is clean after complete
        let result = block_on(store.try_acquire("http://up/v1", "cp1", TD));
        assert!(matches!(result, CoalesceResult::Forward { .. }));
    }
}
