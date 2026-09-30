//! Decode the RESP2 FT.SEARCH replies returned by our Lua scripts.
use super::{
    AppError, ConversationRecord, Message, MessageRecord, Provider, SearchHit, decode, fields,
};
use crate::search_results::ThreadMetadata;
use redis::Value;
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub(super) enum Ranking {
    Lexical,
    Semantic,
}

#[derive(Default)]
struct ReturnedFields<'a> {
    record: Option<&'a str>,
    node_id: Option<&'a str>,
    provider: Option<&'a str>,
    projection: Option<&'a str>,
    conversation: Option<&'a str>,
    distance: Option<&'a str>,
}

fn invalid() -> AppError {
    // Never include stored history or credentials in decoder errors.
    AppError::shared("invalid Redis Search response; check the index schema and stored records")
}

fn text(value: &Value) -> Result<&str, AppError> {
    match value {
        Value::BulkString(bytes) => std::str::from_utf8(bytes).map_err(|_| invalid()),
        Value::SimpleString(text) => Ok(text),
        _ => Err(invalid()),
    }
}

fn rows(value: &Value, width: usize) -> Result<std::slice::ChunksExact<'_, Value>, AppError> {
    let Value::Array(values) = value else {
        return Err(invalid());
    };
    let Some(Value::Int(total)) = values.first() else {
        return Err(invalid());
    };
    let total = usize::try_from(*total).map_err(|_| invalid())?;
    let rows = values[1..].chunks_exact(width);
    if !rows.remainder().is_empty() || total < rows.len() {
        return Err(invalid());
    }
    let mut keys = HashSet::new();
    for row in rows.clone() {
        let key = text(&row[0])?;
        if key.is_empty() || !keys.insert(key) {
            return Err(invalid());
        }
    }
    Ok(rows)
}

fn returned_fields(value: &Value) -> Result<ReturnedFields<'_>, AppError> {
    let Value::Array(values) = value else {
        return Err(invalid());
    };
    let (pairs, remainder) = values.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(invalid());
    }
    let mut result = ReturnedFields::default();
    let mut seen = HashSet::new();
    for pair in pairs {
        let name = text(&pair[0])?;
        if name.is_empty() || !seen.insert(name) {
            return Err(invalid());
        }
        let slot = match name {
            fields::RECORD => &mut result.record,
            fields::NODE_ID => &mut result.node_id,
            fields::PROVIDER => &mut result.provider,
            fields::PROJECTION => &mut result.projection,
            fields::CONVERSATION_RECORD => &mut result.conversation,
            fields::DISTANCE => &mut result.distance,
            // Additive search fields don't change the stored-record contract.
            _ => continue,
        };
        *slot = Some(text(&pair[1])?);
    }
    Ok(result)
}

fn message(fields: &ReturnedFields<'_>) -> Result<MessageRecord, AppError> {
    let message: MessageRecord = decode(fields.record.ok_or_else(invalid)?)?;
    if message.id.is_empty() || message.conversation_id.is_empty() || message.ordinal < 0 {
        return Err(invalid());
    }
    Ok(message)
}

fn score(value: &Value) -> Result<f64, AppError> {
    let value = match value {
        Value::Double(value) => *value,
        Value::BulkString(_) | Value::SimpleString(_) => {
            text(value)?.parse().map_err(|_| invalid())?
        }
        _ => return Err(invalid()),
    };
    if !value.is_finite() {
        return Err(invalid());
    }
    Ok(value)
}

pub(super) fn candidates(value: &Value, ranking: Ranking) -> Result<Vec<SearchHit>, AppError> {
    rows(value, 3)?
        .map(|row| {
            let fields = returned_fields(&row[2])?;
            let message = message(&fields)?;
            let node_id = fields
                .node_id
                .filter(|s| !s.is_empty())
                .ok_or_else(invalid)?;
            let provider: Provider = fields
                .provider
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?;
            if fields.projection.ok_or_else(invalid)? != message.projection {
                return Err(invalid());
            }
            let score = score(&row[1])?;
            let distance = fields
                .distance
                .map(|s| s.parse::<f32>().map_err(|_| invalid()))
                .transpose()?;
            // FLOAT32 cosine distance can have small rounding error at 0/2.
            if distance.is_some_and(|d| !d.is_finite() || !(-0.0001..=2.0001).contains(&d)) {
                return Err(invalid());
            }
            let (lexical_score, semantic_score) = match ranking {
                Ranking::Lexical => (Some(-score), None),
                Ranking::Semantic => (None, Some(1.0 - distance.ok_or_else(invalid)?)),
            };
            let metadata = fields
                .conversation
                .map(|raw| {
                    let conversation: ConversationRecord = decode(raw)?;
                    if conversation.id != message.conversation_id
                        || conversation.provider != provider
                    {
                        return Err(invalid());
                    }
                    Ok(ThreadMetadata {
                        title: conversation.title,
                        project: conversation.project,
                        repository: conversation.repository,
                        created_at: conversation.created_at,
                        updated_at: conversation.updated_at,
                    })
                })
                .transpose()?
                .unwrap_or_default();
            Ok(SearchHit {
                node_id: Some(node_id.to_owned()),
                id: message.id,
                conversation_id: message.conversation_id,
                provider,
                role: message.role,
                content: message.projection,
                copies: Vec::new(),
                created_at: message.created_at,
                metadata,
                lexical_score,
                semantic_score,
                fusion_score: 0.0,
                rerank_score: None,
            })
        })
        .collect()
}

pub(super) fn view(value: &Value) -> Result<Vec<Message>, AppError> {
    let mut conversation = None;
    let mut ordinals = HashSet::new();
    let mut ids = HashSet::new();
    let mut messages = Vec::new();
    for row in rows(value, 2)? {
        let message = message(&returned_fields(&row[1])?)?;
        if conversation
            .as_ref()
            .is_some_and(|id| id != &message.conversation_id)
            || !ordinals.insert(message.ordinal)
            || !ids.insert(message.id.clone())
        {
            return Err(invalid());
        }
        conversation = Some(message.conversation_id);
        messages.push(Message {
            id: message.id,
            ordinal: message.ordinal,
            role: message.role,
            content: message.content,
            created_at: message.created_at,
        });
    }
    messages.sort_by_key(|m| m.ordinal);
    Ok(messages)
}

#[cfg(test)]
mod tests;
