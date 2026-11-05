//! View projection of connector objects: searchable entries and documents.
//!
//! Connector objects have no native message rowid, provider or role. Each unit
//! is keyed by a namespaced anchor, indexed for lexical search, and embedded
//! once per generation. Coordinators synchronize the complete projection
//! fingerprint, so unchanged records keep their Qdrant points and vectors.
use super::Storage;
use super::generated as sql;
use crate::AppError;
use crate::connectors::{AuthorKind, Object, Role};
use crate::source::{Ancestor, ObjectType, Source};
use rusqlite::Connection;
use std::collections::HashMap;

/// Every connector anchor starts with this; native message IDs never do.
pub(crate) const ANCHOR_PREFIX: &str = "connector#";

pub(crate) fn role_name(role: Role) -> &'static str {
    crate::connectors::silver::role_name(role)
}

pub(crate) fn parse_role(role: &str) -> Option<Role> {
    match role {
        "container" => Some(Role::Container),
        "segment" => Some(Role::Segment),
        "entry" => Some(Role::Entry),
        "document" => Some(Role::Document),
        _ => None,
    }
}

/// `connector#<role>#<source_id>#<id>`. Connector IDs exclude `#`, so the
/// anchor splits unambiguously.
pub(crate) fn anchor(role: Role, source_id: &str, id: &str) -> String {
    format!("{ANCHOR_PREFIX}{}#{source_id}#{id}", role_name(role))
}

/// The role, source and object ID named by an anchor.
pub(crate) fn parse_anchor(anchor: &str) -> Option<(Role, &str, &str)> {
    let mut parts = anchor.strip_prefix(ANCHOR_PREFIX)?.splitn(3, '#');
    Some((parse_role(parts.next()?)?, parts.next()?, parts.next()?))
}

/// The text indexed and embedded for a unit. A container or document title
/// gives short context, much as native embeddings prefix the chat title.
pub(crate) fn unit_text(title: Option<&str>, content: &str) -> String {
    match title.map(str::trim).filter(|title| !title.is_empty()) {
        Some(title) => format!("{title}\n{content}"),
        None => content.to_owned(),
    }
}

#[derive(Clone)]
pub(crate) struct Unit {
    pub(crate) anchor: String,
    pub(crate) object: ObjectType,
    pub(crate) source_id: String,
    pub(crate) record_id: String,
    /// An entry's nearest container, or a document's own anchor.
    pub(crate) group_anchor: String,
    pub(crate) created_at: Option<i64>,
    /// Position among the container's entries, the same total order the thread
    /// reader uses. Native messages call this `ordinal`.
    pub(crate) sequence: i64,
    /// What is indexed and embedded; its digest decides when to re-index.
    pub(crate) text: String,
}

fn decode(record: Option<&str>) -> Result<Object, AppError> {
    let record =
        record.ok_or_else(|| AppError::database_data("stored connector record is missing"))?;
    serde_json::from_str(record)
        .map_err(|_| AppError::database_data("stored connector record is invalid"))
}

/// A unit's evidence, hydrated from silver: the object and its ancestry.
pub(crate) struct Evidence {
    pub(crate) object: Object,
    /// Enclosing objects, nearest first.
    pub(crate) ancestors: Vec<(Role, Object)>,
}

impl Evidence {
    /// The group a result belongs to: an entry's nearest container, or itself.
    fn group(&self) -> Option<&Object> {
        if self.object.role == Role::Entry {
            self.ancestors
                .iter()
                .find(|(role, _)| *role == Role::Container)
                .map(|(_, object)| object)
        } else {
            Some(&self.object)
        }
    }

    pub(crate) fn title(&self) -> Option<&str> {
        self.group().and_then(|group| group.title.as_deref())
    }

    pub(crate) fn content(&self) -> &str {
        self.object.content.as_deref().unwrap_or_default()
    }

    pub(crate) fn author(&self) -> Option<AuthorKind> {
        (self.object.role == Role::Entry).then(|| {
            self.object
                .author
                .as_ref()
                .map_or(AuthorKind::Unknown, |author| author.kind)
        })
    }

