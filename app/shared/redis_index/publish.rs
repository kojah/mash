//! Stage and atomically commit bounded shared-index publications.
use super::fields;
use super::{
    AppError, CANONICAL_CHUNK_BYTES, CanonicalManifest, Data, Forget, Forgotten, Head, Identity,
    Indexed, Plan, STAGE_TTL, Store, catalog_key, conflict, data_key, document_key, encode, prefix,
    redis_error, state_key, valid_id, vector_bytes,
};
use crate::shared::snapshot::{Encoded, encoding};
use crate::shared::snapshot::{forget, plan};
impl Store {
    async fn stage_json(
        &self,
        key: String,
        encoded: &Encoded,
        keys: &mut Vec<String>,
    ) -> Result<(), AppError> {
        let manifest = CanonicalManifest {
            bytes: encoded.bytes,
            chunks: encoded.bytes.div_ceil(CANONICAL_CHUNK_BYTES),
            digest: encoded.digest.clone(),
        };
        let mut reader = encoded.reader()?;
        for chunk in 0..manifest.chunks {
            let bytes = encoding::read_bytes(&mut reader, CANONICAL_CHUNK_BYTES).await?;
            let chunk_key = format!("{key}:chunk:{chunk}");
            redis::cmd("SET")
                .arg(&chunk_key)
                .arg(bytes)
                .arg("EX")
                .arg(STAGE_TTL)
                .query_async::<()>(&mut self.connection.clone())
                .await
                .map_err(redis_error)?;
            keys.push(chunk_key);
        }
        redis::cmd("SET")
            .arg(&key)
            .arg(encode(&manifest)?)
            .arg("EX")
            .arg(STAGE_TTL)
            .query_async::<()>(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        keys.push(key);
        Ok(())
    }
    pub(in crate::shared) async fn index(
        &self,
        identity: &Identity,
        file: tempfile::NamedTempFile,
    ) -> Result<Indexed, AppError> {
        let incoming = file
            .as_file()
            .metadata()
            .map_err(|_| AppError::internal("inspect index upload"))?
            .len();
        if incoming > self.max_node_bytes() {
            return Err(AppError::usage(
                "Redis upload exceeds the configured snapshot limit",
            ));
        }
        let pending = self.pending_snapshot(identity).await?;
        let reservation = self
            .snapshot_budget
            .reserve(pending.encoded_bytes(), incoming)?;
        let dimensions = identity.dimensions;
        let (plan, _reservation) = crate::blocking(move || {
            plan(&file, pending.decode()?, dimensions).map(|plan| (plan, reservation))
        })
        .await?;
        let instance = plan
            .loaded
            .head
            .as_ref()
            .map(|h| h.instance.clone())
            .ok_or_else(|| AppError::internal("missing index instance"))?;
        self.commit(identity, plan, instance, true).await
    }
    pub(in crate::shared) async fn forget(
        &self,
        identity: &Identity,
        input: Forget,
    ) -> Result<Forgotten, AppError> {
        valid_id(&input.instance)?;
        if input.ids.len() > 10000 {
            return Err(AppError::usage("too many forget identities"));
        }
        let pending = self.pending_snapshot(identity).await?;
        let reservation = self.snapshot_budget.reserve(pending.encoded_bytes(), 0)?;
        let instance = input.instance.clone();
        let (plan, forgotten, _reservation) = crate::blocking(move || {
            let loaded = pending.decode()?;
            let accepted = loaded.head.as_ref().map_or(0, |h| h.revision);
            if loaded
                .head
                .as_ref()
                .is_some_and(|h| h.instance != input.instance)
                || input.revision.is_some_and(|r| r <= accepted)
            {
                return Err(conflict("node identity or revision conflict"));
            }
            let revision = input.revision.unwrap_or(
                accepted
                    .checked_add(1)
                    .ok_or_else(|| conflict("node revision exhausted"))?,
            );
            let mut plan = Plan {
                loaded,
                revision,
                changed: false,
                removed: 0,
                purged: 0,
            };
            let mut forgotten = false;
            for id in &input.ids {
                forgotten |= forget(&mut plan.loaded.data, id)?;
            }
            plan.changed = forgotten;
            if let Some(head) = &mut plan.loaded.head {
                head.source_silver = None;
            }
            Ok((plan, forgotten, reservation))
        })
        .await?;
        self.commit(identity, plan, instance, false).await?;
        Ok(Forgotten { forgotten })
    }
    #[allow(
        clippy::too_many_lines,
        reason = "bounded document pipelines and stage manifest must finish before the atomic visibility change"
    )]
    async fn stage(
        &self,
        identity: &Identity,
        data: &Data,
        encoded: &Encoded,
        catalog: &Encoded,
        version: &str,
    ) -> Result<Vec<String>, AppError> {
        let mut keys = Vec::new();
        // Metadata has its own bounded payload; catalog reads need no canonical
        // message bodies or vectors. Both payloads use the same stage lifecycle.
        self.stage_json(data_key(&identity.group_id, version), encoded, &mut keys)
            .await?;
        self.stage_json(catalog_key(&identity.group_id, version), catalog, &mut keys)
            .await?;
        let state = data.state(identity);
        let state_key = state_key(&identity.group_id, version);
        let mut pipe = redis::pipe();
        pipe.cmd("SET")
            .arg(&state_key)
            .arg(encode(&state)?)
            .arg("EX")
            .arg(STAGE_TTL)
            .ignore();
        pipe.query_async::<()>(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        keys.push(state_key);
        let mut pipe = redis::pipe();
        let mut queued = 0;
        for message in data.messages.values() {
            let conversation = data
                .conversations
                .get(&message.conversation_id)
                .ok_or_else(|| AppError::internal("orphan Redis message"))?;
            let key = document_key(&identity.group_id, version, &message.id);
            keys.push(key.clone());
            pipe.cmd("HSET")
                .arg(&key)
                .arg(fields::VERSION)
                .arg(version)
                .arg(fields::NODE_ID)
                .arg(&identity.node_id)
                .arg(fields::NODE_TAG)
                .arg(crate::search_filter::tag(&identity.node_id))
                .arg(fields::ROLE_TAG)
                .arg(crate::search_filter::tag(message.role.as_str()))
                .arg(fields::PROJECT_TAG)
                .arg(crate::search_filter::tag(
                    conversation.project.as_deref().unwrap_or_default(),
                ))
                .arg(fields::REPOSITORY_TAG)
                .arg(crate::search_filter::tag(
                    conversation.repository.as_deref().unwrap_or_default(),
                ))
                .arg(fields::DATED)
                .arg(u8::from(message.created_at.is_some()))
                .arg(fields::CONVERSATION)
                .arg(
                    blake3::hash(message.conversation_id.as_bytes())
                        .to_hex()
                        .to_string(),
                )
                .arg(fields::PROVIDER)
                .arg(conversation.provider.as_str())
                .arg(fields::PROVIDER_TAG)
                .arg(conversation.provider.redis_tag())
                .arg(fields::ORDINAL)
                .arg(message.ordinal)
                .arg(fields::CONTEXT_PARENT)
                .arg(message.context.parent().unwrap_or(-2))
                .arg(fields::CREATED_AT)
                .arg(message.created_at.unwrap_or(i64::MIN))
                .arg(fields::SEARCHABLE)
                .arg(u8::from(!message.projection.is_empty()))
                .arg(fields::PROJECTION)
                .arg(&message.projection)
                .arg(fields::CONVERSATION_RECORD)
                .arg(encode(conversation)?)
                .arg(fields::RECORD)
                .arg(encode(message)?);
            if let Some(vector) = &message.vector {
                pipe.arg(fields::VECTOR).arg(vector_bytes(vector));
            }
            pipe.ignore()
                .cmd("EXPIRE")
                .arg(&key)
                .arg(STAGE_TTL)
                .ignore();
            queued += 1;
            if queued == 128 {
                pipe.query_async::<()>(&mut self.connection.clone())
                    .await
                    .map_err(redis_error)?;
                pipe.clear();
                queued = 0;
            }
        }
        if queued > 0 {
            pipe.query_async::<()>(&mut self.connection.clone())
                .await
                .map_err(redis_error)?;
        }
        let manifest = format!("{}stage:{version}:keys", prefix(&identity.group_id));
        for chunk in keys.chunks(128) {
            redis::pipe()
                .cmd("RPUSH")
                .arg(&manifest)
                .arg(chunk)
                .ignore()
                .cmd("EXPIRE")
                .arg(&manifest)
                .arg(STAGE_TTL)
                .ignore()
                .query_async::<()>(&mut self.connection.clone())
                .await
                .map_err(redis_error)?;
        }
        keys.push(manifest);
        Ok(keys)
    }
    async fn commit(
        &self,
        identity: &Identity,
        plan: Plan,
        instance: String,
        checked: bool,
    ) -> Result<Indexed, AppError> {
        // Redis Lua cjson emits numbers with 14 significant digits.
        if !(1..=99_999_999_999_999).contains(&plan.revision) {
            return Err(AppError::usage("invalid Redis revision"));
        }
        let max_bytes = self.max_node_bytes;
        let (plan, encoded, catalog) = crate::blocking(move || {
            let encoded = Encoded::new(&plan.loaded.data.layers(), max_bytes)?;
            let catalog = Encoded::new(&plan.loaded.data.conversations, max_bytes)?;
            Ok((plan, encoded, catalog))
        })
        .await?;
        let counts = plan.loaded.data.counts(identity);
        let mut random = [0u8; 32];
        getrandom::fill(&mut random)
            .map_err(|_| AppError::internal("create Redis staging identity"))?;
        let new_version = blake3::hash(&random).to_hex().to_string();
        let previous = plan.loaded.head.as_ref();
        let reuse = previous.is_some()
            && previous.is_some_and(|head| head.catalog_ready)
            && !plan.changed
            && plan.loaded.digest.as_deref() == Some(encoded.digest.as_str());
        let version = if reuse {
            previous.unwrap().version.clone()
        } else {
            new_version
        };
        let keys = if reuse {
            vec![
                data_key(&identity.group_id, &version),
                state_key(&identity.group_id, &version),
                catalog_key(&identity.group_id, &version),
            ]
        } else {
            self.stage(identity, &plan.loaded.data, &encoded, &catalog, &version)
                .await?
        };
        let head = Head {
            source_silver: previous.and_then(|h| h.source_silver.clone()),
            snapshot_schema: crate::shared::snapshot::layers::Schema::Layered,
            instance,
            generation: identity.generation.clone(),
            revision: plan.revision,
            version: version.clone(),
            counts: counts.clone(),
            last_synced_at: previous.and_then(|h| h.last_synced_at.clone()),
            last_updated_at: previous.and_then(|h| h.last_updated_at.clone()),
            metadata_ready: !reuse || previous.is_some_and(|h| h.metadata_ready),
            catalog_ready: true,
        };
        let mut command = redis::cmd("EVAL");
        command
            .arg(include_str!("../redis/commit.lua"))
            .arg(keys.len() + 1)
            .arg(format!("{}nodes", prefix(&identity.group_id)));
        for key in &keys {
            command.arg(key);
        }
        command
            .arg(&identity.node_id)
            .arg(&plan.loaded.expected)
            .arg(encode(&head)?)
            .arg(u8::from(checked))
            .arg(u8::from(plan.changed))
            .arg(
                previous
                    .filter(|p| p.version != version && !p.version.is_empty())
                    .map_or_else(String::new, |p| {
                        format!("{}stage:{}:keys", prefix(&identity.group_id), p.version)
                    }),
            );
        command
            .query_async::<i64>(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        Ok(Indexed {
            counts,
            revision: plan.revision,
            removed_messages: plan.removed,
            purged_conversations: plan.purged,
        })
    }
}
