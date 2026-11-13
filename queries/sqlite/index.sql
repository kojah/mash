-- name: ListChats :many
-- Activity filters apply to recorded thread updated_at, not individual messages.
-- Null sort keys are always last; identity breaks ties in either direction.
WITH scope AS (SELECT
    CAST(sqlc.arg(all_providers) AS INTEGER) AS all_providers,
    CAST(sqlc.arg(providers_json) AS TEXT) AS providers_json,
    CAST(sqlc.narg(title_contains) AS TEXT) AS title_contains,
    CAST(sqlc.narg(after_ms) AS INTEGER) AS after_ms,
    CAST(sqlc.narg(before_ms) AS INTEGER) AS before_ms,
    CAST(sqlc.arg(sort_key) AS TEXT) AS sort_key,
    CAST(sqlc.arg(descending) AS INTEGER) AS descending)
SELECT c.id,c.provider AS provider,c.title,c.project,c.repository,c.created_at,c.updated_at,e.evidence AS enrichment,
    f.id AS first_message_id
FROM chats c CROSS JOIN scope s LEFT JOIN chat_enrichment e ON e.chat_id=c.id
LEFT JOIN messages f ON f.chat_id=c.id
    AND f.ordinal=(SELECT MIN(m.ordinal) FROM messages m WHERE m.chat_id=c.id)
WHERE (s.all_providers = 1 OR c.provider IN (
    SELECT CAST(value AS TEXT) FROM json_each(s.providers_json)
))
AND (s.title_contains IS NULL OR instr(lower(c.title),lower(s.title_contains)) > 0)
AND (s.after_ms IS NULL OR c.updated_at >= s.after_ms)
AND (s.before_ms IS NULL OR c.updated_at < s.before_ms)
ORDER BY
CASE s.sort_key
    WHEN 'title' THEN c.title IS NULL
    WHEN 'created_at' THEN c.created_at IS NULL
    ELSE c.updated_at IS NULL END ASC,
CASE WHEN s.sort_key = 'title' AND s.descending = 0 THEN c.title END COLLATE BINARY ASC,
CASE WHEN s.sort_key = 'title' AND s.descending = 1 THEN c.title END COLLATE BINARY DESC,
CASE WHEN s.sort_key = 'created_at' AND s.descending = 0 THEN c.created_at END ASC,
CASE WHEN s.sort_key = 'created_at' AND s.descending = 1 THEN c.created_at END DESC,
CASE WHEN s.sort_key = 'updated_at' AND s.descending = 0 THEN c.updated_at END ASC,
CASE WHEN s.sort_key = 'updated_at' AND s.descending = 1 THEN c.updated_at END DESC,
c.provider COLLATE BINARY ASC,c.id COLLATE BINARY ASC
LIMIT sqlc.arg(page_size) OFFSET sqlc.arg(page_offset);

-- name: ChatMessageState :many
SELECT id,fingerprint,ordinal FROM messages WHERE chat_id=sqlc.arg(chat_id);

-- name: ClaudeSourceChat :one
SELECT chat_id FROM claude_sources WHERE source_key=sqlc.arg(source_key);

-- name: ClaudeSourceMetadata :many
SELECT source_key,source_path,title,project,repository FROM claude_sources
WHERE chat_id=sqlc.arg(chat_id) ORDER BY source_key;

-- name: ClaudeSourceReferences :many
SELECT source_key,source_path,chat_id FROM claude_sources ORDER BY source_key;

-- name: ClaudeContributionMessages :many
SELECT m.source_key,m.message_id,m.parent_id,m.role,m.content,m.projection,m.created_at,m.attachments,m.metadata
FROM claude_source_messages m JOIN claude_sources s USING(source_key)
WHERE s.chat_id=sqlc.arg(chat_id) ORDER BY m.message_id,m.source_key;

-- name: ClaudeSourceScope :many
SELECT s.source_key,s.source_path,c.modified_ns FROM claude_sources s
LEFT JOIN source_checkpoints c ON c.provider='claude-code' AND c.source_key=s.source_key;

