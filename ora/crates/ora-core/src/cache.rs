//! A small in-memory cache for Web API reads (DO_NEXT §5, Phase 6 TTLs).
//!
//! Rules:
//! - Concurrent identical requests share one fetch: callers for a key wait behind it.
//! - Failures are never cached; the next caller tries again.
//! - Entries belong to a scope (the sign-in generation and account). A new scope empties the
//!   cache, so one account never sees another's data.

use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// More keys than this and the cache starts again; each key is one page of one list.
const MAX_KEYS: usize = 512;

type Slot<V> = Arc<tokio::sync::Mutex<Option<(V, Instant)>>>;

struct Entries<V> {
    scope: String,
    slots: HashMap<String, Slot<V>>,
}

pub struct Cache<V> {
    ttl: Duration,
    entries: Mutex<Entries<V>>,
}

impl<V: Clone> Cache<V> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::new(Entries {
                scope: String::new(),
                slots: HashMap::new(),
            }),
        }
    }

    fn slot(&self, scope: &str, key: &str) -> Slot<V> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if entries.scope != scope || entries.slots.len() >= MAX_KEYS {
            entries.scope = scope.to_owned();
            entries.slots.clear();
        }
        Arc::clone(entries.slots.entry(key.to_owned()).or_default())
    }

    /// The cached value for `key` while it's fresh, otherwise the result of `fetch`.
    pub async fn get<E, Fut>(
        &self,
        scope: &str,
        key: &str,
        fetch: impl FnOnce() -> Fut,
    ) -> Result<V, E>
    where
        Fut: Future<Output = Result<V, E>>,
    {
        let slot = self.slot(scope, key);
        let mut held = slot.lock().await;
        if let Some((value, at)) = held.as_ref() {
            if at.elapsed() < self.ttl {
                return Ok(value.clone());
            }
        }
        let value = fetch().await?;
        *held = Some((value.clone(), Instant::now()));
        Ok(value)
    }

    /// Forgets everything, for example after the list changed.
    pub fn clear(&self) {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .slots
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn concurrent_callers_share_one_fetch_and_reuse_it() {
        let cache = Arc::new(Cache::<u32>::new(Duration::from_secs(60)));
        let fetches = Arc::new(AtomicUsize::new(0));
        let calls: Vec<_> = (0..5)
            .map(|_| {
                let (cache, fetches) = (Arc::clone(&cache), Arc::clone(&fetches));
                tokio::spawn(async move {
                    cache
                        .get("a", "k", || async {
                            fetches.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(20)).await;
                            Ok::<_, ()>(7)
                        })
                        .await
                })
            })
            .collect();
        for call in calls {
            assert_eq!(call.await.unwrap(), Ok(7));
        }
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failures_expiry_scope_and_clear_fetch_again() {
        let cache = Cache::<u32>::new(Duration::from_millis(30));
        assert_eq!(
            cache.get("a", "k", || async { Err::<u32, _>("no") }).await,
            Err("no")
        );
        assert_eq!(
            cache.get("a", "k", || async { Ok::<_, ()>(1) }).await,
            Ok(1)
        );
        assert_eq!(
            cache.get("a", "k", || async { Ok::<_, ()>(2) }).await,
            Ok(1)
        );
        // Another account's scope never sees the first one's value.
        assert_eq!(
            cache.get("b", "k", || async { Ok::<_, ()>(3) }).await,
            Ok(3)
        );
        cache.clear();
        assert_eq!(
            cache.get("b", "k", || async { Ok::<_, ()>(4) }).await,
            Ok(4)
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(
            cache.get("b", "k", || async { Ok::<_, ()>(5) }).await,
            Ok(5)
        );
    }
}
