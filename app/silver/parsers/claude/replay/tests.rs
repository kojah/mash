//! Differential control: the pre-staging replay rules retained verbatim in memory.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Reference {
    anchor: Option<String>,
    duplicates: BTreeSet<String>,
    changes: BTreeMap<String, ParentLink>,
}

impl Reference {
    fn clear(&mut self) {
        self.anchor = None;
        self.duplicates.clear();
        self.changes.clear();
    }

    fn observe(&mut self, record: &Record, parents: &mut Parents) -> Result<(), AppError> {
        let boundary = record.kind == Kind::System
            && record.subtype == Some(SystemSubtype::CompactBoundary)
            && record.parent == ParentLink::Root;
        let parent = record.parent.clone();
        let Some(uuid) = record.uuid.as_ref() else {
            if matches!(record.kind, Kind::User | Kind::Assistant) {
                self.clear();
            }
            return Ok(());
        };
        if let Some(known) = parents.get(uuid)? {
            self.duplicates.insert(uuid.clone());
            if let Ancestry::Known(previous) = known
                && previous != parent
            {
                self.changes.entry(uuid.clone()).or_insert(previous);
                parents.insert(uuid, Ancestry::Ambiguous)?;
            }
            return Ok(());
        }
        let proven = boundary
            && self.anchor.as_ref().is_some_and(|anchor| {
                record.logical_parent.as_ref() == Some(anchor)
                    && !self.duplicates.contains(anchor)
                    && record
                        .compact_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.preserved_segment.as_ref())
                        .is_some_and(|segment| segment.tail == *anchor)
            });
        if proven {
            for (key, original) in &self.changes {
                parents.insert(key, Ancestry::Known(original.clone()))?;
            }
        }
        self.clear();
        self.anchor = Some(uuid.clone());
        parents.insert(uuid, Ancestry::Known(parent))?;
        Ok(())
    }
}

fn record(uuid: &str, parent: Option<&str>) -> Record {
    serde_json::from_value(serde_json::json!({
        "type":"progress", "uuid":uuid, "parentUuid":parent,
    }))
    .unwrap()
}

fn boundary(uuid: &str, anchor: &str, tail: &str) -> Record {
    serde_json::from_value(serde_json::json!({
        "type":"system", "subtype":"compact_boundary", "uuid":uuid,
        "parentUuid":null, "logicalParentUuid":anchor,
        "compactMetadata":{"preservedSegment":{"tailUuid":tail}},
    }))
    .unwrap()
}