-- name: UpsertClaudeSource :exec
INSERT INTO claude_sources(source_key,source_path,chat_id,title,project,repository)
VALUES(sqlc.arg(source_key),sqlc.arg(source_path),sqlc.arg(chat_id),sqlc.narg(title),sqlc.narg(project),sqlc.narg(repository))
ON CONFLICT(source_key) DO UPDATE SET source_path=excluded.source_path,
chat_id=excluded.chat_id,title=excluded.title,project=excluded.project,repository=excluded.repository
WHERE source_path IS NOT excluded.source_path OR chat_id IS NOT excluded.chat_id
   OR title IS NOT excluded.title OR project IS NOT excluded.project OR repository IS NOT excluded.repository;

-- name: ClaudeSourceMessageIds :many
SELECT message_id FROM claude_source_messages WHERE source_key=sqlc.arg(source_key);

-- name: DeleteClaudeSourceMessage :exec
DELETE FROM claude_source_messages WHERE source_key=sqlc.arg(source_key) AND message_id=sqlc.arg(message_id);

-- name: UpsertClaudeSourceMessage :exec
-- An unchanged row is not rewritten, so appending to a transcript writes only its new rows.
INSERT INTO claude_source_messages(source_key,message_id,parent_id,role,content_packed,projection,created_at,attachments,metadata)
VALUES(sqlc.arg(source_key),sqlc.arg(message_id),sqlc.narg(parent_id),sqlc.arg(role),silver_pack(CAST(sqlc.arg(content) AS TEXT)),sqlc.narg(projection),sqlc.narg(created_at),sqlc.arg(attachments),sqlc.narg(metadata))
ON CONFLICT(source_key,message_id) DO UPDATE SET parent_id=excluded.parent_id,role=excluded.role,
content_packed=excluded.content_packed,projection=excluded.projection,created_at=excluded.created_at,attachments=excluded.attachments,
metadata=excluded.metadata
WHERE parent_id IS NOT excluded.parent_id OR role IS NOT excluded.role
OR (content_packed IS NOT excluded.content_packed AND content IS NOT excluded.content)
OR projection IS NOT excluded.projection OR created_at IS NOT excluded.created_at OR attachments IS NOT excluded.attachments
OR metadata IS NOT excluded.metadata;

-- name: DeleteClaudeSource :exec
DELETE FROM claude_sources WHERE source_key=sqlc.arg(source_key);

-- name: DeleteClaudeChatSources :exec
DELETE FROM claude_sources WHERE chat_id=sqlc.arg(chat_id);

-- name: ClaudeTombstone :one
SELECT chat_id FROM tombstones WHERE provider='claude-code' AND chat_id=sqlc.arg(chat_id);

-- name: CanonicalChatMessageCount :one
SELECT count(*) AS messages FROM messages WHERE chat_id=sqlc.arg(chat_id);

-- name: ParkMessageOrdinal :exec
UPDATE messages SET ordinal=sqlc.arg(ordinal)
WHERE chat_id=sqlc.arg(chat_id) AND id=sqlc.arg(message_id);

-- name: ChatFirstMessage :one
-- A chat ID passed where a message ID belongs; NULL when the chat has no messages.
SELECT f.id AS first_message_id
FROM chats c LEFT JOIN messages f ON f.chat_id=c.id
    AND f.ordinal=(SELECT MIN(m.ordinal) FROM messages m WHERE m.chat_id=c.id)
WHERE c.id=?1;

-- name: Anchor :one
SELECT id,chat_id,ordinal,context_parent FROM messages WHERE id=?1;

-- name: MessageAttachments :one
SELECT attachments FROM messages WHERE id=?1;

-- name: AttachmentCandidates :many
SELECT id,attachments FROM messages WHERE attachments<>'[]' ORDER BY id;

-- name: RetainedAttachments :many
-- Unordered: a full scan reads table pages in storage order, whereas ORDER BY id
-- looks every row up through the id index, several times slower when cold.
SELECT attachments FROM messages WHERE attachments<>'[]';

