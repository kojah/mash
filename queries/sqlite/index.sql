-- name: Counts :one
SELECT (SELECT count(*) FROM conversations) AS conversations,
(SELECT count(*) FROM messages) AS messages,
(SELECT count(*) FROM messages WHERE COALESCE(search_projection,content)<>'') AS searchable_messages,
(SELECT count(*) FROM message_embeddings) AS embeddings;

-- name: ConversationMessageState :many
SELECT id,fingerprint,ordinal,COALESCE(search_projection,content) AS search_text FROM messages WHERE conversation_id=sqlc.arg(conversation_id);

-- name: ClaudeSourceThread :one
SELECT conversation_id FROM claude_sources WHERE source_key=sqlc.arg(source_key);

-- name: ClaudeSourceMetadata :many
SELECT source_key,source_path,title,project,repository FROM claude_sources
WHERE conversation_id=sqlc.arg(conversation_id) ORDER BY source_key;

-- name: ClaudeSourceReferences :many
SELECT source_key,source_path,conversation_id FROM claude_sources ORDER BY source_key;

-- name: ClaudeContributionMessages :many
SELECT m.source_key,m.message_id,m.parent_id,m.role,m.content,m.projection,m.created_at
FROM claude_source_messages m JOIN claude_sources s USING(source_key)
WHERE s.conversation_id=sqlc.arg(conversation_id) ORDER BY m.message_id,m.source_key;

-- name: ClaudeSourceScope :many
SELECT s.source_key,s.source_path,c.modified_ns FROM claude_sources s
LEFT JOIN source_checkpoints c ON c.provider='claude-code' AND c.source_key=s.source_key;

-- name: UpsertClaudeSource :exec
INSERT INTO claude_sources(source_key,source_path,conversation_id,title,project,repository)
VALUES(sqlc.arg(source_key),sqlc.arg(source_path),sqlc.arg(conversation_id),sqlc.narg(title),sqlc.narg(project),sqlc.narg(repository))
ON CONFLICT(source_key) DO UPDATE SET source_path=excluded.source_path,
conversation_id=excluded.conversation_id,title=excluded.title,project=excluded.project,repository=excluded.repository;

-- name: DeleteClaudeSourceMessages :exec
DELETE FROM claude_source_messages WHERE source_key=sqlc.arg(source_key);

-- name: InsertClaudeSourceMessage :exec
INSERT INTO claude_source_messages(source_key,message_id,parent_id,role,content,projection,created_at)
VALUES(sqlc.arg(source_key),sqlc.arg(message_id),sqlc.narg(parent_id),sqlc.arg(role),sqlc.arg(content),sqlc.narg(projection),sqlc.narg(created_at));

-- name: DeleteClaudeSource :exec
DELETE FROM claude_sources WHERE source_key=sqlc.arg(source_key);

-- name: DeleteClaudeThreadSources :exec
DELETE FROM claude_sources WHERE conversation_id=sqlc.arg(conversation_id);

-- name: ClaudeTombstone :one
SELECT conversation_id FROM tombstones WHERE provider='claude-code' AND conversation_id=sqlc.arg(conversation_id);

-- name: CanonicalThreadMessageCount :one
SELECT count(*) AS messages FROM messages WHERE conversation_id=sqlc.arg(conversation_id);

-- name: ParkMessageOrdinal :exec
UPDATE messages SET ordinal=sqlc.arg(ordinal)
WHERE conversation_id=sqlc.arg(conversation_id) AND id=sqlc.arg(message_id);

-- name: SearchHit :one
SELECT m.id,m.conversation_id,c.provider,m.role,COALESCE(m.search_projection,m.content) AS content,
m.created_at,c.title,c.project,c.repository,c.created_at AS conversation_created_at,c.updated_at AS conversation_updated_at
FROM messages m JOIN conversations c ON c.id=m.conversation_id WHERE m.storage_id=?1;

-- name: SearchDocument :one
SELECT COALESCE(search_projection,content) AS content FROM messages WHERE id=?1;

-- name: Anchor :one
SELECT conversation_id,ordinal,context_parent FROM messages WHERE id=?1;

-- name: ViewMessages :many
SELECT id,ordinal,role,content,COALESCE(search_projection,content) AS projection,created_at FROM messages
WHERE conversation_id=sqlc.arg(conversation_id) AND ordinal>=sqlc.arg(first_ordinal) AND ordinal<=sqlc.arg(last_ordinal) ORDER BY ordinal;

-- name: ContextMessage :one
SELECT id,ordinal,role,content,COALESCE(search_projection,content) AS projection,created_at,context_parent FROM messages
WHERE conversation_id=sqlc.arg(conversation_id) AND ordinal=sqlc.arg(ordinal);

-- name: FirstChild :one
SELECT ordinal FROM messages WHERE conversation_id=sqlc.arg(conversation_id) AND context_parent=sqlc.arg(parent) ORDER BY ordinal LIMIT 1;
