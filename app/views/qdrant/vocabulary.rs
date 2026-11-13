//! The lexical matching vocabulary, kept between searches.
//!
//! Prefix and typo matching read every distinct indexed word. That is megabytes
//! of terms and the slowest read of a word search, so a store keeps the list
//! for a bounded time. A local index also keeps it in a small file beside its
//! database, so one-shot searches in separate processes share it.
//!
//! A kept list only proposes alternatives. The words as typed are always
//! queried live, and every alternative is matched against the current
//! documents, so an older list can narrow prefix and typo expansion but never
//! adds a result the current views lack.
use super::{Scope, Store, api, collection, views::views_id};
use crate::AppError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Distinct filters whose vocabularies are kept, most recently loaded last.
const ENTRIES: usize = 4;
/// One-shot searches mostly share one filter; the file keeps the newest few.
const FILE_ENTRIES: usize = 2;
const FILE_VERSION: u32 = 1;
/// Room for the kept vocabularies at their size limit; a larger file is ignored.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Words added since a vocabulary was read are not offered as alternatives,
/// and removed words are offered but match nothing, until it is read again.
/// A watcher certifies a new revision every minute or two, so a new revision
/// alone does not reread it; this bounds how long any change goes unseen.
const MAX_AGE: Duration = Duration::from_secs(300);

/// The file a local index keeps its matching vocabulary in.
pub(crate) fn file_beside(database: &Path) -> PathBuf {
    let mut name = database.as_os_str().to_owned();
    name.push(".vocabulary.json");
    PathBuf::from(name)
}

/// A node's views state, as far as a kept vocabulary depends on it.
#[derive(Deserialize)]
struct Basis {
    node_id: String,
    #[serde(default)]
    certified: bool,
    #[serde(default)]
    input: Option<i64>,
    #[serde(default)]
    build: Option<crate::views::config::Build>,
    #[serde(default)]
    generation: Option<String>,
}

/// Which node, built how: a kept vocabulary is reused only while these match.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Built {
    node_id: String,
    build: crate::views::config::Build,
    generation: String,
}

#[derive(Clone)]
struct Entry {
    scope: String,
    nodes: Vec<Built>,
    /// When the read began, in Unix milliseconds.
    loaded: u64,
    terms: Arc<Vec<String>>,
}

impl Entry {
    fn fresh(&self, now: u64) -> bool {
        now.checked_sub(self.loaded)
            .is_some_and(|age| u128::from(age) < MAX_AGE.as_millis())
    }
}

#[derive(Serialize)]
struct FileOut<'a> {
    version: u32,
    entries: Vec<EntryOut<'a>>,
}
#[derive(Serialize)]
struct EntryOut<'a> {
    scope: &'a str,
    nodes: &'a [Built],
    loaded: u64,
    terms: &'a [String],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileIn {
    version: u32,
    entries: Vec<EntryIn>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryIn {
    scope: String,
    nodes: Vec<Built>,
    loaded: u64,
    terms: Vec<String>,
}

#[derive(Default)]
struct Kept {
    entries: Vec<Entry>,
    file: Option<PathBuf>,
    /// The file has been merged in (or there is none to read).
    read: bool,
}

#[derive(Clone, Default)]
pub(super) struct Vocabulary(Arc<Mutex<Kept>>);

impl Vocabulary {
    pub(super) fn persisted(file: PathBuf) -> Self {
        Self(Arc::new(Mutex::new(Kept {
            file: Some(file),
            ..Kept::default()
        })))
    }

    fn get(&self, scope: &str, nodes: &[Built], now: u64) -> Option<Arc<Vec<String>>> {
        let kept = self.0.lock().ok()?;
        kept.entries
            .iter()
            .find(|entry| entry.scope == scope && entry.nodes == nodes && entry.fresh(now))
            .map(|entry| Arc::clone(&entry.terms))
    }

    fn put(&self, entry: Entry) {
        let Ok(mut kept) = self.0.lock() else {
            return;
        };
        kept.entries.retain(|kept| kept.scope != entry.scope);
        if kept.entries.len() == ENTRIES {
            kept.entries.remove(0);
        }
        kept.entries.push(entry);
    }

    /// Whether an entry for this filter was dropped.
    fn discard(&self, scope: &str) -> bool {
        let Ok(mut kept) = self.0.lock() else {
            return false;
        };
        let before = kept.entries.len();
        kept.entries.retain(|entry| entry.scope != scope);
        kept.entries.len() != before
    }

