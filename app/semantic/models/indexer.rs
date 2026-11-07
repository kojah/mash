//! One expiring idle embedding pool per watcher; ordinary indexing retains no models.
use super::EmbeddingPool;
use std::cell::RefCell;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

#[cfg(all(test, target_os = "linux"))]
mod memory;

struct CachedIndexer {
    key: AssetsKey,
    pool: EmbeddingPool,
    idle_since: Instant,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct AssetsKey {
    pub(super) root: PathBuf,
    pub(super) marker_modified: Option<SystemTime>,
    pub(super) files: Vec<(String, u64, Option<SystemTime>)>,
}

#[derive(Clone, Default)]
pub(crate) struct Cache(Arc<Mutex<Option<CachedIndexer>>>);

tokio::task_local! { static ASYNC_CACHE: Cache; }
thread_local! { static BLOCKING_CACHE: RefCell<Option<Cache>> = const { RefCell::new(None) }; }

pub(crate) fn current() -> Option<Cache> {
    ASYNC_CACHE
        .try_with(Clone::clone)
        .ok()
        .or_else(|| BLOCKING_CACHE.with(|cache| cache.borrow().clone()))
}

pub(crate) fn on_blocking_thread<T>(cache: Option<Cache>, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<Cache>);
    impl Drop for Restore {
        fn drop(&mut self) {
            BLOCKING_CACHE.with(|cache| *cache.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(BLOCKING_CACHE.with(|current| current.replace(cache)));
    work()
}

pub(crate) fn scope<T>(work: impl Future<Output = T>) -> impl Future<Output = T> {
    scope_with_idle_timeout(Box::pin(work), IDLE_TIMEOUT)
}

async fn scope_with_idle_timeout<T>(work: impl Future<Output = T>, idle_timeout: Duration) -> T {
    let cache = Cache::default();
    let result = ASYNC_CACHE
        .scope(cache.clone(), async {
            tokio::pin!(work);
            loop {
                tokio::select! {
                    result = &mut work => break result,
                    () = tokio::time::sleep(idle_timeout / 2) => {
                        let cache = cache.clone();
                        // Model destruction can join inference threads; never do it
                        // on the async runtime or while holding the cache lock.
                        let _ = tokio::task::spawn_blocking(move || cache.expire(idle_timeout)).await;
                    }
                }
            }
        })
        .await;
    // Destruction joins model resources; keep normal watcher shutdown off Tokio.
    let _ = tokio::task::spawn_blocking(move || drop(cache)).await;
    result
}

impl Cache {
    fn expire(&self, idle_timeout: Duration) {
        let expired = {
            let mut cached = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if cached
                .as_ref()
                .is_some_and(|entry| entry.idle_since.elapsed() >= idle_timeout)
            {
                cached.take()
            } else {
                None
            }
        };
        drop(expired);
    }

    pub(super) fn clear(&self) {
        let previous = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(previous);
    }

    pub(super) fn take(&self, key: &AssetsKey) -> Option<EmbeddingPool> {
        let cached = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        match cached {
            Some(cached) if cached.key == *key => Some(cached.pool),
            // Drop mismatched assets outside the lock, never reuse another model.
            _ => None,
        }
    }
}

pub(crate) struct LoadedIndexer {
    pool: Option<EmbeddingPool>,
    cache: Option<(AssetsKey, Cache)>,
}

impl LoadedIndexer {
    #[cfg(test)]
    pub(crate) fn fixture(state: std::sync::Arc<crate::semantic::test_embeddings::State>) -> Self {
        Self {
            pool: Some(EmbeddingPool {
                backend: crate::semantic::embeddings::EmbeddingPoolBackend::Fixture(state),
                assets_lease: None,
            }),
            cache: None,
        }
    }
    pub(super) fn new(pool: EmbeddingPool, key: AssetsKey, cache: Option<Cache>) -> Self {
        Self {
            pool: Some(pool),
            cache: cache.map(|cache| (key, cache)),
        }
    }
}

impl Deref for LoadedIndexer {
    type Target = EmbeddingPool;

    fn deref(&self) -> &Self::Target {
        self.pool.as_ref().expect("indexer is present until drop")
    }
}

impl DerefMut for LoadedIndexer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.pool.as_mut().expect("indexer is present until drop")
    }
}

impl Drop for LoadedIndexer {
    fn drop(&mut self) {
        if let (Some((key, cache)), Some(pool)) = (self.cache.take(), self.pool.take()) {
            let previous = cache
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .replace(CachedIndexer {
                    key,
                    pool,
                    idle_since: Instant::now(),
                });
            drop(previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::{Models, embeddings::EmbeddingPoolBackend};

    fn key() -> AssetsKey {
        AssetsKey {
            root: PathBuf::from("/installed/generation-a"),
            marker_modified: Some(SystemTime::UNIX_EPOCH),
            files: vec![("model.onnx".to_owned(), 42, Some(SystemTime::UNIX_EPOCH))],
        }
    }

    // A deterministic pool exercises ownership without loading model assets.
    fn pool() -> EmbeddingPool {
        let fixture = crate::semantic::test_embeddings::Fixture::new(384);
        EmbeddingPool {
            backend: EmbeddingPoolBackend::Fixture(std::sync::Arc::clone(&fixture.state)),
            assets_lease: None,
        }
    }

    #[test]
    fn idle_expiry_preserves_checked_out_models_and_resets_on_return() {
        let cache = Cache::default();
        drop(LoadedIndexer::new(pool(), key(), Some(cache.clone())));
        cache.expire(IDLE_TIMEOUT);
        let checked_out = cache.take(&key()).expect("warm pool survives");
        cache.expire(Duration::ZERO);
        drop(LoadedIndexer::new(checked_out, key(), Some(cache.clone())));
        cache.expire(IDLE_TIMEOUT);
        assert!(
            cache.take(&key()).is_some(),
            "return starts a new idle period"
        );
        drop(LoadedIndexer::new(pool(), key(), Some(cache.clone())));
        cache.expire(Duration::ZERO);
        assert!(cache.take(&key()).is_none());
    }

    #[tokio::test]
    async fn watcher_releases_idle_assets_without_another_index_pass() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("asset-lease");
        let collector = std::fs::File::create(&path).unwrap();
        scope_with_idle_timeout(
            async {
                let reader = std::fs::File::open(&path).unwrap();
                reader.lock_shared().unwrap();
                let mut retained = pool();
                retained.assets_lease = Some(reader);
                drop(LoadedIndexer::new(retained, key(), current()));
                assert!(matches!(
                    collector.try_lock(),
                    Err(std::fs::TryLockError::WouldBlock)
                ));
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        if collector.try_lock().is_ok() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .expect("idle pool releases its asset lease while watcher remains alive");
                assert!(current().unwrap().take(&key()).is_none());
                // The next update can cache a fresh pool after expiry.
                drop(LoadedIndexer::new(pool(), key(), current()));
                assert!(current().unwrap().take(&key()).is_some());
            },
            Duration::from_millis(20),
        )
        .await;
    }

    #[test]
    fn checkout_is_exclusive_and_asset_changes_discard_idle_pool() {
        let cache = Cache::default();
        drop(LoadedIndexer::new(pool(), key(), Some(cache.clone())));
        let checked_out = cache.take(&key()).unwrap();
        assert!(cache.take(&key()).is_none());
        drop(LoadedIndexer::new(checked_out, key(), Some(cache.clone())));
        assert!(cache.take(&key()).is_some());

        let mut changes = vec![key(); 4];
        changes[0].root.push("different-generation");
        changes[1].marker_modified = None;
        changes[2].files[0].1 += 1;
        changes[3].files[0].2 = None;
        for changed in changes {
            drop(LoadedIndexer::new(pool(), key(), Some(cache.clone())));
            assert!(cache.take(&changed).is_none());
            assert!(cache.take(&key()).is_none());
        }
    }

    #[test]
    fn blocking_context_restores_after_nested_work_and_error() {
        let outer = Cache::default();
        let inner = Cache::default();
        assert!(current().is_none());
        on_blocking_thread(Some(outer.clone()), || {
            assert!(Arc::ptr_eq(&current().unwrap().0, &outer.0));
            let failure: Result<(), crate::AppError> =
                on_blocking_thread(Some(inner.clone()), || {
                    assert!(Arc::ptr_eq(&current().unwrap().0, &inner.0));
                    Err(crate::AppError::model("exercise context restoration"))
                });
            assert!(failure.is_err());
            assert!(Arc::ptr_eq(&current().unwrap().0, &outer.0));
        });
        assert!(current().is_none());
    }

    #[tokio::test]
    async fn watcher_scope_propagates_to_core_workers_and_releases_its_cache() {
        assert!(current().is_none());
        let weak = scope(async {
            let cache = current().unwrap();
            let weak = Arc::downgrade(&cache.0);
            crate::blocking(|| {
                let cache = current().expect("watcher cache reaches core work");
                drop(LoadedIndexer::new(pool(), key(), Some(cache)));
                Ok(())
            })
            .await
            .unwrap();
            assert!(cache.take(&key()).is_some());
            drop(LoadedIndexer::new(pool(), key(), Some(cache)));
            weak
        })
        .await;
        assert!(current().is_none());
        assert!(weak.upgrade().is_none());
        scope(async {
            assert!(current().unwrap().take(&key()).is_none());
        })
        .await;
    }

    #[tokio::test]
    async fn asset_lease_survives_idle_passes_and_is_released_when_watching_stops() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("asset-lease");
        let collector = std::fs::File::create(&path).unwrap();
        scope(async {
            let reader = std::fs::File::open(&path).unwrap();
            reader.lock_shared().unwrap();
            let mut retained = pool();
            retained.assets_lease = Some(reader);
            drop(LoadedIndexer::new(retained, key(), current()));
            assert!(matches!(
                collector.try_lock(),
                Err(std::fs::TryLockError::WouldBlock)
            ));
        })
        .await;
        collector
            .try_lock()
            .expect("stopped watcher releases asset lease");
    }

    #[tokio::test]
    async fn missing_local_assets_clear_retained_local_resources() {
        let directory = tempfile::tempdir().unwrap();
        scope(async {
            crate::blocking(move || {
                let cache = current().unwrap();
                drop(LoadedIndexer::new(pool(), key(), Some(cache.clone())));
                let models = Models::new(directory.path().to_path_buf());
                assert!(models.load_indexer()?.is_none());
                assert!(cache.take(&key()).is_none());
                drop(LoadedIndexer::new(pool(), key(), Some(cache.clone())));
                Ok(())
            })
            .await
            .unwrap();
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
    async fn real_watcher_reuses_same_pool_and_preserves_vectors_across_passes() {
        let root = std::env::var_os("MASH_TEST_MODELS_DIR")
            .map(PathBuf::from)
            .expect("explicit model installation");
        scope(async {
            crate::blocking(move || {
                let identity = |pool: &EmbeddingPool| match &pool.backend {
                    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
                    EmbeddingPoolBackend::Cpu(workers) => workers.as_ptr().cast::<()>(),
                    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
                    EmbeddingPoolBackend::CoreMl(pool) => {
                        std::ptr::from_ref(pool.as_ref()).cast::<()>()
                    }
                    EmbeddingPoolBackend::Fixture(_) => panic!("requires local inference"),
                };
                let models = Models::new(root);
                let longer =
                    "SQLite transactions, Qdrant indexing and debounced change notifications. "
                        .repeat(90);
                let texts = ["Brief status of local indexing.", longer.as_str()];
                let mut first = models.load_indexer()?.expect("installed models");
                let pointer = identity(&first);
                let vectors = first.shared_vectors(&texts)?;
                drop(first);
                let mut second = models.load_indexer()?.expect("installed models");
                assert_eq!(
                    identity(&second),
                    pointer,
                    "same allocated worker/session pool"
                );
                assert_eq!(
                    second.shared_vectors(&texts)?,
                    vectors,
                    "unchanged quantized vectors"
                );
                Ok(())
            })
            .await
            .unwrap();
        })
        .await;
    }
}
