//! Freeze evaluation identity from canonical text before derived topic work.
use super::{BTreeMap, BTreeSet, Mode, Thread, TopicId};
use rusqlite::Connection;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub(super) struct SourceId([u8; 32]);

#[derive(Default)]
struct Identity {
    texts: BTreeSet<[u8; 32]>,
    hash: Option<blake3::Hasher>,
}

impl Identity {
    fn push(&mut self, text: &str) {
        if !self.texts.insert(*blake3::hash(text.as_bytes()).as_bytes()) {
            return;
        }
        let hash = self.hash.get_or_insert_with(|| {
            let mut hash = blake3::Hasher::new();
            hash.update(b"mash-topic-canonical-user-text-v1\0");
            hash
        });
        hash.update(
            &u64::try_from(text.len())
                .expect("text length fits u64")
                .to_le_bytes(),
        );
        hash.update(text.as_bytes());
    }

    fn finish(self) -> Option<SourceId> {
        self.hash.map(|h| SourceId(*h.finalize().as_bytes()))
    }
}

#[cfg(test)]
pub(super) fn identity(texts: &[String]) -> Option<SourceId> {
    let mut source = Identity::default();
    for text in texts {
        source.push(text);
    }
    source.finish()
}

/// Include unembedded user records too. Embedding availability, projection,
/// provider, IDs and topic omissions must not decide the experiment partition.
pub(super) fn capture(connection: &Connection, threads: &mut BTreeMap<String, Thread>) {
    let mut statement = connection
        .prepare("SELECT conversation_id,content FROM messages WHERE role='user' ORDER BY conversation_id,ordinal,id")
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>("conversation_id")?,
                row.get::<_, String>("content")?,
            ))
        })
        .unwrap();
    let mut identities: BTreeMap<String, Identity> = BTreeMap::new();
    for row in rows {
        let (thread, text) = row.unwrap();
        if threads.contains_key(&thread) {
            identities.entry(thread).or_default().push(&text);
        }
    }
    for (thread, source) in identities {
        threads.get_mut(&thread).unwrap().source_id = source.finish();
    }
}

pub(super) fn held_out(thread: &Thread) -> bool {
    thread.source_id.is_some_and(|s| s.0[0].is_multiple_of(5))
}

#[derive(Serialize)]
pub(super) struct Summary {
    identity_version: &'static str,
    source_groups: usize,
    held_out_source_groups: usize,
    eligible_threads: usize,
    assigned_eligible_threads: usize,
    held_out_eligible_threads: usize,
    held_out_assigned_eligible_threads: usize,
    source_ids: BTreeMap<String, Option<SourceId>>,
}

pub(super) fn summarize(
    threads: &BTreeMap<String, Thread>,
    memberships: &BTreeMap<String, BTreeSet<TopicId>>,
    mode: Mode,
    holdout: bool,
) -> Summary {
    let eligible = |t: &Thread| match mode {
        Mode::ThreadMean | Mode::Segments => !t.vectors.is_empty(),
        _ => !t.user_messages.is_empty(),
    };
    let selected = |t: &Thread| holdout && held_out(t);
    let assigned = |id: &String| !memberships[id].is_empty();
    let groups: BTreeSet<_> = threads.values().filter_map(|t| t.source_id).collect();
    Summary {
        identity_version: "canonical_user_text_v1",
        source_groups: groups.len(),
        held_out_source_groups: groups
            .iter()
            .filter(|s| holdout && s.0[0].is_multiple_of(5))
            .count(),
        eligible_threads: threads.values().filter(|t| eligible(t)).count(),
        assigned_eligible_threads: threads
            .iter()
            .filter(|(id, t)| eligible(t) && assigned(id))
            .count(),
        held_out_eligible_threads: threads
            .values()
            .filter(|t| eligible(t) && selected(t))
            .count(),
        held_out_assigned_eligible_threads: threads
            .iter()
            .filter(|(id, t)| eligible(t) && selected(t) && assigned(id))
            .count(),
        source_ids: threads
            .iter()
            .map(|(id, t)| (id.clone(), t.source_id))
            .collect(),
    }
}

#[test]
fn canonical_identity_preserves_order_and_boundaries_without_vector_collisions() {
    let texts = |s: &[&str]| s.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        identity(&texts(&["a", "b", "a"])),
        identity(&texts(&["a", "b"]))
    );
    assert_ne!(identity(&texts(&["a", "b"])), identity(&texts(&["b", "a"])));
    assert_ne!(
        identity(&texts(&["a", "bc"])),
        identity(&texts(&["ab", "c"]))
    );
    assert_ne!(
        identity(&texts(&["Redis"])),
        identity(&texts(&["PostgreSQL"]))
    );
    assert_eq!(identity(&[]), None);
}

#[test]
fn projection_and_empty_evidence_do_not_change_partition_or_hide_denominators() {
    let mut original = super::anchored::thread(&[0]);
    original.source_id = (0..100).find_map(|i| {
        let id = identity(&[format!("canonical source {i}")]);
        id.filter(|id| id.0[0].is_multiple_of(5))
    });
    assert!(held_out(&original));
    original.user_sources =
        vec!["# AGENTS.md instructions for /example\n<INSTRUCTIONS>Git</INSTRUCTIONS>".into()];
    let mut copy = original.clone();
    copy.provider = crate::provider::Provider::ClaudeCode;
    let mut threads = BTreeMap::from([("original".into(), original), ("copy".into(), copy)]);
    let ids: Vec<_> = threads.values().map(|t| t.source_id).collect();
    super::input::clean(&mut threads, None).unwrap();
    assert_eq!(
        ids,
        threads.values().map(|t| t.source_id).collect::<Vec<_>>()
    );
    assert!(threads.values().all(held_out));
    let report = super::run(&threads, "generation", Mode::UserMean, 0.4, false, true).unwrap();
    assert_eq!(report.held_out_threads, 2);
    assert_eq!(report.partition.source_groups, 1);
    assert_eq!(report.partition.held_out_source_groups, 1);
    assert_eq!(report.partition.eligible_threads, 0);
    assert_eq!(report.partition.held_out_eligible_threads, 0);
    assert_eq!(report.samples, 0);
    let baseline = super::run(&threads, "generation", Mode::ThreadMean, 0.4, false, false).unwrap();
    assert_eq!(baseline.partition.eligible_threads, 2);
    assert_eq!(baseline.partition.assigned_eligible_threads, 2);
}