    pub(crate) fn text(&self) -> String {
        unit_text(self.title(), self.content())
    }

    pub(crate) fn object_type(&self) -> ObjectType {
        ObjectType {
            role: self.object.role,
            kind: self.object.kind.as_str().to_owned(),
        }
    }

    pub(crate) fn ancestors(&self, source_id: &str) -> Vec<Ancestor> {
        self.ancestors
            .iter()
            .map(|(role, object)| Ancestor {
                id: anchor(*role, source_id, object.id.as_str()),
                role: *role,
                kind: object.kind.as_str().to_owned(),
                title: object.title.clone(),
            })
            .collect()
    }

    pub(crate) fn group_times(&self) -> (Option<i64>, Option<i64>) {
        self.group()
            .map_or((None, None), |group| (group.created_at, group.updated_at))
    }
}

/// Load one object and walk its parents. Missing objects yield `None`.
pub(crate) fn evidence(
    connection: &Connection,
    source_id: &str,
    id: &str,
) -> Result<Option<Evidence>, AppError> {
    let load = |id: &str| {
        sql::ConnectorObject::builder()
            .source_id(source_id)
            .id(id)
            .build()
            .query_opt(connection)
            .map_err(AppError::database)
    };
    let Some(row) = load(id)? else {
        return Ok(None);
    };
    let object = decode(row.record.as_deref())?;
    let mut ancestors = Vec::new();
    let mut current = row.parent_id;
    // Silver rejects parent cycles; the bound only guards corrupt data.
    while let Some(parent) = current {
        if ancestors.len() > 256 {
            return Err(AppError::database_data("connector ancestry is too deep"));
        }
        let row = load(&parent)?
            .ok_or_else(|| AppError::database_data("connector ancestry is incomplete"))?;
        let role = parse_role(&row.role)
            .ok_or_else(|| AppError::database_data("stored connector role is invalid"))?;
        ancestors.push((role, decode(row.record.as_deref())?));
        current = row.parent_id;
    }
    Ok(Some(Evidence { object, ancestors }))
}

struct Row {
    role: Role,
    parent: Option<String>,
    object: Object,
}

impl Row {
    fn new(role: &str, parent: Option<String>, record: Option<&str>) -> Result<Self, AppError> {
        Ok(Self {
            role: parse_role(role)
                .ok_or_else(|| AppError::database_data("stored connector role is invalid"))?,
            parent,
            object: decode(record)?,
        })
    }
}

/// Every searchable unit that connector silver currently holds.
pub(crate) fn silver_units(connection: &Connection) -> Result<Vec<Unit>, AppError> {
    let mut objects = HashMap::new();
    for row in sql::ConnectorSilverObjects
        .query_many(connection)
        .map_err(AppError::database)?
    {
        objects.insert(
            (row.source_id, row.id),
            Row::new(&row.role, row.parent_id, row.record.as_deref())?,
        );
    }
    units(&objects, None)
}

fn units(
    objects: &HashMap<(String, String), Row>,
    emit: Option<&std::collections::HashSet<(String, String)>>,
) -> Result<Vec<Unit>, AppError> {
    let mut units = Vec::new();
    for ((source_id, id), row) in objects {
        if emit.is_some_and(|emit| !emit.contains(&(source_id.clone(), id.clone()))) {
            continue;
        }
        let content = match (row.role, row.object.content.as_deref()) {
            (Role::Entry | Role::Document, Some(content)) if !content.trim().is_empty() => content,
            _ => continue,
        };
        let (group_anchor, title) = if row.role == Role::Entry {
            // Silver guarantees a container ancestor and no cycles.
            let mut current = row.parent.clone();
            let mut found = None;
            while let Some(parent) = current.take() {
                let Some(ancestor) = objects.get(&(source_id.clone(), parent.clone())) else {
                    break;
                };
                if ancestor.role == Role::Container {
                    found = Some((
                        anchor(Role::Container, source_id, &parent),
                        ancestor.object.title.as_deref(),
                    ));
                    break;
                }
                current.clone_from(&ancestor.parent);
            }
            found.ok_or_else(|| AppError::database_data("connector entry has no container"))?
        } else {
            (anchor(row.role, source_id, id), row.object.title.as_deref())
        };
        units.push(Unit {
            anchor: anchor(row.role, source_id, id),
            object: ObjectType {
                role: row.role,
                kind: row.object.kind.as_str().to_owned(),
            },
            source_id: source_id.clone(),
            record_id: id.clone(),
            group_anchor,
            created_at: row.object.created_at,
            sequence: 0,
            text: unit_text(title, content),
        });
    }
    units.sort_by(|left, right| left.anchor.cmp(&right.anchor));
    order_entries(&mut units);
    Ok(units)
}

