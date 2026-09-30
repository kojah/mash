//! Repair old metadata once per immutable snapshot; interrupted repairs resume.
use super::{AppError, Identity, Store, document_key, encode, fields, prefix, redis_error};

impl Store {
    pub(in crate::shared) async fn repair_metadata(
        &self,
        identity: &Identity,
    ) -> Result<(), AppError> {
        let Some(head) = self.head(identity).await?.0 else {
            return Ok(());
        };
        if head.metadata_ready {
            return Ok(());
        }
        let loaded = self.load(identity).await?;
        let expected = loaded.expected;
        let mut head = loaded.head.ok_or_else(|| {
            super::conflict("shared snapshot changed during metadata repair; restart the server")
        })?;
        if head.metadata_ready {
            return Ok(());
        }
        let group = identity.group_id.clone();
        let version = head.version.clone();
        let records = crate::blocking(move || {
            let mut records = Vec::with_capacity(loaded.data.messages.len());
            for message in loaded.data.messages.values() {
                let conversation = loaded
                    .data
                    .conversations
                    .get(&message.conversation_id)
                    .ok_or_else(|| {
                        AppError::shared("shared snapshot has missing conversation metadata")
                    })?;
                records.push((
                    document_key(&group, &version, &message.id),
                    encode(conversation)?,
                ));
            }
            Ok(records)
        })
        .await?;
        let mut connection = self.connection.clone();
        for chunk in records.chunks(256) {
            let mut pipeline = redis::pipe();
            for (key, payload) in chunk {
                // A concurrent publication can retire this stage. Never recreate expired hashes.
                pipeline.cmd("EVAL")
                    .arg("if redis.call('EXISTS',KEYS[1])==1 then return redis.call('HSETNX',KEYS[1],ARGV[1],ARGV[2]) end return 0")
                    .arg(1).arg(key).arg(fields::CONVERSATION_RECORD).arg(payload).ignore();
            }
            pipeline
                .query_async::<()>(&mut connection)
                .await
                .map_err(redis_error)?;
        }
        head.metadata_ready = true;
        // Preserve revision/freshness and acknowledge only the exact snapshot repaired.
        redis::cmd("EVAL")
            .arg("if redis.call('HGET',KEYS[1],ARGV[1])==ARGV[2] then redis.call('HSET',KEYS[1],ARGV[1],ARGV[3]); return 1 end return 0")
            .arg(1).arg(format!("{}nodes", prefix(&identity.group_id)))
            .arg(&identity.node_id).arg(expected).arg(encode(&head)?)
            .query_async::<i64>(&mut connection).await.map_err(redis_error)?;
        Ok(())
    }
}
