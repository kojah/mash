//! Read candidates, context, readiness, and node freshness.
use super::{
    AppError, Counts, Freshness, Head, Identity, Message, MessageRecord, NodeFreshness, NodeState,
    Provider, Search, SearchHit, Store, decode, document_prefix, index_name, prefix, protocol,
    redis_error, timestamp, valid_id, vector_bytes,
};
use std::collections::HashMap;
impl Store {
    pub(in crate::shared) async fn status(&self, identity: &Identity) -> Result<Counts, AppError> {
        Ok(self
            .head(identity)
            .await?
            .0
            .map_or_else(Counts::default, |h| h.counts))
    }
    pub(in crate::shared) async fn freshness(
        &self,
        identity: &Identity,
    ) -> Result<Freshness, AppError> {
        let heads: HashMap<String, String> = redis::cmd("HGETALL")
            .arg(format!("{}nodes", prefix(&identity.group_id)))
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        let mut nodes = Vec::new();
        for (node_id, raw) in heads {
            let head: Head = decode(&raw)?;
            nodes.push(NodeFreshness {
                node_id,
                revision: head.revision,
                last_synced_at: timestamp(head.last_synced_at)?,
                last_updated_at: timestamp(head.last_updated_at)?,
                conversations: i64::try_from(head.counts.conversations).unwrap_or(i64::MAX),
                messages: i64::try_from(head.counts.messages).unwrap_or(i64::MAX),
            });
        }
        nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        Ok(Freshness {
            group_id: identity.group_id.clone(),
            nodes,
        })
    }
    pub(in crate::shared) async fn state(
        &self,
        identity: &Identity,
    ) -> Result<NodeState, AppError> {
        let script = "local raw=redis.call('HGET',KEYS[1],ARGV[1]); if not raw then return nil end; local h=cjson.decode(raw); local state=redis.call('GET',ARGV[2]..h.version..':state'); if not state then return redis.error_reply('MASH_STAGE_MISSING') end; return {raw,state}";
        let result: Option<(String, String)> = redis::cmd("EVAL_RO")
            .arg(script)
            .arg(1)
            .arg(format!("{}nodes", prefix(&identity.group_id)))
            .arg(&identity.node_id)
            .arg(format!("{}stage:", prefix(&identity.group_id)))
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        if let Some((head, state)) = result {
            let head: Head = decode(&head)?;
            let mut state: NodeState = decode(&state)?;
            state.instance = Some(head.instance);
            state.revision = head.revision;
            if head.generation != identity.generation {
                for source in &mut state.sources {
                    source.ready = false;
                }
            }
            Ok(state)
        } else {
            Ok(NodeState {
                instance: None,
                revision: 0,
                sources: Vec::new(),
                tombstones: Vec::new(),
            })
        }
    }
    pub(in crate::shared) async fn search(
        &self,
        identity: &Identity,
        input: Search,
    ) -> Result<protocol::Candidates, AppError> {
        if input.query.trim().is_empty()
            || input.query.len() > 16384
            || !protocol::valid_vector(&input.vector, identity.dimensions)
            || input.generation != identity.generation
        {
            return Err(AppError::usage("invalid search query or vector"));
        }

        if let Some(filter) = &input.filter {
            filter.validate()?;
        }
        let words = input
            .query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        let lexical = if words.is_empty() {
            String::new()
        } else {
            format!("@projection:({}) @searchable:[1 1]", words.join(" "))
        };
        let mut filters = String::new();
        if let Some(provider) = input.provider {
            filters.push_str(if provider == Provider::Codex {
                " @provider_tag:{x}"
            } else {
                " @provider_tag:{c}"
            });
        }
        if let Some(days) = input.days {
            use std::fmt::Write;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let cutoff = i64::try_from(now)
                .unwrap_or(i64::MAX)
                .saturating_sub(i64::from(days) * 86400)
                .saturating_mul(1000);
            write!(&mut filters, " @created_at:[{cutoff} +inf]")
                .map_err(|_| AppError::internal("format search filter"))?;
        }
        if let Some(filter) = &input.filter {
            filters.push(' ');
            filters.push_str(&filter.redis());
        }
        let (lexical, semantic): (redis::Value, redis::Value) = redis::cmd("EVAL_RO")
            .arg(include_str!("../redis/search.lua"))
            .arg(2)
            .arg(format!("{}nodes", prefix(&identity.group_id)))
            .arg(index_name(&identity.group_id))
            .arg(lexical)
            .arg(filters)
            .arg(vector_bytes(&input.vector))
            .arg(&identity.generation)
            .arg(if input.exclude_self {
                identity.node_id.as_str()
            } else {
                ""
            })
            .arg(input.unit.candidate_limit())
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        let (lexical, mut documents) = hits(lexical, true)?;
        let (semantic, other) = hits(semantic, false)?;
        documents.extend(other);
        let documents = documents
            .into_iter()
            .map(|((node_id, id), projection)| protocol::Document {
                node_id,
                id,
                projection,
            })
            .collect();
        Ok(protocol::Candidates {
            lexical,
            semantic,
            documents,
        })
    }
    pub(in crate::shared) async fn view(
        &self,
        identity: &Identity,
        input: protocol::View,
    ) -> Result<Vec<Message>, AppError> {
        if input.context > 1000 {
            return Err(AppError::usage("invalid view context"));
        }
        valid_id(&input.id)?;
        let value: redis::Value = redis::cmd("EVAL_RO")
            .arg(include_str!("../redis/view.lua"))
            .arg(2)
            .arg(format!("{}nodes", prefix(&identity.group_id)))
            .arg(index_name(&identity.group_id))
            .arg(input.node_id)
            .arg(document_prefix(&identity.group_id))
            .arg(blake3::hash(input.id.as_bytes()).to_hex().to_string())
            .arg(input.context)
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        let redis::Value::Array(values) = value else {
            return Err(AppError::shared("invalid Redis search response"));
        };
        if values.is_empty() {
            return Err(AppError::shared("invalid Redis view response"));
        }
        let mut result = Vec::new();
        for pair in values[1..].chunks(2) {
            if pair.len() != 2 {
                return Err(AppError::shared("invalid Redis view response"));
            }
            let fields: HashMap<String, String> = redis::from_redis_value(pair[1].clone())
                .map_err(|_| AppError::shared("invalid Redis response"))?;
            let message: MessageRecord = decode(
                fields
                    .get("record")
                    .ok_or_else(|| AppError::shared("missing Redis view record"))?,
            )?;
            result.push(Message {
                id: message.id,
                ordinal: message.ordinal,
                role: message.role,
                content: message.content,
                created_at: message.created_at,
            });
        }
        result.sort_by_key(|m| m.ordinal);
        Ok(result)
    }
}
type Documents = HashMap<(String, String), String>;
fn hits(value: redis::Value, lexical: bool) -> Result<(Vec<SearchHit>, Documents), AppError> {
    let redis::Value::Array(values) = value else {
        return Err(AppError::shared("invalid Redis search response"));
    };
    if values.is_empty() {
        return Err(AppError::shared("invalid Redis search response"));
    }
    let mut hits = Vec::new();
    let mut documents = HashMap::new();
    for row in values[1..].chunks(3) {
        if row.len() != 3 {
            return Err(AppError::shared("invalid Redis candidate response"));
        }
        let fields: HashMap<String, String> = redis::from_redis_value(row[2].clone())
            .map_err(|_| AppError::shared("invalid Redis response"))?;
        let message: MessageRecord = decode(
            fields
                .get("record")
                .ok_or_else(|| AppError::shared("missing Redis candidate record"))?,
        )?;
        let node_id = fields
            .get("node_id")
            .ok_or_else(|| AppError::shared("missing Redis candidate owner"))?
            .clone();
        let provider = fields
            .get("provider")
            .ok_or_else(|| AppError::shared("missing Redis candidate provider"))?
            .clone();
        let score: f64 = redis::from_redis_value(row[1].clone())
            .map_err(|_| AppError::shared("invalid Redis response"))?;
        let distance = fields
            .get("distance")
            .map(|v| v.parse::<f32>())
            .transpose()
            .map_err(|_| AppError::shared("invalid Redis vector distance"))?;
        documents.insert(
            (node_id.clone(), message.id.clone()),
            message.projection.clone(),
        );
        hits.push(SearchHit {
            node_id: Some(node_id),
            id: message.id,
            conversation_id: message.conversation_id,
            provider: provider.parse()?,
            role: message.role,
            content: message.projection,
            copies: Vec::new(),
            created_at: message.created_at,
            metadata: fields
                .get("conversation_record")
                .map(|raw| {
                    let c: super::ConversationRecord = decode(raw)?;
                    Ok::<_, AppError>(crate::search_results::ThreadMetadata {
                        title: c.title,
                        project: c.project,
                        repository: c.repository,
                        created_at: c.created_at,
                        updated_at: c.updated_at,
                    })
                })
                .transpose()?
                .unwrap_or_default(),
            lexical_score: lexical.then_some(-score),
            semantic_score: distance.map(|d| 1.0 - d),
            fusion_score: 0.0,
            rerank_score: None,
        });
    }
    hits.sort_by(|a, b| {
        if lexical {
            a.lexical_score
                .partial_cmp(&b.lexical_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            b.semantic_score
                .partial_cmp(&a.semantic_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
        .then_with(|| a.node_id.cmp(&b.node_id))
        .then_with(|| a.id.cmp(&b.id))
    });
    Ok((hits, documents))
}
