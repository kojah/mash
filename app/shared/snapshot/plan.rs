//! Validate canonical uploads and calculate node snapshot changes.
use super::{
    AppError, Batch, Counts, Data, Event, Head, Identity, Loaded, NodeState, Plan, Provider, Start,
    Tombstone, conflict, identity_key, protocol, semantic, valid_id,
};
use std::collections::{HashMap, HashSet};
impl Data {
    pub(in crate::shared) fn counts(&self, identity: &Identity) -> Counts {
        Counts {
            conversations: self.conversations.len() as u64,
            messages: self.messages.len() as u64,
            searchable_messages: self
                .messages
                .values()
                .filter(|m| !m.projection.is_empty())
                .count() as u64,
            embeddings: self
                .messages
                .values()
                .filter(|m| {
                    !m.projection.is_empty()
                        && m.generation.as_deref() == Some(identity.generation.as_str())
                        && m.vector
                            .as_ref()
                            .is_some_and(|v| protocol::valid_vector(v, identity.dimensions))
                })
                .count() as u64,
        }
    }
    pub(in crate::shared) fn state(&self, identity: &Identity) -> NodeState {
        let incompatible: HashSet<_> = self
            .messages
            .values()
            .filter(|m| {
                !m.projection.is_empty()
                    && (m.generation.as_deref() != Some(identity.generation.as_str())
                        || m.vector
                            .as_ref()
                            .is_none_or(|v| !protocol::valid_vector(v, identity.dimensions)))
            })
            .map(|m| m.conversation_id.as_str())
            .collect();
        let sources = self
            .sources
            .values()
            .map(|s| {
                let mut s = s.clone();
                s.ready = s.interpretation_version
                    == crate::silver::certificate::INTERPRETATION_VERSION
                    && s.conversation_id
                        .as_deref()
                        .is_none_or(|id| !incompatible.contains(id));
                s
            })
            .collect();
        NodeState {
            gold_build: crate::gold::config::Build::default(),
            silver_coverage: Vec::new(),
            silver_revision: None,
            silver_scans: Vec::new(),
            source_silver: None,
            instance: None,
            revision: 0,
            sources,
            tombstones: self.tombstones.values().cloned().collect(),
        }
    }
    pub(in crate::shared) fn remove(&mut self, id: &str) -> (u64, u64) {
        let before = self.messages.len();
        self.messages.retain(|_, m| m.conversation_id != id);
        (
            (before - self.messages.len()) as u64,
            u64::from(self.conversations.remove(id).is_some()),
        )
    }
}
pub(in crate::shared) fn forget(data: &mut Data, id: &str) -> Result<bool, AppError> {
    valid_id(id)?;
    let providers = data
        .conversations
        .get(id)
        .map_or_else(|| Provider::ALL.to_vec(), |c| vec![c.provider]);
    let (_, removed) = data.remove(id);
    for provider in providers {
        data.tombstones.insert(
            identity_key(provider, id),
            Tombstone {
                provider,
                conversation_id: id.to_owned(),
            },
        );
    }
    Ok(removed > 0)
}
pub(in crate::shared) fn start(
    loaded: &mut Loaded,
    input: &Start,
) -> Result<(i64, bool), AppError> {
    input.validate_source_silver()?;
    valid_id(&input.instance)?;
    if input.revision.is_some_and(|r| r < 1)
        || input.base_revision.is_some_and(|r| r < 0)
        || input.forgets.len() > 10000
        || (input.snapshot && (input.base_revision.is_some() || !input.forgets.is_empty()))
    {
        return Err(AppError::usage("invalid index metadata"));
    }
    if loaded
        .head
        .as_ref()
        .is_some_and(|h| h.instance != input.instance)
    {
        return Err(conflict(
            "node_id belongs to another database or indexing mode",
        ));
    }
    let accepted = loaded.head.as_ref().map_or(0, |h| h.revision);
    let revision = if input.snapshot {
        let revision = input
            .revision
            .ok_or_else(|| AppError::usage("snapshot requires revision"))?;
        if revision < accepted {
            return Err(conflict("shared index has a newer snapshot"));
        }
        revision
    } else {
        if input.base_revision != Some(accepted) || input.revision.is_some_and(|r| r <= accepted) {
            return Err(conflict(
                "shared index changed while preparing upload; retry indexing",
            ));
        }
        input.revision.unwrap_or(
            accepted
                .checked_add(1)
                .ok_or_else(|| conflict("node revision exhausted"))?,
        )
    };
    if loaded.head.is_none() {
        loaded.head = Some(Head {
            lexical_manifest: None,
            gold_manifest: None,
            silver_coverage: Vec::new(),
            silver_revision: None,
            silver_scans: Vec::new(),
            source_silver: None,
            snapshot_schema: super::layers::Schema::Layered,
            instance: input.instance.clone(),
            generation: semantic::SHARED_EMBEDDING_GENERATION.to_owned(),
            revision: 0,
            version: String::new(),
            counts: Counts::default(),
            last_synced_at: None,
            last_updated_at: None,
            metadata_ready: false,
            catalog_ready: false,
        });
    }
    Ok((revision, input.snapshot && revision == accepted))
}
#[allow(
    clippy::too_many_lines,
    reason = "validate the complete upload before staging any shared data"
)]
pub(in crate::shared) fn plan(
    file: &tempfile::NamedTempFile,
    mut loaded: Loaded,
    identity: &Identity,
    build: &crate::gold::config::Build,
) -> Result<Plan, AppError> {
    use std::io::BufRead;
    let mut lines = std::io::BufReader::new(
        file.reopen()
            .map_err(|_| AppError::internal("open incoming index"))?,
    )
    .lines();
    let first = lines
        .next()
        .ok_or_else(|| AppError::usage("empty index stream"))?
        .map_err(|_| AppError::usage("invalid index stream"))?;
    let Event::Start(input) =
        serde_json::from_str(&first).map_err(|_| AppError::usage("invalid index start"))?
    else {
        return Err(AppError::usage("index stream must start with metadata"));
    };
    let (revision, duplicate) = start(&mut loaded, &input)?;
    if input
        .gold_build
        .as_ref()
        .is_some_and(|requested| requested != build)
    {
        return Err(conflict("shared gold recipe changed; retry indexing"));
    }
    let requested = input.gold_build.clone().unwrap_or_default();
    let previous_build = loaded.head.as_ref().and_then(|head| {
        head.gold_manifest
            .as_ref()
            .filter(|manifest| {
                manifest.matches(
                    &manifest.build,
                    &identity.generation,
                    head.silver_revision.as_ref(),
                )
            })
            .map(|manifest| manifest.build.clone())
    });
    let mut plan = Plan {
        source_build: None,
        loaded,
        revision,
        changed: false,
        removed: 0,
        purged: 0,
    };
    let mut dirty = HashSet::new();
    if !duplicate {
        if let Some(head) = &mut plan.loaded.head {
            head.source_silver.clone_from(&input.source_silver);
        }
        for id in &input.forgets {
            if let Some(c) = plan.loaded.data.conversations.get(id) {
                dirty.insert(c.provider);
            }
            plan.changed |= forget(&mut plan.loaded.data, id)?;
        }
        if input.snapshot {
            plan.loaded.data.conversations.clear();
            plan.loaded.data.messages.clear();
            plan.loaded.data.sources.clear();
            plan.changed = true;
        }
    }
    let mut seen: HashMap<Provider, HashSet<String>> = HashMap::new();
    let mut reconciled = HashMap::new();
    let mut scans = std::collections::BTreeMap::new();
    let mut finished = false;
    let mut supplied = HashSet::new();
    for line in lines {
        if finished {
            return Err(AppError::usage("data after index finish"));
        }
        let event: Event =
            serde_json::from_str(&line.map_err(|_| AppError::usage("invalid index stream"))?)
                .map_err(|_| AppError::usage("invalid index event"))?;
        if matches!(event, Event::Finish) {
            finished = true;
            continue;
        }
        if matches!(event, Event::Start(_)) {
            return Err(AppError::usage("duplicate index start"));
        }
        if duplicate {
            continue;
        }
        let data = &mut plan.loaded.data;
        let affected = super::super::silver_coverage::affected(&event, data);
        if affected.iter().any(|provider| scans.contains_key(provider)) {
            return Err(AppError::usage("provider data after silver scan report"));
        }
        dirty.extend(affected);
        match event {
            Event::Batch(batch) => {
                if batch.len() > 128 {
                    return Err(AppError::usage("index batch exceeds 128 records"));
                }
                match batch {
                    Batch::Tombstones { records } => {
                        for t in records {
                            valid_id(&t.conversation_id)?;
                            if data
                                .conversations
                                .get(&t.conversation_id)
                                .is_some_and(|c| c.provider == t.provider)
                            {
                                data.remove(&t.conversation_id);
                            }
                            data.tombstones
                                .insert(identity_key(t.provider, &t.conversation_id), t);
                            plan.changed = true;
                        }
                    }
                    Batch::Conversations { records } => {
                        for c in records {
                            c.validate_enrichment()?;
                            valid_id(&c.id)?;
                            if !input.snapshot {
                                data.remove(&c.id);
                            } else if data.conversations.contains_key(&c.id) {
                                return Err(AppError::usage("duplicate conversation"));
                            }
                            if !data
                                .tombstones
                                .contains_key(&identity_key(c.provider, &c.id))
                            {
                                supplied.insert(c.id.clone());
                                data.conversations.insert(c.id.clone(), c);
                                plan.changed = true;
                            }
                        }
                    }
                    Batch::Messages { records } => {
                        for m in records {
                            valid_id(&m.id)?;
                            valid_id(&m.conversation_id)?;
                            if m.ordinal < 0
                                || !m.context.valid(m.ordinal)
                                || m.vector.as_ref().is_some_and(|v| {
                                    !protocol::valid_vector(v, identity.dimensions)
                                })
                                || (m.projection.is_empty()
                                    && (m.vector.is_some() || m.generation.is_some()))
                            {
                                return Err(AppError::usage(
                                    "invalid message embedding or ordinal",
                                ));
                            }
                            if data.conversations.contains_key(&m.conversation_id) {
                                if data.messages.insert(m.id.clone(), m).is_some() {
                                    return Err(AppError::usage("duplicate message"));
                                }
                                plan.changed = true;
                            }
                        }
                    }
                }
            }
            Event::Source(s) if !input.snapshot => {
                valid_id(&s.path_hash)?;
                if s.size_bytes < 0
                    || reconciled.contains_key(&s.provider)
                    || scans.contains_key(&s.provider)
                {
                    return Err(AppError::usage("invalid source checkpoint"));
                }
                data.sources
                    .insert(identity_key(s.provider, &s.path_hash), s);
            }
            Event::Seen { provider, hashes } if !input.snapshot => {
                if hashes.len() > 128
                    || reconciled.contains_key(&provider)
                    || scans.contains_key(&provider)
                {
                    return Err(AppError::usage("invalid observed sources"));
                }
                seen.entry(provider).or_default().extend(hashes);
            }
            Event::Reconcile { provider, cutoff } if !input.snapshot => {
                if scans.contains_key(&provider) || reconciled.insert(provider, cutoff).is_some() {
                    return Err(AppError::usage("duplicate reconciliation"));
                }
                let observed = seen.get(&provider);
                let mut missing = Vec::new();
                data.sources.retain(|_, s| {
                    let remove = s.provider == provider
                        && observed.is_none_or(|seen| !seen.contains(&s.path_hash))
                        && cutoff.is_none_or(|c| s.modified_ns >= c);
                    if remove {
                        missing.extend(s.conversation_id.clone());
                    }
                    !remove
                });
                for id in missing {
                    if !data
                        .sources
                        .values()
                        .any(|s| s.conversation_id.as_deref() == Some(&id))
                    {
                        let (m, c) = data.remove(&id);
                        plan.removed += m;
                        plan.purged += c;
                        plan.changed |= c > 0;
                    }
                }
            }
            Event::Remove { id } if !input.snapshot => {
                valid_id(&id)?;
                let (m, c) = data.remove(&id);
                plan.removed += m;
                plan.purged += c;
                plan.changed |= c > 0;
            }
            Event::Scan(report) if !input.snapshot => {
                protocol::record_scan(&mut scans, report, &reconciled)?;
            }
            _ => {
                return Err(AppError::usage(
                    "invalid index event for this indexing mode",
                ));
            }
        }
    }
    if !finished {
        return Err(AppError::usage("incomplete index stream"));
    }
    let mut ordinals = HashSet::new();
    if plan
        .loaded
        .data
        .messages
        .values()
        .any(|m| !ordinals.insert((&m.conversation_id, m.ordinal)))
    {
        return Err(AppError::usage("duplicate message ordinal"));
    }
    if plan.loaded.data.messages.values().any(|m| {
        matches!(m.context, crate::message_context::MessageContext::Parent { ordinal } if !ordinals.contains(&(&m.conversation_id, ordinal)))
    }) {
        return Err(AppError::usage("missing message context parent"));
    }
    if !duplicate && let Some(head) = &mut plan.loaded.head {
        head.silver_coverage = super::super::silver_coverage::update(
            &head.silver_coverage,
            &scans,
            &dirty,
            &plan.loaded.data,
            revision,
            input.snapshot,
        );
        head.silver_scans = scans.into_values().collect();
    }
    plan.source_build = if duplicate {
        previous_build
    } else if previous_build.as_ref() == Some(&requested)
        || plan
            .loaded
            .data
            .messages
            .values()
            .filter(|message| !message.projection.is_empty())
            .all(|message| supplied.contains(&message.conversation_id))
    {
        Some(requested)
    } else {
        None
    };
    Ok(plan)
}