    /// The file to merge, the first time only.
    fn unread(&self) -> Option<PathBuf> {
        let mut kept = self.0.lock().ok()?;
        if kept.read {
            return None;
        }
        kept.read = true;
        kept.file.clone()
    }

    /// Add file entries for filters this process has not read itself.
    fn merge(&self, entries: Vec<Entry>) {
        let Ok(mut kept) = self.0.lock() else {
            return;
        };
        for entry in entries {
            if !kept.entries.iter().any(|kept| kept.scope == entry.scope) {
                kept.entries.push(entry);
            }
        }
        kept.entries.sort_by_key(|entry| entry.loaded);
        let excess = kept.entries.len().saturating_sub(ENTRIES);
        kept.entries.drain(..excess);
    }

    /// The newest fresh entries, for writing to the file.
    fn snapshot(&self, now: u64) -> Option<(PathBuf, Vec<Entry>)> {
        let kept = self.0.lock().ok()?;
        let file = kept.file.clone()?;
        let fresh = kept
            .entries
            .iter()
            .filter(|entry| entry.fresh(now))
            .cloned()
            .collect::<Vec<_>>();
        let skip = fresh.len().saturating_sub(FILE_ENTRIES);
        Some((file, fresh.into_iter().skip(skip).collect()))
    }

    async fn load(&self) {
        if let Some(file) = self.unread()
            && let Ok(entries) = crate::blocking(move || Ok(read_file(&file))).await
        {
            self.merge(entries);
        }
    }

