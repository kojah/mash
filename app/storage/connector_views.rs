//! View projection of connector objects: searchable entries and documents.
//!
//! Connector objects have no native message rowid, provider or role. Each unit
//! is keyed by a namespaced anchor, indexed for lexical search, and embedded
//! once per generation. Units are synchronized from silver incrementally by
//! content digest, so unchanged records keep their FTS rows and vectors.
use super::generated as sql;
use super::{SearchHit, Storage};
use crate::AppError;
use crate::connectors::{AuthorKind, Object, Role};
use crate::source::{Ancestor, ObjectType, Source};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::fmt::Write as _;

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
    /// Every ancestor's identity, kind and title, so renaming or moving an
    /// enclosing object changes the unit's digest too.
    pub(crate) ancestry: String,
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

/// Conversation roots, by source and object ID, a scoped refresh covers.
pub(crate) type Conversations = std::collections::BTreeSet<(String, String)>;

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

/// The units of the given conversations only, identical to the units
/// `silver_units` gives them: entries are ordered within their conversation,
/// and a root's ancestors are loaded for ancestry but yield no units.
pub(crate) fn conversation_units(
    connection: &Connection,
    conversations: &Conversations,
) -> Result<Vec<Unit>, AppError> {
    let mut objects = HashMap::new();
    let mut emit = std::collections::HashSet::new();
    for (source_id, root) in conversations {
        for row in sql::ConnectorConversationObjects::builder()
            .conversation_id(root)
            .source_id(source_id)
            .build()
            .query_many(connection)
            .map_err(AppError::database)?
        {
            emit.insert((source_id.clone(), row.id.clone()));
            objects.insert(
                (source_id.clone(), row.id),
                Row::new(&row.role, row.parent_id, row.record.as_deref())?,
            );
        }
        let mut current = objects
            .get(&(source_id.clone(), root.clone()))
            .and_then(|row| row.parent.clone());
        while let Some(parent) = current.take() {
            let key = (source_id.clone(), parent.clone());
            if let Some(row) = objects.get(&key) {
                current.clone_from(&row.parent);
                continue;
            }
            let Some(row) = sql::ConnectorObject::builder()
                .source_id(source_id)
                .id(&parent)
                .build()
                .query_opt(connection)
                .map_err(AppError::database)?
            else {
                break;
            };
            let row = Row::new(&row.role, row.parent_id, row.record.as_deref())?;
            current.clone_from(&row.parent);
            objects.insert(key, row);
        }
    }
    units(&objects, Some(&emit))
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
        let mut ancestry = String::new();
        let mut current = row.parent.clone();
        while let Some(parent) = current.take() {
            let Some(ancestor) = objects.get(&(source_id.clone(), parent.clone())) else {
                break;
            };
            // Writing to a String cannot fail.
            let _ = writeln!(
                ancestry,
                "{}\0{}\0{}\0{}",
                role_name(ancestor.role),
                parent,
                ancestor.object.kind.as_str(),
                ancestor.object.title.as_deref().unwrap_or_default()
            );
            current.clone_from(&ancestor.parent);
        }
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
            ancestry,
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
    /// The conversations a just-committed batch can have changed units in:
    /// where its objects sit now and sat before (by their existing units), and
    /// every container or document nested under one of them. `None` when only
    /// a full comparison is safe: deletions and complete snapshots can remove
    /// whole subtrees, including nested conversations.
    pub(crate) fn connector_conversations_touched(
        &self,
        batch: &crate::connectors::Batch,
    ) -> Result<Option<Conversations>, AppError> {
        use crate::connectors::{Change, Trigger};
        if matches!(batch.trigger, Some(Trigger::CompleteSnapshot))
            || batch
                .changes
                .iter()
                .any(|change| matches!(change, Change::DeleteObject { .. }))
        {
            return Ok(None);
        }
        let source = batch.source_id.as_str();
        let ids: std::collections::HashSet<&str> = batch
            .changes
            .iter()
            .filter_map(|change| match change {
                Change::UpsertObject(object) => Some(object.id.as_str()),
                _ => None,
            })
            .collect();
        let mut touched = Conversations::new();
        for id in &ids {
            if let Some(row) = sql::ConnectorConversation::builder()
                .source_id(source)
                .id(id)
                .build()
                .query_opt(&self.connection)
                .map_err(AppError::database)?
            {
                touched.insert((source.to_owned(), row.conversation_id));
            }
        }
        // A moved object's units still sit in its previous conversation.
        for row in sql::ConnectorUnitGroups
            .query_many(&self.connection)
            .map_err(AppError::database)?
        {
            if row.source_id == source
                && ids.contains(row.record_id.as_str())
                && let Some((_, unit_source, root)) = parse_anchor(&row.group_anchor)
            {
                touched.insert((unit_source.to_owned(), root.to_owned()));
            }
        }
        // Renaming or moving an ancestor changes nested conversations' ancestry.
        for root in sql::ConnectorRoots::builder()
            .source_id(source)
            .build()
            .query_many(&self.connection)
            .map_err(AppError::database)?
        {
            let mut current = root.parent_id;
            while let Some(parent) = current.take() {
                if ids.contains(parent.as_str()) {
                    touched.insert((source.to_owned(), root.id.clone()));
                    break;
                }
                current = sql::ConnectorObject::builder()
                    .source_id(source)
                    .id(&parent)
                    .build()
                    .query_opt(&self.connection)
                    .map_err(AppError::database)?
                    .and_then(|row| row.parent_id);
            }
        }
        Ok(Some(touched))
    }

    /// Limit the next connector unit refresh to these conversations. The
    /// caller vouches that nothing outside them changed since views were last
    /// certified; without a scope every connector unit is compared.
    pub(crate) fn scope_connector_refresh(&mut self, conversations: Conversations) {
        self.connector_scope = Some(conversations);
    }

    /// Bring connector units, their FTS rows and vector invalidation in line
    /// with connector silver. Runs inside the views build's writer.
    pub(crate) fn sync_connector_units(&mut self) -> Result<(), AppError> {
        self.require_writer()?;
        let scope = self.connector_scope.take();
        let mut known = HashMap::new();
        for row in sql::ConnectorUnitGroups
            .query_many(&self.connection)
            .map_err(AppError::database)?
        {
            // A scoped refresh owns only the units of its conversations.
            let owned = scope.as_ref().is_none_or(|scope| {
                parse_anchor(&row.group_anchor).is_some_and(|(_, source, root)| {
                    scope.contains(&(source.to_owned(), root.to_owned()))
                })
            });
            if owned {
                known.insert(row.anchor, (row.id, row.digest));
            }
        }
        let units = match &scope {
            Some(scope) => conversation_units(&self.connection, scope)?,
            None => silver_units(&self.connection)?,
        };
        for unit in units {
            let digest = blake3::hash(unit_digest_input(&unit).as_bytes())
                .as_bytes()
                .to_vec();
            match known.remove(&unit.anchor) {
                Some((_, previous)) if previous == digest => {}
                Some((id, _)) => {
                    // Positions and ancestry shift as a conversation grows;
                    // the vector embeds only the text, so keep it when that
                    // is unchanged instead of re-embedding the conversation.
                    let text_changed = self.connector_text(id)?.as_deref() != Some(&unit.text);
                    sql::ConnectorUpdateUnit::builder()
                        .label(&unit.object.kind)
                        .group_anchor(&unit.group_anchor)
                        .created_at(unit.created_at)
                        .digest(&digest)
                        .id(id)
                        .build()
                        .execute(&self.connection)
                        .map_err(AppError::database)?;
                    if text_changed {
                        self.delete_connector_text(id)?;
                        self.insert_connector_text(id, &unit.text)?;
                        sql::ConnectorDeleteUnitEmbedding::builder()
                            .unit_id(id)
                            .build()
                            .execute(&self.connection)
                            .map_err(AppError::database)?;
                    }
                }
                None => {
                    let id = sql::ConnectorInsertUnit::builder()
                        .anchor(&unit.anchor)
                        .role(role_name(unit.object.role))
                        .label(&unit.object.kind)
                        .source_id(&unit.source_id)
                        .record_id(&unit.record_id)
                        .group_anchor(&unit.group_anchor)
                        .created_at(unit.created_at)
                        .digest(&digest)
                        .build()
                        .query_one(&self.connection)
                        .map_err(AppError::database)?
                        .id;
                    self.insert_connector_text(id, &unit.text)?;
                }
            }
        }
        for (_, (id, _)) in known {
            self.delete_connector_text(id)?;
            sql::ConnectorDeleteUnitEmbedding::builder()
                .unit_id(id)
                .build()
                .execute(&self.connection)
                .map_err(AppError::database)?;
            sql::ConnectorDeleteUnit::builder()
                .id(id)
                .build()
                .execute(&self.connection)
                .map_err(AppError::database)?;
        }
        Ok(())
    }

    // sqlc cannot analyze FTS5 virtual tables; these statements stay handwritten.
    fn insert_connector_text(&self, id: i64, text: &str) -> Result<(), AppError> {
        self.connection
            .execute(
                "INSERT INTO connector_fts(rowid, content) VALUES (?1, ?2)",
                params![id, text],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    fn connector_text(&self, id: i64) -> Result<Option<String>, AppError> {
        self.connection
            .query_row(
                "SELECT content FROM connector_fts WHERE rowid = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::database)
    }

    fn delete_connector_text(&self, id: i64) -> Result<(), AppError> {
        self.connection
            .execute("DELETE FROM connector_fts WHERE rowid = ?1", [id])
            .map_err(AppError::database)?;
        Ok(())
    }

    /// Ranked unit IDs and BM25 scores for an FTS5 match expression. Handwritten:
    /// sqlc cannot analyze FTS5, and the filter predicate is compiled at runtime.
    pub(crate) fn search_connector_lexical(
        &self,
        query: &str,
        limit: usize,
        cutoff: Option<i64>,
        filters: Option<&crate::search_filter::Scope>,
    ) -> Result<Vec<(i64, f64)>, AppError> {
        let predicate = filters.map_or_else(
            || "TRUE".to_owned(),
            |f| f.filter.connector_sql(Some(&f.node)),
        );
        let limit = i64::try_from(limit.min(1_000)).unwrap_or(1_000);
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT u.id, bm25(connector_fts) AS score
                   FROM connector_fts
                   JOIN connector_units u ON u.id = connector_fts.rowid
                  WHERE connector_fts MATCH ?1
                    AND (?2 IS NULL OR u.created_at >= ?2)
                    AND ({predicate})
                  ORDER BY score, u.id
                  LIMIT ?3"
            ))
            .map_err(AppError::database)?;
        let rows = statement
            .query_map(params![query, cutoff, limit], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(AppError::database)?;
        rows.collect::<Result<_, _>>().map_err(AppError::database)
    }

    /// Current-generation vectors of units the filters admit. Handwritten for the
    /// runtime-compiled filter predicate.
    pub(crate) fn connector_vectors(
        &self,
        generation: &str,
        cutoff: Option<i64>,
        filters: Option<&crate::search_filter::Scope>,
    ) -> Result<Vec<(i64, f32, Vec<u8>)>, AppError> {
        let predicate = filters.map_or_else(
            || "TRUE".to_owned(),
            |f| f.filter.connector_sql(Some(&f.node)),
        );
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT u.id, e.norm, e.vector
                   FROM connector_embeddings e
                   JOIN connector_units u ON u.id = e.unit_id
                  WHERE e.generation = ?1
                    AND (?2 IS NULL OR u.created_at >= ?2)
                    AND ({predicate})"
            ))
            .map_err(AppError::database)?;
        let rows = statement
            .query_map(params![generation, cutoff], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(AppError::database)?;
        rows.map(|row| {
            let (id, norm, vector) = row.map_err(AppError::database)?;
            #[allow(clippy::cast_possible_truncation)]
            Ok((id, norm as f32, vector))
        })
        .collect()
    }

    /// Search hits for unit IDs, in the given order.
    pub(crate) fn connector_hits(&self, ids: &[i64]) -> Result<Vec<SearchHit>, AppError> {
        let mut positions = Positions::default();
        ids.iter()
            .map(|id| {
                let unit = sql::ConnectorUnit::builder()
                    .id(*id)
                    .build()
                    .query_opt(&self.connection)
                    .map_err(AppError::database)?
                    .ok_or_else(|| {
                        AppError::database_data("connector index references a missing unit")
                    })?;
                connector_hit(
                    &self.connection,
                    &unit.source_id,
                    &unit.record_id,
                    &mut positions,
                )
            })
            .collect()
    }

    /// Up to `limit` units lacking a vector for this generation, with their text.
    pub(crate) fn pending_connector_embeddings(
        &self,
        generation: &str,
        limit: usize,
    ) -> Result<Vec<(i64, String)>, AppError> {
        sql::ConnectorPendingEmbeddings::builder()
            .generation(generation)
            .page_size(i64::try_from(limit).unwrap_or(i64::MAX))
            .build()
            .query_many(&self.connection)
            .map_err(AppError::database)?
            .into_iter()
            .map(|row| {
                let evidence = evidence(&self.connection, &row.source_id, &row.record_id)?
                    .ok_or_else(|| {
                        AppError::database_data("connector index references missing silver")
                    })?;
                Ok((row.id, evidence.text()))
            })
            .collect()
    }

    pub(crate) fn store_connector_embedding(
        &self,
        unit_id: i64,
        generation: &str,
        vector: &[u8],
        norm: f32,
    ) -> Result<(), AppError> {
        self.require_writer()?;
        sql::ConnectorPutEmbedding::builder()
            .unit_id(unit_id)
            .generation(generation)
            .dimensions(
                i64::try_from(vector.len())
                    .map_err(|_| AppError::internal("embedding dimensions overflow"))?,
            )
            .norm(f64::from(norm))
            .vector(vector)
            .build()
            .execute(&self.connection)
            .map_err(AppError::database)?;
        Ok(())
    }

    /// Every unit has a vector of this generation.
    pub(crate) fn connector_embeddings_complete(&self, generation: &str) -> Result<bool, AppError> {
        Ok(sql::ConnectorEmbeddingsComplete::builder()
            .generation(generation)
            .build()
            .query_one(&self.connection)
            .map_err(AppError::database)?
            .complete
            != 0)
    }
}

impl Storage {
    /// The indexed text behind a connector anchor, for contextual reranking.
    pub(crate) fn connector_document(&self, anchor: &str) -> Result<String, AppError> {
        let (_, source_id, id) = parse_anchor(anchor)
            .ok_or_else(|| AppError::database_data("invalid connector anchor"))?;
        Ok(evidence(&self.connection, source_id, id)?
            .ok_or_else(|| AppError::database_data("connector index references missing silver"))?
            .text())
    }

    pub(crate) fn connector_view(
        &self,
        anchor: &str,
        context: u32,
    ) -> Result<Vec<super::Message>, AppError> {
        view(&self.connection, anchor, context)
    }
}

/// The digest covers kind, group and ancestry too, so moves and renames re-index.
pub(crate) fn unit_digest_input(unit: &Unit) -> String {
    format!(
        "{}\0{}\0{}\0{}\0{}",
        unit.object.kind, unit.group_anchor, unit.ancestry, unit.text, unit.sequence
    )
}

/// Each container's entry positions, computed once per search: the order
/// `view` reads them in (creation time, then ID), from indexed columns only.
#[derive(Default)]
pub(crate) struct Positions(HashMap<(String, String), HashMap<String, i64>>);

impl Positions {
    fn of(
        &mut self,
        connection: &Connection,
        source_id: &str,
        container: &str,
        id: &str,
    ) -> Result<Option<i64>, AppError> {
        let key = (source_id.to_owned(), container.to_owned());
        if !self.0.contains_key(&key) {
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
            let positions = entries
                .into_iter()
                .enumerate()
                .map(|(position, entry)| (entry.id, i64::try_from(position).unwrap_or(i64::MAX)))
                .collect();
            self.0.insert(key.clone(), positions);
        }
        Ok(self
            .0
            .get(&key)
            .and_then(|positions| positions.get(id))
            .copied())
    }
}

/// One connector object as a search hit with its ancestry.
pub(crate) fn connector_hit(
    connection: &Connection,
    source_id: &str,
    id: &str,
    positions: &mut Positions,
) -> Result<SearchHit, AppError> {
    let evidence = evidence(connection, source_id, id)?
        .ok_or_else(|| AppError::database_data("connector index references missing silver"))?;
    let (group_created_at, group_updated_at) = evidence.group_times();
    let own = anchor(evidence.object.role, source_id, id);
    let group = if evidence.object.role == Role::Entry {
        evidence
            .ancestors
            .iter()
            .find(|(role, _)| *role == Role::Container)
            .map_or_else(
                || own.clone(),
                |(_, container)| anchor(Role::Container, source_id, container.id.as_str()),
            )
    } else {
        own.clone()
    };
    Ok(SearchHit {
        lookup: None,
        lexical_match: None,
        id: own,
        conversation_id: group,
        source: Source::Connector(source_id.to_owned()),
        object: evidence.object_type(),
        ancestors: evidence.ancestors(source_id),
        role: None,
        author_kind: evidence.author(),
        author: crate::source::author(None, evidence.author()),
        sequence: match (evidence.object.role, evidence.group()) {
            (Role::Entry, Some(container)) => {
                positions.of(connection, source_id, container.id.as_str(), id)?
            }
            _ => None,
        },
        content: evidence.content().to_owned(),
        created_at: evidence.object.created_at,
        metadata: crate::search_results::ChatMetadata {
            enrichment: None,
            title: evidence.title().map(str::to_owned),
            project: None,
            repository: None,
            created_at: group_created_at,
            updated_at: group_updated_at.or(evidence.object.updated_at),
        },
        copies: Vec::new(),
        lexical_score: None,
        semantic_score: None,
        fusion_score: 0.0,
        rerank_score: None,
        node_id: None,
    })
}

/// An entry with up to `context` chronological neighbours among the entries of
/// its nearest container (segments included), or a document on its own.
/// Unknown anchors yield nothing.
pub(crate) fn view(
    connection: &Connection,
    target: &str,
    context: u32,
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
            let mut entries = sql::ConnectorEntryOrder::builder()
                .conversation_id(container.id.as_str())
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
            ancestry: String::new(),
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