#[test]
fn staged_replay_matches_original_rules_after_every_arrival() {
    for limit in [0, 4096] {
        let mut replay = Replay {
            duplicates: Parents::with_limit(limit),
            ..Replay::default()
        };
        let mut reference = Reference::default();
        let mut parents = Parents::with_limit(limit);
        let mut reference_parents = Parents::with_limit(limit);
        let mut inputs = Vec::new();
        for round in 0..24 {
            let root = format!("root-{round}");
            let answer = format!("answer-{round}");
            let anchor = format!("anchor-{round}");
            inputs.extend([
                record(&root, None),
                record(&answer, Some(&root)),
                record(&anchor, Some(&answer)),
            ]);
            // Unchanged first occurrence, followed by conflicting and repeated copies.
            inputs.extend([
                record(&answer, Some(&root)),
                record(&answer, None),
                record(&answer, Some("other")),
            ]);
            match round % 4 {
                0 => {}
                1 => inputs.push(record(&anchor, None)), // replayed anchor cannot prove restoration
                2 => inputs.push(serde_json::from_str(r#"{"type":"user"}"#).unwrap()),
                _ => inputs.push(serde_json::from_str(r#"{"type":"progress"}"#).unwrap()),
            }
            inputs.push(boundary(
                &format!("boundary-{round}"),
                &anchor,
                if round % 3 == 0 { "wrong" } else { &anchor },
            ));
        }
        let mut keys = BTreeSet::new();
        for input in inputs {
            if let Some(key) = &input.uuid {
                keys.insert(key.clone());
            }
            replay.observe(&input, &mut parents).unwrap();
            reference.observe(&input, &mut reference_parents).unwrap();
            assert_eq!(replay.anchor, reference.anchor);
            for key in &keys {
                assert_eq!(
                    parents.get(key).unwrap(),
                    reference_parents.get(key).unwrap(),
                    "{key}"
                );
            }
        }
    }
}

#[test]
fn stop_hook_restoration_requires_a_linked_uninterrupted_boundary_in_both_stores() {
    for limit in [0, 4096] {
        for interruption in [
            "none",
            "new",
            "duplicate",
            "unidentified",
            "wrong-parent",
            "non-root",
        ] {
            let mut replay = Replay::with_limit(limit);
            let mut parents = Parents::with_limit(limit);
            for input in [
                record("root", None),
                record("answer", Some("root")),
                record("anchor", Some("answer")),
                record("answer", None),
            ] {
                replay.observe(&input, &mut parents).unwrap();
            }
            let hook: Record = serde_json::from_value(serde_json::json!({
                "type":"system","subtype":"stop_hook_summary","uuid":"hook",
                "parentUuid":if interruption == "wrong-parent" { "root" } else { "answer" },
            }))
            .unwrap();
            replay.observe(&hook, &mut parents).unwrap();
            let interrupt = match interruption {
                "new" => Some(record("unrelated", Some("hook"))),
                "duplicate" => Some(record("answer", None)),
                "unidentified" => Some(serde_json::from_str(r#"{"type":"user"}"#).unwrap()),
                _ => None,
            };
            if let Some(input) = interrupt {
                replay.observe(&input, &mut parents).unwrap();
            }
            let mut end = boundary("boundary", "hook", "hook");
            if interruption == "non-root" {
                end.parent = ParentLink::Parent("hook".into());
            }
            replay.observe(&end, &mut parents).unwrap();
            assert_eq!(
                parents.get("answer").unwrap(),
                Some(if interruption == "none" {
                    Ancestry::Known(ParentLink::Parent("root".into()))
                } else {
                    Ancestry::Ambiguous
                }),
                "{limit} {interruption}"
            );
        }
    }
}

#[test]
#[ignore = "explicit synthetic replay metadata memory diagnostic"]
fn benchmark_claude_replay_metadata() {
    let reference_mode = std::env::var("MASH_BENCH_REPLAY_MODE").expect("reference or staged");
    let count: usize = std::env::var("MASH_BENCH_REPLAY_COUNT")
        .expect("event count")
        .parse()
        .unwrap();
    assert!(count > 0 && count <= 1_000_000);
    assert!(matches!(reference_mode.as_str(), "reference" | "staged"));
    let started = std::time::Instant::now();
    let mut parents = Parents::default();
    let mut staged = Replay::default();
    let mut reference = Reference::default();
    let mut observe = |input: Record| {
        if reference_mode == "reference" {
            reference.observe(&input, &mut parents)
        } else {
            staged.observe(&input, &mut parents)
        }
        .unwrap();
    };
    for index in 0..count {
        observe(record(
            &format!("event-{index:012}"),
            Some("retained-dialogue"),
        ));
    }
    observe(record("anchor", Some("retained-dialogue")));
    for index in 0..count {
        observe(record(&format!("event-{index:012}"), None));
    }
    observe(boundary("boundary", "anchor", "anchor"));
    let mut digest = blake3::Hasher::new();
    for index in 0..count {
        let key = format!("event-{index:012}");
        assert_eq!(
            parents.get(&key).unwrap(),
            Some(Ancestry::Known(ParentLink::Parent(
                "retained-dialogue".to_owned()
            )))
        );
        digest.update(key.as_bytes());
        digest.update(b"retained-dialogue");
    }
    eprintln!(
        "mode={reference_mode} count={count} elapsed_ms={} digest={}",
        started.elapsed().as_millis(),
        digest.finalize()
    );
}
