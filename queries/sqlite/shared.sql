-- Server queries over one node's silver database (the shared silver schema).

-- name: Head :one
SELECT head FROM shared_head WHERE singleton = 1;

-- name: PutHead :exec
INSERT INTO shared_head(singleton, head) VALUES (1, sqlc.arg(head))
ON CONFLICT(singleton) DO UPDATE SET head = excluded.head;

-- name: Counts :one
SELECT (SELECT count(*) FROM conversations) AS conversations,
    (SELECT count(*) FROM messages) AS messages,
    (SELECT count(*) FROM messages WHERE COALESCE(search_projection, content) <> '') AS searchable,
    (SELECT count(*) FROM messages
        WHERE COALESCE(search_projection, content) <> '' AND role = 'user') AS searchable_users;

-- name: Conversation :one
SELECT c.id, c.provider, c.title, c.project, c.repository, c.created_at, c.updated_at,
    e.evidence AS enrichment
FROM conversations c LEFT JOIN conversation_enrichment e ON e.conversation_id = c.id
WHERE c.id = sqlc.arg(id);

-- name: NodeConversations :many
SELECT c.id, c.provider, c.title, c.project, c.repository, c.created_at, c.updated_at,
    e.evidence AS enrichment
FROM conversations c LEFT JOIN conversation_enrichment e ON e.conversation_id = c.id
ORDER BY c.id;

-- name: ConversationProvider :one
SELECT provider FROM conversations WHERE id = sqlc.arg(id);

-- name: Fingerprints :many
SELECT id, thread_fingerprint FROM conversations ORDER BY id;

-- name: InsertConversation :exec
INSERT INTO conversations(
    id, provider, source_path, title, created_at, updated_at, project, repository
) VALUES (
    sqlc.arg(id), sqlc.arg(provider), '', sqlc.narg(title), sqlc.narg(created_at),
    sqlc.narg(updated_at), sqlc.narg(project), sqlc.narg(repository)
);

-- name: InsertEnrichment :exec
INSERT INTO conversation_enrichment(conversation_id, evidence)
VALUES (sqlc.arg(conversation_id), sqlc.arg(evidence));

-- name: SetFingerprint :exec
UPDATE conversations SET thread_fingerprint = sqlc.arg(fingerprint) WHERE id = sqlc.arg(id);

-- name: DeleteConversation :execrows
DELETE FROM conversations WHERE id = sqlc.arg(id);

-- name: DeleteNodeConversations :exec
DELETE FROM conversations;

-- name: Message :one
SELECT attachments, id, conversation_id, ordinal, role, content, search_projection,
    created_at, context_parent
FROM messages WHERE id = sqlc.arg(id);

-- name: MessageAt :one
SELECT attachments, id, conversation_id, ordinal, role, content, search_projection,
    created_at, context_parent
FROM messages
WHERE conversation_id = sqlc.arg(conversation_id) AND ordinal = sqlc.arg(ordinal);

-- name: FirstChild :one
SELECT attachments, id, conversation_id, ordinal, role, content, search_projection,
    created_at, context_parent
FROM messages
WHERE conversation_id = sqlc.arg(conversation_id) AND context_parent = sqlc.arg(parent)
ORDER BY ordinal LIMIT 1;

-- name: MessageWindow :many
SELECT attachments, id, conversation_id, ordinal, role, content, search_projection,
    created_at, context_parent
FROM messages
WHERE conversation_id = sqlc.arg(conversation_id)
  AND ordinal >= sqlc.arg(first_ordinal) AND ordinal <= sqlc.arg(last_ordinal)
ORDER BY ordinal;

-- name: ThreadMessages :many
SELECT attachments, id, conversation_id, ordinal, role, content, search_projection,
    created_at, context_parent
FROM messages WHERE conversation_id = sqlc.arg(conversation_id)
ORDER BY ordinal;

-- name: NodeMessages :many
SELECT attachments, id, conversation_id, ordinal, role, content, search_projection,
    created_at, context_parent
FROM messages ORDER BY id;

-- name: InsertMessage :exec
INSERT INTO messages(
    attachments, id, conversation_id, ordinal, role, content, search_projection,
    created_at, context_parent
) VALUES (
    sqlc.arg(attachments), sqlc.arg(id), sqlc.arg(conversation_id), sqlc.arg(ordinal),
    sqlc.arg(role), sqlc.arg(content), sqlc.arg(search_projection), sqlc.narg(created_at),
    sqlc.narg(context_parent)
);

-- name: DeleteThreadMessages :execrows
DELETE FROM messages WHERE conversation_id = sqlc.arg(conversation_id);

-- name: DeleteNodeMessages :exec
DELETE FROM messages;

-- name: MissingParent :one
SELECT m.id FROM messages m
WHERE m.conversation_id = sqlc.arg(conversation_id) AND m.context_parent >= 0
  AND NOT EXISTS(SELECT 1 FROM messages p
      WHERE p.conversation_id = m.conversation_id AND p.ordinal = m.context_parent)
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
