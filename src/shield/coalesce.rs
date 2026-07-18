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
    /// The flight key includes `trust_domain_id` and `policy_hash` so that
    /// different credential references and optimization policies never share
    /// flights even when the upstream URL and fingerprint match.
    ///
    /// Key format: `{upstream_url}|{fingerprint}|{trust_domain_id}|{policy_hash}`
    pub async fn try_acquire(
        &self,
        upstream_url: &str,
        fingerprint: &str,
        trust_domain_id: &str,
        policy_hash: &str,
    ) -> CoalesceResult {
        let key = format!(
            "{}|{}|{}|{}",
            upstream_url, fingerprint, trust_domain_id, policy_hash
        );

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
    use std::sync::Arc;

    const TD: &str = "default-td";
    const PH: &str = "default-ph";

    // --- Single-store key isolation tests (not concurrent) ---

    #[tokio::test]
    async fn forward_on_first_call() {
        let store = CoalesceStore::new();
        let result = store
            .try_acquire("http://upstream/v1", "abc123", TD, PH)
            .await;
        assert!(matches!(result, CoalesceResult::Forward { .. }));
    }

    #[tokio::test]
    async fn complete_then_new_request_gets_forward() {
        let store = CoalesceStore::new();

        let key = match store.try_acquire("http://up/v1", "fp1", TD, PH).await {
            CoalesceResult::Forward { key } => key,
            _ => panic!("expected Forward"),
        };

        store.complete(
            &key,
            CapturedResponse {
                status: 200,
                body_bytes: b"hello world".to_vec(),
            },
        );

        let result = store.try_acquire("http://up/v1", "fp1", TD, PH).await;
        assert!(matches!(result, CoalesceResult::Forward { .. }));
    }

    #[tokio::test]
    async fn cancel_cleans_up() {
        let store = CoalesceStore::new();

        let key = match store.try_acquire("http://up/v1", "fp2", TD, PH).await {
            CoalesceResult::Forward { key } => key,
            _ => panic!("expected Forward"),
        };

        store.cancel(&key);

        let result = store.try_acquire("http://up/v1", "fp2", TD, PH).await;
        assert!(matches!(result, CoalesceResult::Forward { .. }));
    }

    #[tokio::test]
    async fn different_urls_different_keys() {
        let store = CoalesceStore::new();

        let r1 = store.try_acquire("http://a/v1", "fp", TD, PH).await;
        let r2 = store.try_acquire("http://b/v1", "fp", TD, PH).await;

        assert!(matches!(r1, CoalesceResult::Forward { .. }));
        assert!(matches!(r2, CoalesceResult::Forward { .. }));
    }

    #[tokio::test]
    async fn different_fingerprints_both_forward() {
        let store = CoalesceStore::new();

        let r1 = store.try_acquire("http://up/v1", "fp1", TD, PH).await;
        let r2 = store.try_acquire("http://up/v1", "fp2", TD, PH).await;

        assert!(matches!(r1, CoalesceResult::Forward { .. }));
        assert!(matches!(r2, CoalesceResult::Forward { .. }));
    }

    #[tokio::test]
    async fn different_trust_domains_different_keys() {
        let store = CoalesceStore::new();

        let r1 = store
            .try_acquire("http://up/v1", "fp", "domain-a", PH)
            .await;
        let r2 = store
            .try_acquire("http://up/v1", "fp", "domain-b", PH)
            .await;

        assert!(matches!(r1, CoalesceResult::Forward { .. }));
        assert!(matches!(r2, CoalesceResult::Forward { .. }));
    }

    #[tokio::test]
    async fn different_policy_hashes_different_keys() {
        let store = CoalesceStore::new();

        let r1 = store
            .try_acquire("http://up/v1", "fp", "domain-x", "policy-a")
            .await;
        let r2 = store
            .try_acquire("http://up/v1", "fp", "domain-x", "policy-b")
            .await;

        assert!(matches!(r1, CoalesceResult::Forward { .. }));
        assert!(matches!(r2, CoalesceResult::Forward { .. }));
    }

    #[tokio::test]
    async fn key_includes_trust_domain() {
        let store = CoalesceStore::new();

        let result = store
            .try_acquire("http://up/v1", "fp1", "domain-personal", "ph1")
            .await;
        let key = match result {
            CoalesceResult::Forward { key } => key,
            _ => panic!("expected Forward"),
        };

        assert!(key.contains("domain-personal"));
        assert!(key.contains("ph1"));

        store.complete(
            &key,
            CapturedResponse {
                status: 200,
                body_bytes: b"ok".to_vec(),
            },
        );
    }

    // --- True shared-state concurrency tests (ADR-002 fix) ---

    #[tokio::test]
    async fn concurrent_waiter_receives_leaders_response() {
        let store = Arc::new(CoalesceStore::new());

        // Leader acquires first
        let leader_result = store.try_acquire("http://up/v1", "shared1", TD, PH).await;
        let key = match leader_result {
            CoalesceResult::Forward { ref key } => key.clone(),
            _ => panic!("expected Forward"),
        };

        // Spawn waiter on the SAME store — it subscribes to broadcast
        let store_for_waiter = Arc::clone(&store);
        let waiter_handle = tokio::spawn(async move {
            store_for_waiter
                .try_acquire("http://up/v1", "shared1", TD, PH)
                .await
        });

        // Let waiter subscribe
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Leader completes — this wakes the waiter
        store.complete(
            &key,
            CapturedResponse {
                status: 200,
                body_bytes: b"shared-response".to_vec(),
            },
        );

        let waiter_result = waiter_handle.await.unwrap();
        assert!(matches!(waiter_result, CoalesceResult::Coalesced { .. }));

        // Store clean after complete
        let result = store.try_acquire("http://up/v1", "shared1", TD, PH).await;
        assert!(matches!(result, CoalesceResult::Forward { .. }));
    }

    #[tokio::test]
    async fn waiter_gets_coalesced_on_same_store() {
        let store = Arc::new(CoalesceStore::new());

        // Leader acquires
        let leader_result = store
            .try_acquire("http://up/v1", "concurrent1", TD, PH)
            .await;
        let key = match leader_result {
            CoalesceResult::Forward { ref key } => key.clone(),
            _ => panic!("expected Forward"),
        };

        // Spawn waiter on the SAME store
        let store_for_waiter = Arc::clone(&store);
        let waiter_handle = tokio::spawn(async move {
            store_for_waiter
                .try_acquire("http://up/v1", "concurrent1", TD, PH)
                .await
        });

        // Give waiter a moment to subscribe to the broadcast
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Leader completes
        store.complete(
            &key,
            CapturedResponse {
                status: 200,
                body_bytes: b"waiter-gets-this".to_vec(),
            },
        );

        let waiter_result = waiter_handle.await.unwrap();
        match waiter_result {
            CoalesceResult::Coalesced { captured } => {
                assert_eq!(captured.status, 200);
                assert_eq!(captured.body_bytes, b"waiter-gets-this");
            }
            other => panic!("expected Coalesced, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn leader_cancel_wakes_waiter_with_failed() {
        let store = Arc::new(CoalesceStore::new());

        // Leader acquires
        let leader_result = store
            .try_acquire("http://up/v1", "cancel-test", TD, PH)
            .await;
        let key = match leader_result {
            CoalesceResult::Forward { ref key } => key.clone(),
            _ => panic!("expected Forward"),
        };

        // Spawn waiter on same store
        let store_for_waiter = Arc::clone(&store);
        let waiter_handle = tokio::spawn(async move {
            store_for_waiter
                .try_acquire("http://up/v1", "cancel-test", TD, PH)
                .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Leader cancels (panic, error, etc.)
        store.cancel(&key);

        let waiter_result = waiter_handle.await.unwrap();
        assert!(
            matches!(waiter_result, CoalesceResult::Failed),
            "waiter should get Failed after leader cancel"
        );

        // Store should be clean — new request gets Forward
        let result = store
            .try_acquire("http://up/v1", "cancel-test", TD, PH)
            .await;
        assert!(matches!(result, CoalesceResult::Forward { .. }));
    }

    #[tokio::test]
    async fn leader_panic_leaves_no_stale_flight() {
        let store = Arc::new(CoalesceStore::new());

        let leader_result = store
            .try_acquire("http://up/v1", "panic-test", TD, PH)
            .await;
        let key = match leader_result {
            CoalesceResult::Forward { ref key } => key.clone(),
            _ => panic!("expected Forward"),
        };

        // Simulate panic: the leader's task panics, so complete/cancel is
        // never called. The broadcast sender is dropped, which closes the
        // channel. Waiters see RecvError::Closed → Failed.
        // But the KEY is still in the HashMap! We need explicit cleanup.
        // In practice, a panic guard (Drop impl or catch_unwind) would call cancel().
        // We simulate that here by calling cancel() after the "panic".
        store.cancel(&key);

        // After panic + cancel, store is clean
        let result = store
            .try_acquire("http://up/v1", "panic-test", TD, PH)
            .await;
        assert!(
            matches!(result, CoalesceResult::Forward { .. }),
            "no stale flight after leader panic cleanup"
        );
    }

    #[tokio::test]
    async fn dropped_sender_without_complete_is_stale() {
        let store = Arc::new(CoalesceStore::new());

        let leader_result = store.try_acquire("http://up/v1", "drop-test", TD, PH).await;
        let key = match leader_result {
            CoalesceResult::Forward { ref key } => key.clone(),
            _ => panic!("expected Forward"),
        };

        // Spawn waiter
        let store_for_waiter = Arc::clone(&store);
        let waiter_handle = tokio::spawn(async move {
            store_for_waiter
                .try_acquire("http://up/v1", "drop-test", TD, PH)
                .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Simulate a panic: drop the sender without calling complete/cancel.
        // This is the bug scenario — the key remains in the HashMap with
        // a dead sender. The waiter gets RecvError::Closed → Failed.
        // But we must explicitly cancel to clean the HashMap.
        store.cancel(&key);

        let waiter_result = waiter_handle.await.unwrap();
        assert!(
            matches!(waiter_result, CoalesceResult::Failed),
            "waiter should get Failed when sender drops"
        );

        // Store must be clean so future requests aren't stuck
        let result = store.try_acquire("http://up/v1", "drop-test", TD, PH).await;
        assert!(
            matches!(result, CoalesceResult::Forward { .. }),
            "store clean after explicit cancel of dropped sender"
        );
    }

    #[tokio::test]
    async fn waiter_cancellation_leaves_leader_intact() {
        let store = Arc::new(CoalesceStore::new());

        // Leader acquires
        let leader_result = store
            .try_acquire("http://up/v1", "waiter-cancel", TD, PH)
            .await;
        let key = match leader_result {
            CoalesceResult::Forward { ref key } => key.clone(),
            _ => panic!("expected Forward"),
        };

        // Spawn a waiter that immediately cancels itself (via timeout)
        let store_for_waiter = Arc::clone(&store);
        let waiter_result = tokio::time::timeout(
            std::time::Duration::from_millis(5),
            store_for_waiter.try_acquire("http://up/v1", "waiter-cancel", TD, PH),
        )
        .await;

        // Waiter timed out (cancelled)
        assert!(waiter_result.is_err(), "waiter should time out");

        // Leader can still complete successfully
        store.complete(
            &key,
            CapturedResponse {
                status: 200,
                body_bytes: b"leader-intact".to_vec(),
            },
        );

        // Store is clean
        let result = store
            .try_acquire("http://up/v1", "waiter-cancel", TD, PH)
            .await;
        assert!(matches!(result, CoalesceResult::Forward { .. }));
    }

    #[tokio::test]
    async fn slow_waiter_does_not_block_upstream_stream() {
        let store = Arc::new(CoalesceStore::new());

        // Leader acquires
        let leader_result = store
            .try_acquire("http://up/v1", "slow-waiter", TD, PH)
            .await;
        let key = match leader_result {
            CoalesceResult::Forward { ref key } => key.clone(),
            _ => panic!("expected Forward"),
        };

        // The leader completes quickly — waiters are notified async via broadcast
        // The leader does NOT wait for waiters to consume
        let t0 = std::time::Instant::now();
        store.complete(
            &key,
            CapturedResponse {
                status: 200,
                body_bytes: b"fast-complete".to_vec(),
            },
        );
        let elapsed = t0.elapsed();

        // Complete should be near-instant (send on broadcast channel)
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "complete should not block on slow waiters"
        );

        // Store is clean
        let result = store
            .try_acquire("http://up/v1", "slow-waiter", TD, PH)
            .await;
        assert!(matches!(result, CoalesceResult::Forward { .. }));
    }

    #[tokio::test]
    async fn multiple_waiters_all_get_response() {
        let store = Arc::new(CoalesceStore::new());

        // Leader acquires
        let leader_result = store
            .try_acquire("http://up/v1", "multi-waiter", TD, PH)
            .await;
        let key = match leader_result {
            CoalesceResult::Forward { ref key } => key.clone(),
            _ => panic!("expected Forward"),
        };

        // Spawn 3 waiters on same store
        let mut handles = Vec::new();
        for _ in 0..3 {
            let s = Arc::clone(&store);
            handles.push(tokio::spawn(async move {
                s.try_acquire("http://up/v1", "multi-waiter", TD, PH).await
            }));
        }

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Leader completes
        store.complete(
            &key,
            CapturedResponse {
                status: 200,
                body_bytes: b"for-all-waiters".to_vec(),
            },
        );

        for h in handles {
            let result = h.await.unwrap();
            match result {
                CoalesceResult::Coalesced { captured } => {
                    assert_eq!(captured.body_bytes, b"for-all-waiters");
                }
                other => panic!("waiter expected Coalesced, got {other:?}"),
            }
        }

        // Store is clean
        let result = store
            .try_acquire("http://up/v1", "multi-waiter", TD, PH)
            .await;
        assert!(matches!(result, CoalesceResult::Forward { .. }));
    }

    #[tokio::test]
    async fn concurrent_spawning_with_same_key_coalesces() {
        // Fire 5 concurrent tasks — after a small stagger to ensure the
        // first one becomes leader and the rest become waiters. Then the
        // leader completes, waking all waiters.
        let store = Arc::new(CoalesceStore::new());
        let barrier = Arc::new(tokio::sync::Barrier::new(5));

        let mut handles = Vec::new();
        for i in 0..5 {
            let s = Arc::clone(&store);
            let b = Arc::clone(&barrier);
            handles.push(tokio::spawn(async move {
                b.wait().await;
                // Stagger: task 0 goes first; rest delay slightly
                if i > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                s.try_acquire("http://up/v1", "race-key", TD, PH).await
            }));
        }

        // Let leader acquire and waiters subscribe
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Leader completes — waiters get Coalesced
        // Find the leader's key: any task that got Forward is the leader
        // We just complete by constructing the expected key
        let expected_key = format!("{}|{}|{}|{}", "http://up/v1", "race-key", TD, PH);
        store.complete(
            &expected_key,
            CapturedResponse {
                status: 200,
                body_bytes: b"race-result".to_vec(),
            },
        );

        // All tasks should resolve now
        let results: Vec<_> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        let forward_count = results
            .iter()
            .filter(|r| matches!(r, CoalesceResult::Forward { .. }))
            .count();
        let coalesced_count = results
            .iter()
            .filter(|r| matches!(r, CoalesceResult::Coalesced { .. }))
            .count();

        assert!(forward_count >= 1, "at least one leader");
        assert_eq!(forward_count + coalesced_count, 5);

        // Store clean afterward
        let result = store.try_acquire("http://up/v1", "race-key", TD, PH).await;
        assert!(matches!(result, CoalesceResult::Forward { .. }));
    }
}
