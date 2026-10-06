-- name: Head :one
SELECT head FROM nodes WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id);

-- name: Heads :many
SELECT node_id, head FROM nodes WHERE group_id = sqlc.arg(group_id) ORDER BY node_id;

-- name: PutHead :exec
INSERT INTO nodes(group_id, node_id, head) VALUES (sqlc.arg(group_id), sqlc.arg(node_id), sqlc.arg(head))
ON CONFLICT(group_id, node_id) DO UPDATE SET head = excluded.head;

-- name: Counts :one
SELECT (SELECT count(*) FROM conversations c WHERE c.group_id = sqlc.arg(group_id) AND c.node_id = sqlc.arg(node_id)) AS conversations,
    count(*) AS messages,
    CAST(coalesce(sum(searchable), 0) AS INTEGER) AS searchable,
    CAST(coalesce(sum(searchable * user), 0) AS INTEGER) AS searchable_users
FROM messages WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id);

-- name: Conversation :one
SELECT record FROM conversations
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) AND id = sqlc.arg(id);

-- name: ConversationProvider :one
SELECT provider FROM conversations
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) AND id = sqlc.arg(id);

-- name: NodeConversations :many
SELECT id, record FROM conversations
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) ORDER BY id;

-- name: Fingerprints :many
SELECT id, fingerprint FROM conversations
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) ORDER BY id;

-- name: Providers :many
SELECT DISTINCT provider FROM conversations
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id);

-- name: InsertConversation :exec
INSERT INTO conversations(group_id, node_id, id, provider, record, fingerprint)
VALUES (sqlc.arg(group_id), sqlc.arg(node_id), sqlc.arg(id), sqlc.arg(provider), sqlc.arg(record), '');

-- name: SetFingerprint :exec
UPDATE conversations SET fingerprint = sqlc.arg(fingerprint)
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) AND id = sqlc.arg(id);

-- name: DeleteConversation :exec
DELETE FROM conversations
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) AND id = sqlc.arg(id);

-- name: DeleteNodeConversations :exec
DELETE FROM conversations WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id);

-- name: Message :one
SELECT ordinal, parent, record FROM messages
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) AND id = sqlc.arg(id);

-- name: MessageAt :one
SELECT ordinal, parent, record FROM messages
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id)
  AND conversation_id = sqlc.arg(conversation_id) AND ordinal = sqlc.arg(ordinal);

-- name: FirstChild :one
SELECT ordinal, parent, record FROM messages
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id)
  AND conversation_id = sqlc.arg(conversation_id) AND parent = sqlc.arg(parent)
ORDER BY ordinal LIMIT 1;

-- name: MessageWindow :many
SELECT record FROM messages
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id)
  AND conversation_id = sqlc.arg(conversation_id)
  AND ordinal >= sqlc.arg(first_ordinal) AND ordinal <= sqlc.arg(last_ordinal)
ORDER BY ordinal;

-- name: ThreadMessages :many
SELECT record FROM messages
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id)
  AND conversation_id = sqlc.arg(conversation_id)
ORDER BY ordinal;

-- name: NodeMessages :many
SELECT id, record FROM messages
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) ORDER BY id;

-- name: InsertMessage :exec
INSERT INTO messages(group_id, node_id, id, conversation_id, ordinal, parent, searchable, user, record)
VALUES (sqlc.arg(group_id), sqlc.arg(node_id), sqlc.arg(id), sqlc.arg(conversation_id),
        sqlc.arg(ordinal), sqlc.narg(parent), sqlc.arg(searchable), sqlc.arg(user), sqlc.arg(record));

-- name: DeleteThreadMessages :exec
DELETE FROM messages
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id)
  AND conversation_id = sqlc.arg(conversation_id);

-- name: DeleteNodeMessages :exec
DELETE FROM messages WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id);

-- name: MissingParent :one
SELECT m.id FROM messages m
WHERE m.group_id = sqlc.arg(group_id) AND m.node_id = sqlc.arg(node_id)
  AND m.conversation_id = sqlc.arg(conversation_id) AND m.parent IS NOT NULL
  AND NOT EXISTS(SELECT 1 FROM messages p
      WHERE p.group_id = m.group_id AND p.node_id = m.node_id
        AND p.conversation_id = m.conversation_id AND p.ordinal = m.parent)
LIMIT 1;

-- name: NodeSources :many
SELECT key, record FROM sources
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) ORDER BY key;

-- name: ProviderSources :many
SELECT key, record FROM sources
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) AND provider = sqlc.arg(provider);

-- name: UpsertSource :exec
INSERT INTO sources(group_id, node_id, key, provider, conversation_id, interpretation_version, record)
VALUES (sqlc.arg(group_id), sqlc.arg(node_id), sqlc.arg(key), sqlc.arg(provider),
        sqlc.narg(conversation_id), sqlc.arg(interpretation_version), sqlc.arg(record))
ON CONFLICT(group_id, node_id, key) DO UPDATE SET
    provider = excluded.provider, conversation_id = excluded.conversation_id,
    interpretation_version = excluded.interpretation_version, record = excluded.record;

-- name: DeleteSource :exec
DELETE FROM sources WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) AND key = sqlc.arg(key);

-- name: DeleteNodeSources :exec
DELETE FROM sources WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id);

-- name: SourceReference :one
SELECT key FROM sources
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id)
  AND conversation_id = sqlc.arg(conversation_id)
LIMIT 1;

-- name: CoverageGaps :one
SELECT
    CAST(EXISTS(SELECT 1 FROM sources s WHERE s.group_id = sqlc.arg(group_id) AND s.node_id = sqlc.arg(node_id)
        AND s.provider = sqlc.arg(provider) AND s.interpretation_version <> sqlc.arg(interpretation_version)) AS INTEGER) AS stale,
    CAST(EXISTS(SELECT 1 FROM conversations c WHERE c.group_id = sqlc.arg(group_id) AND c.node_id = sqlc.arg(node_id)
        AND c.provider = sqlc.arg(provider) AND NOT EXISTS(SELECT 1 FROM sources r
            WHERE r.group_id = c.group_id AND r.node_id = c.node_id AND r.provider = c.provider
              AND r.conversation_id = c.id)) AS INTEGER) AS unmapped;

-- name: NodeTombstones :many
SELECT key, record FROM tombstones
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) ORDER BY key;

-- name: Tombstoned :one
SELECT key FROM tombstones
WHERE group_id = sqlc.arg(group_id) AND node_id = sqlc.arg(node_id) AND key = sqlc.arg(key);

-- name: UpsertTombstone :exec
INSERT INTO tombstones(group_id, node_id, key, record)
VALUES (sqlc.arg(group_id), sqlc.arg(node_id), sqlc.arg(key), sqlc.arg(record))
ON CONFLICT(group_id, node_id, key) DO UPDATE SET record = excluded.record;
