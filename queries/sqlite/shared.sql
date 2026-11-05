-- Server queries over one node's silver database (the shared silver schema).

-- name: Head :one
SELECT head FROM shared_head WHERE singleton = 1;

-- name: PutHead :exec
INSERT INTO shared_head(singleton, head) VALUES (1, sqlc.arg(head))
ON CONFLICT(singleton) DO UPDATE SET head = excluded.head;

-- name: Counts :one
SELECT (SELECT count(*) FROM sessions) AS conversations,
(SELECT count(*) FROM messages) AS messages,
(SELECT count(*) FROM messages INDEXED BY messages_searchable_role WHERE COALESCE(search_projection,content)<>'') AS searchable,
(SELECT count(*) FROM messages INDEXED BY messages_searchable_role WHERE COALESCE(search_projection,content)<>'' AND role='user') AS searchable_users;

-- name: Conversation :one
SELECT c.id, c.provider AS provider, c.title, c.project, c.repository, c.created_at, c.updated_at,
    e.evidence AS enrichment
FROM sessions c LEFT JOIN conversation_enrichment e ON e.conversation_id = c.id
WHERE c.id = sqlc.arg(id);

-- name: NodeConversations :many
SELECT c.id, c.provider AS provider, c.title, c.project, c.repository, c.created_at, c.updated_at,
    e.evidence AS enrichment
FROM sessions c LEFT JOIN conversation_enrichment e ON e.conversation_id = c.id
ORDER BY c.id;

-- name: ConversationProvider :one
SELECT provider FROM sessions
WHERE id = sqlc.arg(id);

-- name: Fingerprints :many
SELECT id,thread_fingerprint FROM sessions ORDER BY id;

-- name: InsertConversation :exec
INSERT INTO sessions(id,provider,title,created_at,updated_at,project,repository)
VALUES(sqlc.arg(id),sqlc.arg(provider),sqlc.narg(title),sqlc.narg(created_at),sqlc.narg(updated_at),sqlc.narg(project),sqlc.narg(repository));

-- name: InsertEnrichment :exec
INSERT INTO conversation_enrichment(conversation_id, evidence)
VALUES (sqlc.arg(conversation_id), sqlc.arg(evidence));

-- name: SetFingerprint :exec
UPDATE sessions SET thread_fingerprint=sqlc.arg(fingerprint) WHERE id=sqlc.arg(id);

-- name: DeleteConversation :execrows
DELETE FROM sessions WHERE id = sqlc.arg(id);

-- name: DeleteNodeConversations :exec
DELETE FROM sessions;

-- name: Message :one
SELECT m.attachments, m.id, m.conversation_id, m.ordinal, m.role, m.content, m.search_projection,
    m.created_at, m.context_parent, m.metadata
FROM messages m WHERE m.id = sqlc.arg(id);

-- name: MessageAt :one
SELECT m.attachments, m.id, m.conversation_id, m.ordinal, m.role, m.content, m.search_projection,
    m.created_at, m.context_parent, m.metadata
FROM messages m
WHERE m.conversation_id = sqlc.arg(conversation_id)
  AND m.ordinal = sqlc.arg(ordinal);

-- name: FirstChild :one
SELECT m.attachments, m.id, m.conversation_id, m.ordinal, m.role, m.content, m.search_projection,
    m.created_at, m.context_parent, m.metadata
FROM messages m
WHERE m.conversation_id = sqlc.arg(conversation_id)
  AND m.context_parent = sqlc.arg(parent)
ORDER BY m.ordinal LIMIT 1;

-- name: MessageWindow :many
SELECT m.attachments, m.id, m.conversation_id, m.ordinal, m.role, m.content, m.search_projection,
    m.created_at, m.context_parent, m.metadata
FROM messages m
WHERE m.conversation_id = sqlc.arg(conversation_id)
  AND m.ordinal >= sqlc.arg(first_ordinal) AND m.ordinal <= sqlc.arg(last_ordinal)
ORDER BY m.ordinal;

-- name: ThreadMessages :many
SELECT m.attachments, m.id, m.conversation_id, m.ordinal, m.role, m.content, m.search_projection,
    m.created_at, m.context_parent, m.metadata
FROM messages m WHERE m.conversation_id = sqlc.arg(conversation_id)
ORDER BY m.ordinal;

-- name: NodeMessages :many
SELECT m.attachments, m.id, m.conversation_id, m.ordinal, m.role, m.content, m.search_projection,
    m.created_at, m.context_parent, m.metadata
FROM messages m ORDER BY m.id;

-- name: InsertMessage :exec
INSERT INTO messages(id,conversation_id,attachments,ordinal,role,content_packed,search_projection,created_at,context_parent,metadata)
VALUES(sqlc.arg(id),sqlc.arg(conversation_id),sqlc.arg(attachments),sqlc.arg(ordinal),sqlc.arg(role),silver_pack(CAST(sqlc.arg(content) AS TEXT)),
sqlc.arg(search_projection),sqlc.narg(created_at),sqlc.narg(context_parent),sqlc.narg(metadata));

-- name: DeleteThreadMessages :execrows
DELETE FROM messages
WHERE conversation_id = sqlc.arg(conversation_id);

-- name: DeleteNodeMessages :exec
DELETE FROM messages;

-- name: MissingParent :one
SELECT m.id FROM messages m
WHERE m.conversation_id = sqlc.arg(conversation_id) AND m.context_parent <> ''
  AND NOT EXISTS(SELECT 1 FROM messages p
      WHERE p.conversation_id = m.conversation_id AND p.id = m.context_parent AND p.ordinal < m.ordinal)
LIMIT 1;

-- name: NodeTombstones :many
SELECT provider, conversation_id FROM tombstones ORDER BY provider, conversation_id;

-- name: Tombstoned :one
SELECT conversation_id FROM tombstones
WHERE provider = sqlc.arg(provider) AND conversation_id = sqlc.arg(conversation_id);

-- name: UpsertTombstone :exec
INSERT INTO tombstones(provider, conversation_id, forgotten_at)
VALUES (sqlc.arg(provider), sqlc.arg(conversation_id), sqlc.arg(forgotten_at))
ON CONFLICT(provider, conversation_id) DO NOTHING;
