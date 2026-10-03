//! Read candidates, context, readiness, and node freshness.
use super::fields;
use super::{
    AppError, Counts, Freshness, Head, Identity, Message, NodeFreshness, NodeState, Search,
    SearchHit, Store, decode, document_prefix, index_name, prefix, protocol, redis_error, response,
    timestamp, valid_id, vector_bytes,
};
use std::collections::HashMap;
use std::fmt::Write;
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
            state.source_silver = head.source_silver;
            if head.generation != identity.generation {
                for source in &mut state.sources {
                    source.ready = false;
                }
            }
            Ok(state)
        } else {
            Ok(NodeState {
                source_silver: None,
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
        crate::search_query::validate(&input.query)?;
        if !crate::search_query::has_terms(&input.query)
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
            format!(
                "@{}:({}) @{}:[1 1]",
                fields::PROJECTION,
                words.join(" "),
                fields::SEARCHABLE
            )
        };
        let mut filters = String::new();
        if let Some(provider) = input.provider {
            write!(
                &mut filters,
                " @{}:{{{}}}",
                fields::PROVIDER_TAG,
                provider.redis_tag()
            )
            .map_err(|_| AppError::internal("format search filter"))?;
        }
        if let Some(days) = input.days {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let cutoff = i64::try_from(now)
                .unwrap_or(i64::MAX)
                .saturating_sub(i64::from(days) * 86400)
                .saturating_mul(1000);
            write!(&mut filters, " @{}:[{cutoff} +inf]", fields::CREATED_AT)
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
            .arg(self.vector_index.query_attributes())
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        let (lexical, mut documents) = hits(&lexical, response::Ranking::Lexical)?;
        let (semantic, other) = hits(&semantic, response::Ranking::Semantic)?;
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
        match input.content {
            crate::storage::MessageContent::Canonical => response::view(&value),
            crate::storage::MessageContent::SearchProjection => {
                response::view_content(&value, input.content)
            }
        }
    }
}
type Documents = HashMap<(String, String), String>;
fn hits(
    value: &redis::Value,
    ranking: response::Ranking,
) -> Result<(Vec<SearchHit>, Documents), AppError> {
    let mut hits = response::candidates(value, ranking)?;
    let documents = hits
        .iter()
        .map(|hit| {
            (
                (hit.node_id.clone().unwrap_or_default(), hit.id.clone()),
                hit.content.clone(),
            )
        })
        .collect();
    hits.sort_by(|a, b| {
        match ranking {
            response::Ranking::Lexical => a
                .lexical_score
                .unwrap_or_default()
                .total_cmp(&b.lexical_score.unwrap_or_default()),
            response::Ranking::Semantic => b
                .semantic_score
                .unwrap_or_default()
                .total_cmp(&a.semantic_score.unwrap_or_default()),
        }
        .then_with(|| a.node_id.cmp(&b.node_id))
        .then_with(|| a.id.cmp(&b.id))
    });
    Ok((hits, documents))
}
