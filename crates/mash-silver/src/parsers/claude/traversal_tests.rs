use super::*;
use std::collections::BTreeMap;

// The previous traversal is an independent oracle for the constant-retention
// implementation. Include all terminal states and paths entering short cycles.
fn reference(
    start: &str,
    parents: &BTreeMap<String, Ancestry>,
    identities: &BTreeMap<String, MessageIdentity>,
) -> Result<Option<MessageIdentity>, AppError> {
    let mut current = start;
    let mut visited = BTreeSet::new();
    loop {
        if let Some(id) = identities.get(current) {
            return Ok(Some(id.clone()));
        }
        if !visited.insert(current) {
            return Err(AppError::configuration(
                "Claude message ancestry contains a cycle",
            ));
        }
        match parents.get(current) {
            Some(Ancestry::Known(ParentLink::Parent(parent))) => current = parent,
            Some(Ancestry::Known(ParentLink::Root | ParentLink::Missing)) => return Ok(None),
            Some(Ancestry::Ambiguous) => return Err(ambiguous_ancestry(current)),
            None => {
                return Ok(Some(MessageIdentity {
                    id: stable_id("claude-message", &["thread", current]),
                    position: None,
                }));
            }
        }
    }
}

fn outcome(
    result: Result<Option<MessageIdentity>, AppError>,
) -> Result<Option<(String, Option<usize>)>, String> {
    result
        .map(|value| value.map(|id| (id.id, id.position)))
        .map_err(|error| error.error.message)
}

#[test]
fn constant_retention_traversal_matches_visited_set_for_all_small_graphs() {
    let keys = ["a", "b", "c"];
    let choices = [
        Ancestry::Known(ParentLink::Root),
        Ancestry::Known(ParentLink::Missing),
        Ancestry::Ambiguous,
        Ancestry::Known(ParentLink::Parent("a".to_owned())),
        Ancestry::Known(ParentLink::Parent("b".to_owned())),
        Ancestry::Known(ParentLink::Parent("c".to_owned())),
        Ancestry::Known(ParentLink::Parent("unknown".to_owned())),
    ];
    let mut staged = Parents::default();
    // Force the production spill threshold with unrelated exact ancestry;
    // then reuse the same disk store across this exhaustive 343-graph matrix.
    for index in 0..20_000 {
        staged
            .insert(
                &format!("unrelated-{index}"),
                Ancestry::Known(ParentLink::Root),
            )
            .unwrap();
    }
    for number in 0..choices.len().pow(3) {
        let mut reference_parents = BTreeMap::new();
        let mut remaining = number;
        for key in keys {
            let value = choices[remaining % choices.len()].clone();
            remaining /= choices.len();
            staged.insert(key, value.clone()).unwrap();
            reference_parents.insert(key.to_owned(), value);
        }
        for dialogue in [false, true] {
            let identities = if dialogue {
                [(
                    "b".to_owned(),
                    MessageIdentity {
                        id: stable_id("claude-message", &["thread", "b"]),
                        position: Some(7),
                    },
                )]
                .into()
            } else {
                BTreeMap::new()
            };
            let mut dialogue_keys = Keys::with_limit(0);
            if dialogue {
                for _ in 0..7 {
                    dialogue_keys.push(None).unwrap();
                }
                dialogue_keys.push(Some("b".to_owned())).unwrap();
            }
            for key in ["a", "b", "c", "unknown"] {
                assert_eq!(
                    outcome(resolve_parent("thread", key, &dialogue_keys, &staged)),
                    outcome(reference(key, &reference_parents, &identities)),
                    "graph {number}, start {key}, dialogue {dialogue}"
                );
            }
        }
    }
}