/// Assign each entry its position among its container's entries, ordered by
/// creation time and then id. Documents and containers stay at zero.
fn order_entries(units: &mut [Unit]) {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (index, unit) in units.iter().enumerate() {
        if unit.object.role == Role::Entry {
            groups
                .entry((unit.source_id.clone(), unit.group_anchor.clone()))
                .or_default()
                .push(index);
        }
    }
    for indices in groups.values_mut() {
        indices.sort_by(|left, right| {
            let left = &units[*left];
            let right = &units[*right];
            (
                left.created_at.is_none(),
                left.created_at,
                left.record_id.as_str(),
            )
                .cmp(&(
                    right.created_at.is_none(),
                    right.created_at,
                    right.record_id.as_str(),
                ))
        });
        for (position, index) in indices.iter().enumerate() {
            units[*index].sequence = i64::try_from(position).unwrap_or(i64::MAX);
        }
    }
}

impl Storage {
    pub(crate) fn connector_view(
        &self,
        anchor: &str,
        context: u32,
    ) -> Result<Vec<super::Message>, AppError> {
        view(&self.connection, anchor, context)
    }
}

fn entry_order(
    connection: &Connection,
    source_id: &str,
    container: &str,
) -> Result<Vec<sql::ConnectorEntryOrderRow>, AppError> {
    let mut entries = sql::ConnectorEntryOrder::builder()
        .conversation_id(container)
        .source_id(source_id)
        .build()
        .query_many(connection)
        .map_err(AppError::database)?;
    entries.sort_by(|left, right| {
        (left.created_at.is_none(), left.created_at, left.id.as_str()).cmp(&(
            right.created_at.is_none(),
            right.created_at,
            right.id.as_str(),
        ))
    });
    Ok(entries)
}

/// Reuse chronological metadata only within one batch of context windows.
#[derive(Default)]
pub(super) struct EntryOrders(HashMap<(String, String), Vec<sql::ConnectorEntryOrderRow>>);

impl EntryOrders {
    fn entries(
        &mut self,
        connection: &Connection,
        source_id: &str,
        container: &str,
    ) -> Result<&[sql::ConnectorEntryOrderRow], AppError> {
        use std::collections::hash_map::Entry;
        let key = (source_id.to_owned(), container.to_owned());
        Ok(match self.0.entry(key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(entry_order(connection, source_id, container)?),
        })
    }
}

/// An entry with up to `context` chronological neighbours among the entries of
/// its nearest container (segments included), or a document on its own.
/// Unknown anchors yield nothing.
pub(crate) fn view(
    connection: &Connection,
    target: &str,
    context: u32,
) -> Result<Vec<super::Message>, AppError> {
    view_with_orders(connection, target, context, &mut EntryOrders::default())
}

