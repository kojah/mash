-- Server queries over one node's silver database (the shared silver schema).

-- name: Head :one
SELECT head FROM shared_head WHERE singleton = 1;

-- name: PutHead :exec
INSERT INTO shared_head(singleton, head) VALUES (1, sqlc.arg(head))
ON CONFLICT(singleton) DO UPDATE SET head = excluded.head;

-- name: Counts :one
SELECT (SELECT count(*) FROM objects WHERE origin = 'provider' AND object_role = 'container') AS conversations,
    (SELECT count(*) FROM objects WHERE origin = 'provider' AND object_role = 'entry') AS messages,
    (SELECT count(*) FROM objects WHERE origin = 'provider' AND object_role = 'entry'
        AND COALESCE(search_projection, content) <> '') AS searchable,
    (SELECT count(*) FROM objects WHERE origin = 'provider' AND object_role = 'entry'
        AND COALESCE(search_projection, content) <> '' AND role = 'user') AS searchable_users;

-- name: Conversation :one
SELECT c.id, c.source AS provider, c.title, c.project, c.repository, c.created_at, c.updated_at,
    e.evidence AS enrichment
FROM objects c LEFT JOIN conversation_enrichment e ON e.conversation_id = c.id
WHERE c.origin = 'provider' AND c.object_role = 'container' AND c.id = sqlc.arg(id);

-- name: NodeConversations :many
SELECT c.id, c.source AS provider, c.title, c.project, c.repository, c.created_at, c.updated_at,
    e.evidence AS enrichment
FROM objects c LEFT JOIN conversation_enrichment e ON e.conversation_id = c.id
WHERE c.origin = 'provider' AND c.object_role = 'container'
ORDER BY c.id;

-- name: ConversationProvider :one
SELECT source AS provider FROM objects
WHERE origin = 'provider' AND object_role = 'container' AND id = sqlc.arg(id);

-- name: Fingerprints :many
SELECT conversation_id AS id, thread_fingerprint FROM native_chats ORDER BY conversation_id;

-- name: InsertConversation :exec
INSERT INTO objects(
    origin, source, id, object_role, kind, conversation_id, title, created_at, updated_at,
    project, repository
) VALUES (
    'provider', sqlc.arg(provider), sqlc.arg(id), 'container', sqlc.arg(kind), sqlc.arg(id),
    sqlc.narg(title), sqlc.narg(created_at), sqlc.narg(updated_at), sqlc.narg(project),
    sqlc.narg(repository)
);

-- name: InsertNativeChat :exec
INSERT INTO native_chats(storage_id, conversation_id, provider, source_path)
SELECT o.storage_id, o.id, o.source, '' FROM objects o
WHERE o.origin = 'provider' AND o.object_role = 'container' AND o.id = sqlc.arg(id);

-- name: InsertEnrichment :exec
INSERT INTO conversation_enrichment(conversation_id, evidence)
VALUES (sqlc.arg(conversation_id), sqlc.arg(evidence));

-- name: SetFingerprint :exec
UPDATE native_chats SET thread_fingerprint = sqlc.arg(fingerprint)
WHERE conversation_id = sqlc.arg(id);

-- name: DeleteConversation :execrows
DELETE FROM objects WHERE origin = 'provider' AND object_role = 'container' AND id = sqlc.arg(id);

-- name: DeleteNodeConversations :exec
DELETE FROM objects WHERE origin = 'provider';

-- name: Message :one
SELECT m.attachments, m.id, m.conversation_id, m.ordinal, m.role, m.content, m.search_projection,
    m.created_at, m.context_parent
FROM objects m WHERE m.origin = 'provider' AND m.object_role = 'entry' AND m.id = sqlc.arg(id);

-- name: MessageAt :one
SELECT m.attachments, m.id, m.conversation_id, m.ordinal, m.role, m.content, m.search_projection,
    m.created_at, m.context_parent
FROM objects m
WHERE m.origin = 'provider' AND m.object_role = 'entry' AND m.conversation_id = sqlc.arg(conversation_id)
  AND m.ordinal = sqlc.arg(ordinal);

-- name: FirstChild :one
SELECT m.attachments, m.id, m.conversation_id, m.ordinal, m.role, m.content, m.search_projection,
    m.created_at, m.context_parent
FROM objects m
WHERE m.origin = 'provider' AND m.object_role = 'entry' AND m.conversation_id = sqlc.arg(conversation_id)
  AND m.context_parent = sqlc.arg(parent)
ORDER BY m.ordinal LIMIT 1;

-- name: MessageWindow :many
SELECT m.attachments, m.id, m.conversation_id, m.ordinal, m.role, m.content, m.search_projection,
    m.created_at, m.context_parent
FROM objects m
WHERE m.origin = 'provider' AND m.object_role = 'entry' AND m.conversation_id = sqlc.arg(conversation_id)
  AND m.ordinal >= sqlc.arg(first_ordinal) AND m.ordinal <= sqlc.arg(last_ordinal)
ORDER BY m.ordinal;

-- name: ThreadMessages :many
SELECT m.attachments, m.id, m.conversation_id, m.ordinal, m.role, m.content, m.search_projection,
    m.created_at, m.context_parent
FROM objects m WHERE m.origin = 'provider' AND m.object_role = 'entry' AND m.conversation_id = sqlc.arg(conversation_id)
ORDER BY m.ordinal;

-- name: NodeMessages :many
SELECT m.attachments, m.id, m.conversation_id, m.ordinal, m.role, m.content, m.search_projection,
    m.created_at, m.context_parent
FROM objects m WHERE m.origin = 'provider' AND m.object_role = 'entry' ORDER BY m.id;

-- name: InsertMessage :exec
-- A native message takes its provider from its chat, and sits under it.
INSERT INTO objects(
    origin, source, id, object_role, kind, parent_id, parent_storage_id, conversation_id,
    attachments, ordinal, role, content, search_projection, created_at, context_parent
)
SELECT 'provider', c.source, sqlc.arg(id), 'entry', c.source || ':message', c.id, c.storage_id, c.id,
    sqlc.arg(attachments), sqlc.arg(ordinal), sqlc.arg(role), sqlc.arg(content),
    sqlc.arg(search_projection), sqlc.narg(created_at), sqlc.narg(context_parent)
FROM objects c
WHERE c.origin = 'provider' AND c.object_role = 'container' AND c.id = sqlc.arg(conversation_id);

-- name: DeleteThreadMessages :execrows
DELETE FROM objects
WHERE origin = 'provider' AND object_role <> 'container'
  AND conversation_id = sqlc.arg(conversation_id);

-- name: DeleteNodeMessages :exec
DELETE FROM objects WHERE origin = 'provider' AND object_role <> 'container';

-- name: MissingParent :one
SELECT m.id FROM objects m
WHERE m.origin = 'provider' AND m.object_role = 'entry' AND m.conversation_id = sqlc.arg(conversation_id) AND m.context_parent >= 0
  AND NOT EXISTS(SELECT 1 FROM objects p
      WHERE p.origin = 'provider' AND p.object_role = 'entry'
        AND p.conversation_id = m.conversation_id AND p.ordinal = m.context_parent)
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