-- name: AttachmentPolicy :one
SELECT fingerprint FROM attachment_policy WHERE singleton=1;

-- name: SetAttachmentPolicy :exec
INSERT INTO attachment_policy(singleton,fingerprint) VALUES(1,sqlc.arg(fingerprint)) ON CONFLICT(singleton) DO UPDATE SET fingerprint=excluded.fingerprint;

-- name: ViewMessages :many
SELECT id,ordinal,role,content,COALESCE(search_projection,content) AS projection,created_at,metadata FROM messages
WHERE chat_id=sqlc.arg(chat_id) AND ordinal>=sqlc.arg(first_ordinal) AND ordinal<=sqlc.arg(last_ordinal) ORDER BY ordinal;

-- name: ContextMessage :one
SELECT id,ordinal,role,content,COALESCE(search_projection,content) AS projection,created_at,context_parent,metadata FROM messages
WHERE chat_id=sqlc.arg(chat_id) AND id=sqlc.arg(id);

-- name: FirstChild :one
SELECT id,ordinal FROM messages WHERE chat_id=sqlc.arg(chat_id) AND context_parent=sqlc.arg(parent) ORDER BY ordinal LIMIT 1;

-- name: ChatWindow :one
-- A view's place in its chat: all messages, and those before and after its ordinal range.
SELECT COUNT(*) AS message_count,
    CAST(COALESCE(SUM(ordinal<sqlc.arg(first_ordinal)),0) AS INTEGER) AS omitted_before,
    CAST(COALESCE(SUM(ordinal>sqlc.arg(last_ordinal)),0) AS INTEGER) AS omitted_after
FROM messages WHERE chat_id=sqlc.arg(chat_id);

-- name: SilverCertificate :one
SELECT s.revision, s.interpretation_version,
    CAST(EXISTS(SELECT 1 FROM silver_scans WHERE state=0) AS INTEGER) AS pending,
    CAST(s.interpretation_version<>sqlc.arg(current_version)
        OR EXISTS(SELECT 1 FROM silver_scans WHERE interpretation_version<>sqlc.arg(current_version))
        OR EXISTS(SELECT 1 FROM source_checkpoints WHERE interpretation_version<>sqlc.arg(current_version))
        OR EXISTS(SELECT 1 FROM chats c WHERE NOT EXISTS(
            SELECT 1 FROM silver_scans p WHERE p.provider=c.provider
              AND p.interpretation_version=sqlc.arg(current_version))) AS INTEGER) AS unverified,
    CAST(EXISTS(SELECT 1 FROM silver_scans WHERE state=2) AS INTEGER) AS incomplete,
    (SELECT count(*) FROM silver_rejections) AS rejected_sources
FROM silver_state s WHERE s.singleton=1;

-- name: SilverRejections :many
SELECT source_key,source_path,modified_ns FROM silver_rejections
WHERE provider=sqlc.arg(provider) ORDER BY source_key;

-- name: SourceEnrichment :many
SELECT e.evidence,c.title,c.project,c.repository,c.created_at,c.updated_at
FROM chats c LEFT JOIN chat_enrichment e ON e.chat_id=c.id
WHERE c.provider=sqlc.arg(provider) AND c.source_key=sqlc.arg(source_key);

-- name: ChatEnrichment :one
SELECT evidence FROM chat_enrichment WHERE chat_id=sqlc.arg(chat_id);

-- name: ChatSourceFingerprint :one
SELECT source_fingerprint FROM chats WHERE id=sqlc.arg(id);

-- name: UpsertChat :exec
INSERT INTO chats(id,provider,title,created_at,updated_at,project,repository)
VALUES(sqlc.arg(id),sqlc.arg(provider),sqlc.narg(title),sqlc.narg(created_at),sqlc.narg(updated_at),sqlc.narg(project),sqlc.narg(repository))
ON CONFLICT(id) DO UPDATE SET title=excluded.title,created_at=excluded.created_at,
updated_at=excluded.updated_at,project=excluded.project,repository=excluded.repository;

