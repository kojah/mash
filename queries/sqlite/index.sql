-- name: Counts :one
SELECT (SELECT count(*) FROM conversations) AS conversations,
(SELECT count(*) FROM messages) AS messages,
(SELECT count(*) FROM messages WHERE COALESCE(search_projection,content)<>'') AS searchable_messages,
(SELECT count(*) FROM message_embeddings) AS embeddings;

-- name: SearchHit :one
SELECT m.id,m.conversation_id,c.provider,m.role,COALESCE(m.search_projection,m.content) AS content,
m.created_at,c.title,c.project,c.repository,c.created_at AS conversation_created_at,c.updated_at AS conversation_updated_at
FROM messages m JOIN conversations c ON c.id=m.conversation_id WHERE m.storage_id=?1;

-- name: SearchDocument :one
SELECT COALESCE(search_projection,content) AS content FROM messages WHERE id=?1;

-- name: Anchor :one
SELECT conversation_id,ordinal FROM messages WHERE id=?1;

-- name: ViewMessages :many
SELECT id,ordinal,role,content,created_at FROM messages
WHERE conversation_id=sqlc.arg(conversation_id) AND ordinal>=sqlc.arg(first_ordinal) AND ordinal<=sqlc.arg(last_ordinal) ORDER BY ordinal;
