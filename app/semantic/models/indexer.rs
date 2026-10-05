//! One idle embedding pool per watcher; ordinary indexing retains no models.
use super::EmbeddingPool;
use std::cell::RefCell;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct AssetsKey {
    pub(super) root: PathBuf,
    pub(super) marker_modified: Option<SystemTime>,
    pub(super) files: Vec<(String, u64, Option<SystemTime>)>,
}

#[derive(Clone, Default)]
pub(crate) struct Cache(Arc<Mutex<Option<(AssetsKey, EmbeddingPool)>>>);

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

pub(crate) async fn scope<T>(work: impl Future<Output = T>) -> T {
    let cache = Cache::default();
    let result = ASYNC_CACHE.scope(cache.clone(), work).await;
    // Destruction joins model resources; keep normal watcher shutdown off Tokio.
    let _ = tokio::task::spawn_blocking(move || drop(cache)).await;
    result
}

impl Cache {
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
            Some((cached_key, pool)) if cached_key == *key => Some(pool),
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
    pub(super) fn uncached(pool: EmbeddingPool) -> Self {
        Self {
            pool: Some(pool),
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
                .replace((key, pool));
            drop(previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::{Models, embeddings::EmbeddingPoolBackend, remote};

    fn key() -> AssetsKey {
        AssetsKey {
            root: PathBuf::from("/installed/generation-a"),
            marker_modified: Some(SystemTime::UNIX_EPOCH),
            files: vec![("model.onnx".to_owned(), 42, Some(SystemTime::UNIX_EPOCH))],
        }
    }

    fn remote_config() -> remote::Configuration {
        remote::Configuration {
            url: "http://127.0.0.1:1/embeddings".to_owned(),
            model: "test".to_owned(),
            dimensions: 384,
            api: remote::Api::OpenaiCompatible,
            token_env: None,
            request_dimensions: false,
        }
    }

    // A client without requests exercises ownership on either inference platform.
    fn pool() -> EmbeddingPool {
        EmbeddingPool {
            backend: EmbeddingPoolBackend::Remote(remote::Client::new(remote_config()).unwrap()),
            assets_lease: None,
        }
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
    async fn missing_local_assets_and_remote_configuration_clear_retained_local_resources() {
        let directory = tempfile::tempdir().unwrap();
        scope(async {
            crate::blocking(move || {
                let cache = current().unwrap();
                drop(LoadedIndexer::new(pool(), key(), Some(cache.clone())));
                let models = Models::new(directory.path().to_path_buf());
                assert!(models.load_indexer()?.is_none());
                assert!(cache.take(&key()).is_none());
                drop(LoadedIndexer::new(pool(), key(), Some(cache.clone())));
                drop(
                    models
                        .with_remote(Some(remote_config()))
                        .load_indexer()?
                        .unwrap(),
                );
                assert!(cache.take(&key()).is_none());
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
                    EmbeddingPoolBackend::Remote(_) => panic!("requires local inference"),
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
