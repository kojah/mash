//! The lexical matching vocabulary, kept between searches by a long-lived store.
//!
//! Prefix and typo matching read every distinct indexed word. That is megabytes
//! of terms, so a store keeps the list until the nodes it covers are certified
//! for a different silver revision.
use super::{Scope, Store, api, collection, views::views_id};
use crate::AppError;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Distinct filters whose vocabularies are kept, most recently loaded last.
const ENTRIES: usize = 4;
/// A synchronization rewrites documents before it certifies a new revision,
/// and searches keep serving the certified one meanwhile. Until then a kept
/// vocabulary can lack words that are being added, which only narrows prefix
/// and typo expansion: the words as typed are always queried live. This bounds
/// that window, and any change that leaves the revision alone.
const MAX_AGE: Duration = Duration::from_secs(300);

/// What a node's views were certified for when a vocabulary was read.
#[derive(Clone, Deserialize, PartialEq, Eq)]
struct Basis {
    node_id: String,
    #[serde(default)]
    certified: bool,
    #[serde(default)]
    input: Option<i64>,
}

struct Entry {
    scope: String,
    nodes: Vec<Basis>,
    loaded: Instant,
    terms: Arc<Vec<String>>,
}

#[derive(Clone, Default)]
pub(super) struct Vocabulary(Arc<Mutex<Vec<Entry>>>);

impl Vocabulary {
    fn get(&self, scope: &str, nodes: &[Basis]) -> Option<Arc<Vec<String>>> {
        let entries = self.0.lock().ok()?;
        entries
            .iter()
            .find(|entry| {
                entry.scope == scope && entry.nodes == nodes && entry.loaded.elapsed() < MAX_AGE
            })
            .map(|entry| Arc::clone(&entry.terms))
    }

    fn put(&self, scope: String, nodes: Vec<Basis>, terms: &Arc<Vec<String>>) {
        let Ok(mut entries) = self.0.lock() else {
            return;
        };
        entries.retain(|entry| entry.scope != scope);
        if entries.len() == ENTRIES {
            entries.remove(0);
        }
        entries.push(Entry {
            scope,
            nodes,
            loaded: Instant::now(),
            terms: Arc::clone(terms),
        });
    }
}

impl Store {
    /// Every distinct indexed word the filter admits.
    pub(super) async fn vocabulary(
        &self,
        identity: &Scope,
        active: &BTreeSet<String>,
        filter: &api::Filter,
    ) -> Result<Arc<Vec<String>>, AppError> {
        let name = collection(&identity.group_id, false);
        // Read what is certified first: a certification that lands during the
        // word read leaves an entry the next search discards, never a stale one.
        let Some(nodes) = self.certified(identity, active).await? else {
            return Ok(Arc::new(self.api.terms(&name, filter).await?));
        };
        let scope = format!(
            "{name}\n{}",
            serde_json::to_string(filter)
                .map_err(|_| AppError::internal("cannot encode vocabulary filter"))?
        );
        if let Some(terms) = self.vocabulary.get(&scope, &nodes) {
            return Ok(terms);
        }
        let terms = Arc::new(self.api.terms(&name, filter).await?);
        self.vocabulary.put(scope, nodes, &terms);
        Ok(terms)
    }

    /// None while any searched node is being rebuilt or has never been
    /// certified: its documents can change without notice, so nothing is kept.
    async fn certified(
        &self,
        identity: &Scope,
        active: &BTreeSet<String>,
    ) -> Result<Option<Vec<Basis>>, AppError> {
        let ids = active.iter().map(|node| views_id(node)).collect::<Vec<_>>();
        let mut nodes = self
            .api
            .retrieve_fields::<Basis>(
                &collection(&identity.group_id, true),
                &ids,
                &["node_id", "certified", "input"],
            )
            .await?
            .into_iter()
            .map(|point| point.payload)
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        let complete = nodes.len() == active.len()
            && nodes
                .iter()
                .zip(active)
                .all(|(node, id)| node.node_id == *id && node.certified && node.input.is_some());
        Ok(complete.then_some(nodes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::post};
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

    /// A node certified for `revision` (negative: being rebuilt) and a counted
    /// word read.
    async fn store(revision: &Arc<AtomicI64>, reads: &Arc<AtomicUsize>) -> (Store, Scope) {
        let identity = Scope {
            group_id: "test".into(),
            node_id: "node".into(),
            generation: "unused".into(),
            dimensions: 4,
        };
        let (revision, reads) = (Arc::clone(revision), Arc::clone(reads));
        let router = Router::new()
            .route(
                &format!("/collections/{}/points", collection("test", true)),
                post(move || async move {
                    let revision = revision.load(Ordering::SeqCst);
                    format!(
                        r#"{{"status":"ok","result":[{{"id":"{}","payload":{{"node_id":"node","certified":{},"input":{}}}}}]}}"#,
                        views_id("node"),
                        revision >= 0,
                        revision.abs()
                    )
                }),
            )
            .route(
                &format!("/collections/{}/facet", collection("test", false)),
                post(move || async move {
                    reads.fetch_add(1, Ordering::SeqCst);
                    r#"{"status":"ok","result":{"hits":[{"value":"bun","count":1}]}}"#
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut store = Store::unavailable();
        store.api = api::Client::new(&url, None, 64 * 1024).unwrap();
        (store, identity)
    }

    #[tokio::test]
    async fn vocabulary_is_reread_only_when_certification_or_the_filter_changes() {
        let (revision, reads) = (Arc::new(AtomicI64::new(7)), Arc::new(AtomicUsize::new(0)));
        let (store, identity) = store(&revision, &reads).await;
        let active = BTreeSet::from(["node".to_owned()]);
        let all = api::Filter::default();
        let read = |filter: api::Filter| {
            let (store, identity, active) = (store.clone(), identity.clone(), active.clone());
            async move { store.vocabulary(&identity, &active, &filter).await.unwrap() }
        };
        assert_eq!(*read(all.clone()).await, ["bun"]);
        assert_eq!(*read(all.clone()).await, ["bun"]);
        assert_eq!(reads.load(Ordering::SeqCst), 1);

        // Another filter admits other documents, so it has its own words.
        let mut narrowed = all.clone();
        narrowed
            .must
            .push(api::matches("tokens", vec!["bun".into()]));
        read(narrowed).await;
        assert_eq!(reads.load(Ordering::SeqCst), 2);

        // A newly certified revision replaces the kept words.
        revision.store(8, Ordering::SeqCst);
        read(all.clone()).await;
        read(all.clone()).await;
        assert_eq!(reads.load(Ordering::SeqCst), 3);

        // Nothing is kept or reused while the node is being rebuilt.
        revision.store(-8, Ordering::SeqCst);
        read(all.clone()).await;
        read(all.clone()).await;
        assert_eq!(reads.load(Ordering::SeqCst), 5);
        revision.store(8, Ordering::SeqCst);
        read(all).await;
        assert_eq!(reads.load(Ordering::SeqCst), 5);
    }
}
