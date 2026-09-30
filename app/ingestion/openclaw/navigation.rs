//! Visible transcript ancestry, independently of model compaction windows.
use super::{AppendMode, Event, Kind, Parent};
use crate::AppError;
use std::collections::{BTreeMap, BTreeSet};

struct Node {
    index: usize,
    parent: Option<String>,
}

fn path(nodes: &BTreeMap<String, Node>, leaf: Option<&str>) -> Result<Vec<usize>, AppError> {
    let mut current = leaf;
    let mut seen = BTreeSet::new();
    let mut indexes = Vec::new();
    while let Some(id) = current {
        if !seen.insert(id) {
            return Err(AppError::configuration(
                "cyclic OpenClaw transcript ancestry",
            ));
        }
        let Some(node) = nodes.get(id) else {
            break;
        };
        indexes.push(node.index);
        current = node.parent.as_deref();
    }
    indexes.reverse();
    Ok(indexes)
}

fn reference(value: &Parent, fallback: Option<&str>) -> Option<String> {
    match value {
        Parent::Missing => fallback.map(str::to_owned),
        Parent::Null => None,
        Parent::Id(id) => Some(id.clone()),
    }
}

#[derive(Default)]
struct Navigation {
    nodes: BTreeMap<String, Node>,
    leaf: Option<String>,
    append: Option<String>,
    reset_descendants: BTreeSet<String>,
    reset_seen: bool,
    explicit: bool,
}
impl Navigation {
    fn control(&mut self, event: &Event, index: usize, id: &str) -> Result<(), AppError> {
        if event.target == Parent::Missing {
            return Err(AppError::configuration("OpenClaw leaf has no target"));
        }
        let mut target = reference(&event.target, None);
        let mut cursor = reference(&event.append_parent, target.as_deref());
        if self.reset_seen
            && self.leaf.is_some()
            && target
                .as_ref()
                .is_none_or(|id| !self.reset_descendants.contains(id))
        {
            target.clone_from(&self.leaf);
            cursor.clone_from(&self.leaf);
        }
        if target
            .as_ref()
            .is_some_and(|id| !self.nodes.contains_key(id))
            || cursor
                .as_ref()
                .is_some_and(|id| !self.nodes.contains_key(id))
        {
            return Err(AppError::configuration(
                "OpenClaw leaf references an unknown event",
            ));
        }
        self.nodes.insert(
            id.to_owned(),
            Node {
                index,
                parent: target.clone(),
            },
        );
        self.leaf = target;
        self.append = cursor;
        self.explicit = true;

        Ok(())
    }
    fn observe(&mut self, events: &[Event], index: usize) -> Result<(), AppError> {
        let event = &events[index];

        if event.kind == Kind::Session {
            return Ok(());
        }
        let Some(id) = event.id.as_ref().filter(|id| !id.trim().is_empty()) else {
            if event.kind.canonical() {
                return Err(AppError::configuration("OpenClaw event has no identity"));
            }
            return Ok(());
        };
        if self.nodes.contains_key(id) {
            return Err(AppError::configuration("duplicate OpenClaw event identity"));
        }
        let side = event.append_mode == Some(AppendMode::Side);
        if event.kind == Kind::Leaf {
            return self.control(event, index, id);
        }
        let mut parent = reference(&event.parent, self.leaf.as_deref());
        if event.kind.canonical() && !side {
            if (self.reset_seen
                && parent
                    .as_ref()
                    .is_none_or(|id| !self.reset_descendants.contains(id)))
                || (self.leaf.is_some()
                    && parent
                        .as_ref()
                        .is_some_and(|id| !self.nodes.contains_key(id)))
                || (parent == self.append && self.leaf != self.append)
            {
                parent.clone_from(&self.leaf);
            }
            let mut seen = BTreeSet::new();
            while let Some(id) = parent.as_ref() {
                if !seen.insert(id.clone()) {
                    return Err(AppError::configuration("cyclic OpenClaw leaf control"));
                }
                let Some(node) = self.nodes.get(id) else {
                    break;
                };
                if events[node.index].kind != Kind::Leaf {
                    break;
                }
                parent.clone_from(&node.parent);
            }
        }
        if event.kind == Kind::Reset {
            self.reset_seen = true;
            self.reset_descendants.clear();
            self.reset_descendants.insert(id.clone());
        } else if self.reset_seen
            && parent
                .as_ref()
                .is_some_and(|id| self.reset_descendants.contains(id))
        {
            self.reset_descendants.insert(id.clone());
        }
        self.nodes.insert(id.clone(), Node { index, parent });
        self.append = Some(id.clone());
        if event.kind.canonical() && !side {
            self.leaf = Some(id.clone());
            self.explicit |= event.parent != Parent::Missing;
        }

        Ok(())
    }
}

fn reset_window(events: &[Event], mut selected: Vec<usize>) -> Vec<usize> {
    // A reset changes visible history; compaction changes model context only.
    if let Some(reset) = selected
        .iter()
        .rposition(|&i| events[i].kind == Kind::Reset)
    {
        let kept = events[selected[reset]]
            .first_kept
            .as_deref()
            .and_then(|id| {
                selected[..reset]
                    .iter()
                    .position(|&i| events[i].id.as_deref() == Some(id))
            });
        let mut recent = Vec::new();
        if let Some(first) = kept {
            recent.extend(
                selected[first..reset]
                    .iter()
                    .copied()
                    .filter(|&i| events[i].kind == Kind::Message),
            );
        }
        recent.extend_from_slice(&selected[reset..]);
        selected = recent;
    }
    selected.retain(|&i| events[i].kind != Kind::Leaf);
    selected
}

pub(super) fn select(events: &[Event]) -> Result<Vec<usize>, AppError> {
    let mut navigation = Navigation::default();
    for index in 0..events.len() {
        navigation.observe(events, index)?;
    }
    let selected = if navigation.explicit {
        path(&navigation.nodes, navigation.leaf.as_deref())?
    } else {
        (0..events.len()).collect()
    };
    Ok(reset_window(events, selected))
}