    /// Best effort: a search never fails because its cache cannot be written.
    async fn save(&self) {
        if let Some((file, entries)) = self.snapshot(now()) {
            let _ = crate::blocking(move || write_file(&file, &entries)).await;
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Entries from a well-formed file of this version; anything else is ignored.
fn read_file(path: &Path) -> Vec<Entry> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_FILE_BYTES
    {
        return Vec::new();
    }
    let Ok(stored) = serde_json::from_slice::<FileIn>(&bytes) else {
        return Vec::new();
    };
    if stored.version != FILE_VERSION {
        return Vec::new();
    }
    stored
        .entries
        .into_iter()
        .filter(|entry| {
            !entry.scope.is_empty()
                && !entry.nodes.is_empty()
                && entry.terms.len() <= crate::views::matching::VOCABULARY_LIMIT
                && entry.terms.iter().all(|term| !term.is_empty())
        })
        .map(|entry| Entry {
            scope: entry.scope,
            nodes: entry.nodes,
            loaded: entry.loaded,
            terms: Arc::new(entry.terms),
        })
        .collect()
}

/// Replace the file in one rename, readable only by its owner.
fn write_file(path: &Path, entries: &[Entry]) -> Result<(), AppError> {
    let bytes = serde_json::to_vec(&FileOut {
        version: FILE_VERSION,
        entries: entries
            .iter()
            .map(|entry| EntryOut {
                scope: &entry.scope,
                nodes: &entry.nodes,
                loaded: entry.loaded,
                terms: &entry.terms,
            })
            .collect(),
    })
    .map_err(|_| AppError::internal("cannot encode matching vocabulary"))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| AppError::internal("matching vocabulary has no directory"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(AppError::io)?;
    file.write_all(&bytes).map_err(AppError::io)?;
    file.persist(path).map_err(|e| AppError::io(e.error))?;
    Ok(())
}

impl Store {
    /// Use and keep the matching vocabulary in this file, beside a local index.
    pub(crate) fn with_vocabulary_file(mut self, file: PathBuf) -> Self {
        self.vocabulary = Vocabulary::persisted(file);
        self
    }

    /// Every distinct indexed word the filter admits, or a recent list of them.
    pub(super) async fn vocabulary(
        &self,
        identity: &Scope,
        active: &BTreeSet<String>,
        filter: &api::Filter,
    ) -> Result<Arc<Vec<String>>, AppError> {
        // A kept list is dated from before its read began, never after.
        let started = now();
        let name = collection(&identity.group_id, false);
        let scope = format!(
            "{name}\n{}",
            serde_json::to_string(filter)
                .map_err(|_| AppError::internal("cannot encode vocabulary filter"))?
        );
        let ((), nodes) = tokio::join!(self.vocabulary.load(), self.certified(identity, active));
        let Some(nodes) = nodes? else {
            // Its documents can change without notice: read the words for
            // this search alone, and read them again once it is certified.
            if self.vocabulary.discard(&scope) {
                self.vocabulary.save().await;
            }
            return Ok(Arc::new(self.api.terms(&name, filter).await?));
        };
        if let Some(terms) = self.vocabulary.get(&scope, &nodes, started) {
            return Ok(terms);
        }
        let terms = Arc::new(self.api.terms(&name, filter).await?);
        self.vocabulary.put(Entry {
            scope,
            nodes,
            loaded: started,
            terms: Arc::clone(&terms),
        });
        self.vocabulary.save().await;
        Ok(terms)
    }

    /// None while any searched node is being rebuilt or has never been
    /// certified: its documents can change without notice, so nothing is kept.
    async fn certified(
        &self,
        identity: &Scope,
        active: &BTreeSet<String>,
    ) -> Result<Option<Vec<Built>>, AppError> {
        let ids = active.iter().map(|node| views_id(node)).collect::<Vec<_>>();
        let mut nodes = self
            .api
            .retrieve_fields::<Basis>(
                &collection(&identity.group_id, true),
                &ids,
                &["node_id", "certified", "input", "build", "generation"],
            )
            .await?
            .into_iter()
            .map(|point| point.payload)
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        if nodes.len() != active.len() {
            return Ok(None);
        }
        Ok(nodes
            .into_iter()
            .zip(active)
            .map(|(node, id)| match node {
                Basis {
                    node_id,
                    certified: true,
                    input: Some(_),
                    build: Some(build),
                    generation: Some(generation),
                } if node_id == *id => Some(Built {
                    node_id,
                    build,
                    generation,
                }),
                _ => None,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, routing::post};
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};

    /// Views state for every node: certified for `revision` (negative: being
    /// rebuilt), built per message or with parents; and a counted word read.
    #[derive(Default)]
    struct Backend {
        revision: AtomicI64,
        parents: AtomicBool,
        reads: AtomicUsize,
    }

    impl Backend {
        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    async fn serve(backend: &Arc<Backend>) -> String {
        let (views, words) = (Arc::clone(backend), Arc::clone(backend));
        let router = Router::new()
            .route(
                &format!("/collections/{}/points", collection("test", true)),
                post(move |Json(body): Json<serde_json::Value>| async move {
                    let revision = views.revision.load(Ordering::SeqCst);
                    let build = if views.parents.load(Ordering::SeqCst) {
                        serde_json::json!({"retrieval_unit": "parent-and-message"})
                    } else {
                        serde_json::json!({})
                    };
                    let points = ["node", "other"]
                        .into_iter()
                        .filter(|node| {
                            body["ids"]
                                .as_array()
                                .is_some_and(|ids| ids.iter().any(|id| *id == views_id(node)))
                        })
                        .map(|node| {
                            serde_json::json!({"id": views_id(node), "payload": {
                                "node_id": node,
                                "certified": revision >= 0,
                                "input": revision.abs(),
                                "build": build,
                                "generation": "unused",
                            }})
                        })
                        .collect::<Vec<_>>();
                    Json(serde_json::json!({"status": "ok", "result": points}))
                }),
            )
            .route(
                &format!("/collections/{}/facet", collection("test", false)),
                post(move || async move {
                    words.reads.fetch_add(1, Ordering::SeqCst);
                    r#"{"status":"ok","result":{"hits":[{"value":"bun","count":1}]}}"#
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        url
    }

    fn open(url: &str, file: Option<&Path>) -> Store {
        let mut store = Store::unavailable();
        store.api = api::Client::new(url, None, 64 * 1024).unwrap();
        match file {
            Some(file) => store.with_vocabulary_file(file.to_path_buf()),
            None => store,
        }
    }

    async fn read(store: &Store, active: &[&str], filter: &api::Filter) -> Arc<Vec<String>> {
        let identity = Scope {
            group_id: "test".into(),
            node_id: "node".into(),
            generation: "unused".into(),
            dimensions: 4,
        };
        let active = active.iter().map(|node| (*node).to_owned()).collect();
        store.vocabulary(&identity, &active, filter).await.unwrap()
    }

    /// Make every kept vocabulary as old as the reuse bound.
    fn age(store: &Store) {
        for entry in &mut store.vocabulary.0.lock().unwrap().entries {
            entry.loaded -= u64::try_from(MAX_AGE.as_millis()).unwrap();
        }
    }

    #[tokio::test]
    async fn vocabulary_is_kept_across_revisions_until_it_ages() {
        let backend = Arc::new(Backend::default());
        backend.revision.store(7, Ordering::SeqCst);
        let store = open(&serve(&backend).await, None);
        let all = api::Filter::default();
        assert_eq!(*read(&store, &["node"], &all).await, ["bun"]);
        // The watcher certifies a later revision; the kept words still serve.
        backend.revision.store(8, Ordering::SeqCst);
        assert_eq!(*read(&store, &["node"], &all).await, ["bun"]);
        assert_eq!(backend.reads(), 1);

        age(&store);
        read(&store, &["node"], &all).await;
        read(&store, &["node"], &all).await;
        assert_eq!(backend.reads(), 2);
    }

    #[tokio::test]
    async fn vocabulary_is_reread_when_filter_nodes_settings_or_certification_change() {
        let backend = Arc::new(Backend::default());
        backend.revision.store(7, Ordering::SeqCst);
        let store = open(&serve(&backend).await, None);
        let all = api::Filter::default();
        read(&store, &["node"], &all).await;
        assert_eq!(backend.reads(), 1);

        // Another filter admits other documents, so it has its own words.
        let mut narrowed = all.clone();
        narrowed
            .must
            .push(api::matches("tokens", vec!["bun".into()]));
        read(&store, &["node"], &narrowed).await;
        assert_eq!(backend.reads(), 2);

        // Another node contributes its own words, and the list for one node
        // set never serves another.
        read(&store, &["node", "other"], &all).await;
        read(&store, &["node", "other"], &all).await;
        assert_eq!(backend.reads(), 3);
        read(&store, &["node"], &all).await;
        read(&store, &["node"], &all).await;
        assert_eq!(backend.reads(), 4);

        // Documents built another way hold other words.
        backend.parents.store(true, Ordering::SeqCst);
        read(&store, &["node"], &all).await;
        read(&store, &["node"], &all).await;
        assert_eq!(backend.reads(), 5);

        // Nothing is kept or reused while the node is being rebuilt, and what
        // was kept before is read again once it is certified.
        backend.revision.store(-8, Ordering::SeqCst);
        read(&store, &["node"], &all).await;
        read(&store, &["node"], &all).await;
        assert_eq!(backend.reads(), 7);
        backend.revision.store(8, Ordering::SeqCst);
        read(&store, &["node"], &all).await;
        read(&store, &["node"], &all).await;
        assert_eq!(backend.reads(), 8);
    }

    #[tokio::test]
    async fn vocabulary_file_serves_later_processes_and_is_ignored_when_unusable() {
        let backend = Arc::new(Backend::default());
        backend.revision.store(7, Ordering::SeqCst);
        let url = serve(&backend).await;
        let directory = tempfile::tempdir().unwrap();
        let file = file_beside(&directory.path().join("mash.sqlite3"));
        let all = api::Filter::default();

        read(&open(&url, Some(&file)), &["node"], &all).await;
        assert_eq!(backend.reads(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "the kept words are private");
        }
        // A new store, as a new process opens it, after a new certification.
        backend.revision.store(8, Ordering::SeqCst);
        assert_eq!(
            *read(&open(&url, Some(&file)), &["node"], &all).await,
            ["bun"]
        );
        assert_eq!(backend.reads(), 1);

        // An aged file is not reused, and is replaced.
        let mut stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        stored["entries"][0]["loaded"] = serde_json::json!(now() - 301_000);
        std::fs::write(&file, serde_json::to_vec(&stored).unwrap()).unwrap();
        read(&open(&url, Some(&file)), &["node"], &all).await;
        read(&open(&url, Some(&file)), &["node"], &all).await;
        assert_eq!(backend.reads(), 2);

        // Unreadable, foreign or future-version files are ignored and replaced.
        let mut reads = 2;
        for unusable in [
            b"not json".as_slice(),
            br#"{"version":999,"entries":[]}"#,
            br#"{"version":1,"entries":[{"scope":"x"}]}"#,
        ] {
            std::fs::write(&file, unusable).unwrap();
            assert_eq!(
                *read(&open(&url, Some(&file)), &["node"], &all).await,
                ["bun"]
            );
            reads += 1;
            assert_eq!(backend.reads(), reads);
            read(&open(&url, Some(&file)), &["node"], &all).await;
            assert_eq!(backend.reads(), reads);
        }
    }
}