pub(super) fn view_with_orders(
    connection: &Connection,
    target: &str,
    context: u32,
    orders: &mut EntryOrders,
) -> Result<Vec<super::Message>, AppError> {
    let Some((_, source_id, id)) = parse_anchor(target) else {
        return Ok(Vec::new());
    };
    let Some(evidence) = evidence(connection, source_id, id)? else {
        return Ok(Vec::new());
    };
    let message = |object: &Object, ordinal: usize, text: String| super::Message {
        attachments: Vec::new(),
        id: anchor(object.role, source_id, object.id.as_str()),
        ordinal: i64::try_from(ordinal).unwrap_or(i64::MAX),
        role: None,
        author_kind: (object.role == Role::Entry).then(|| {
            object
                .author
                .as_ref()
                .map_or(AuthorKind::Unknown, |author| author.kind)
        }),
        content: text,
        created_at: object.created_at,
        tools: crate::tools::Activity::default(),
    };
    match evidence.object.role {
        Role::Document => Ok(vec![message(&evidence.object, 0, evidence.text())]),
        Role::Container | Role::Segment => Ok(Vec::new()),
        Role::Entry => {
            let Some(container) = evidence.group() else {
                return Ok(Vec::new());
            };
            // Order the container's entries from indexed columns, then decode
            // only the window: a long channel is not read for a few messages.
            let entries = orders.entries(connection, source_id, container.id.as_str())?;
            let Some(position) = entries.iter().position(|entry| entry.id.as_str() == id) else {
                return Ok(Vec::new());
            };
            let context = usize::try_from(context).unwrap_or(usize::MAX);
            let first = position.saturating_sub(context);
            let last = position.saturating_add(context).min(entries.len() - 1);
            entries[first..=last]
                .iter()
                .enumerate()
                .map(|(offset, entry)| {
                    let row = sql::ConnectorObject::builder()
                        .source_id(source_id)
                        .id(&entry.id)
                        .build()
                        .query_opt(connection)
                        .map_err(AppError::database)?
                        .ok_or_else(|| AppError::database_data("connector entry disappeared"))?;
                    let entry = decode(row.record.as_deref())?;
                    Ok(message(
                        &entry,
                        first + offset,
                        entry.content.clone().unwrap_or_default(),
                    ))
                })
                .collect()
        }
    }
}

/// Connector containers as catalog rows, keyed by container anchor.
pub(crate) fn chats(
    connection: &Connection,
    node: &str,
) -> Result<Vec<crate::chat_catalog::Chat>, AppError> {
    sql::ConnectorContainers
        .query_many(connection)
        .map_err(AppError::database)?
        .into_iter()
        .map(|row| {
            let evidence = evidence(connection, &row.source_id, &row.id)?
                .ok_or_else(|| AppError::database_data("connector container disappeared"))?;
            Ok(crate::chat_catalog::Chat {
                conversation_id: anchor(Role::Container, &row.source_id, &row.id),
                object: evidence.object_type(),
                ancestors: evidence.ancestors(&row.source_id),
                source: Source::Connector(row.source_id),
                node_id: node.into(),
                metadata: crate::search_results::ChatMetadata {
                    enrichment: None,
                    title: evidence.object.title.clone(),
                    project: None,
                    repository: None,
                    created_at: evidence.object.created_at,
                    updated_at: evidence.object.updated_at,
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod sequence_tests {
    use super::*;

    fn entry(id: &str, created_at: Option<i64>) -> Unit {
        Unit {
            anchor: id.to_owned(),
            object: ObjectType {
                role: Role::Entry,
                kind: "slack:message".to_owned(),
            },
            source_id: "slack:T0TEAM:U0ALICE".to_owned(),
            record_id: id.to_owned(),
            group_anchor: "channel".to_owned(),
            created_at,
            sequence: 0,
            text: String::new(),
        }
    }

    /// Connector entries get the position `thread()` reads them in: creation
    /// time, then id, with undated entries last.
    #[test]
    fn entries_are_ordered_by_creation_then_id() {
        let mut units = vec![
            entry("b", Some(2)),
            entry("a", Some(1)),
            entry("c", None),
            entry("d", Some(1)),
        ];
        order_entries(&mut units);
        let mut ordered: Vec<_> = units
            .iter()
            .map(|unit| (unit.sequence, unit.record_id.as_str()))
            .collect();
        ordered.sort_unstable();
        assert_eq!(ordered, vec![(0, "a"), (1, "d"), (2, "b"), (3, "c")]);
    }
}