-- name: SetChatSource :exec
UPDATE chats SET source_path=sqlc.arg(source_path),source_fingerprint=sqlc.arg(source_fingerprint),
source_key=sqlc.narg(source_key),source_session=sqlc.narg(source_session)
WHERE id=sqlc.arg(id);

-- name: DisplacedChatMessages :one
SELECT (SELECT count(*) FROM messages m WHERE m.chat_id=n.id) AS messages
FROM chats n WHERE n.provider=sqlc.arg(provider) AND n.source_key=sqlc.arg(source_key) AND n.id<>sqlc.arg(id);

-- name: UpsertMessage :exec
INSERT INTO messages(id,chat_id,ordinal,role,content_packed,search_projection,created_at,context_parent,attachments,metadata)
VALUES(sqlc.arg(id),sqlc.arg(chat_id),sqlc.arg(ordinal),sqlc.arg(role),silver_pack(CAST(sqlc.arg(content) AS TEXT)),
sqlc.narg(search_projection),sqlc.narg(created_at),sqlc.narg(context_parent),sqlc.arg(attachments),sqlc.narg(metadata))
ON CONFLICT(id) DO UPDATE SET chat_id=excluded.chat_id,ordinal=excluded.ordinal,role=excluded.role,
content_packed=excluded.content_packed,search_projection=excluded.search_projection,created_at=excluded.created_at,
context_parent=excluded.context_parent,attachments=excluded.attachments,metadata=excluded.metadata;

-- name: SetMessageFingerprint :exec
UPDATE messages SET fingerprint=sqlc.arg(fingerprint) WHERE id=sqlc.arg(id);

-- name: DeleteChat :execrows
-- Its turns and messages are deleted through their chat foreign keys.
DELETE FROM chats WHERE id=sqlc.arg(id);

-- name: DeleteMessage :exec
DELETE FROM messages WHERE id=sqlc.arg(id);

-- name: ChatsForPurge :many
SELECT n.id,n.source_path,n.source_key,k.modified_ns FROM chats n
LEFT JOIN source_checkpoints k ON k.provider=n.provider AND k.source_key=n.source_key
WHERE n.provider=sqlc.arg(provider);

-- name: ChatBySource :one
SELECT id FROM chats WHERE provider=sqlc.arg(provider) AND source_key=sqlc.arg(source_key);

-- name: ChatProvider :one
SELECT provider FROM chats WHERE id=sqlc.arg(id);

-- name: TurnSources :many
SELECT id,ordinal,role,COALESCE(search_projection,content) AS projection,context_parent,created_at
FROM messages WHERE chat_id=sqlc.arg(chat_id)
ORDER BY ordinal;

-- name: ChatTurns :many
SELECT id FROM turns WHERE chat_id=sqlc.arg(chat_id);

-- name: UpsertTurn :exec
-- Derived groups have no canonical children. Unchanged turns are left alone.
INSERT INTO turns(id,chat_id,revision,boundary,closed,created_at,updated_at)
VALUES(sqlc.arg(id),sqlc.arg(chat_id),sqlc.arg(revision),sqlc.arg(boundary),
       sqlc.arg(closed),sqlc.narg(created_at),sqlc.narg(updated_at))
ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,boundary=excluded.boundary,
closed=excluded.closed,created_at=excluded.created_at,updated_at=excluded.updated_at
WHERE turns.revision IS NOT excluded.revision OR turns.boundary IS NOT excluded.boundary
   OR turns.closed IS NOT excluded.closed OR turns.created_at IS NOT excluded.created_at
   OR turns.updated_at IS NOT excluded.updated_at;

-- name: SetMessageTurn :exec
UPDATE messages SET turn_id=sqlc.narg(turn_id)
WHERE chat_id=sqlc.arg(chat_id)
  AND id=sqlc.arg(id) AND turn_id IS NOT sqlc.narg(turn_id);

-- name: DeleteTurn :exec
DELETE FROM turns WHERE id=sqlc.arg(id);

-- name: ChatIds :many
SELECT id FROM chats ORDER BY id;
