-- name: Counts :one
SELECT (SELECT count(*) FROM conversations) AS conversations,
(SELECT count(*) FROM messages) AS messages,
(SELECT count(*) FROM messages WHERE COALESCE(search_projection,content)<>'') AS searchable_messages,
(SELECT count(*) FROM message_embeddings) AS embeddings;

-- name: ListThreads :many
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
SELECT c.id,c.provider,c.title,c.project,c.repository,c.created_at,c.updated_at,e.evidence AS enrichment
FROM conversations c CROSS JOIN scope s LEFT JOIN conversation_enrichment e ON e.conversation_id=c.id
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
SELECT m.source_key,m.message_id,m.parent_id,m.role,m.content,m.projection,m.created_at,m.attachments
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
INSERT INTO claude_source_messages(source_key,message_id,parent_id,role,content,projection,created_at,attachments)
VALUES(sqlc.arg(source_key),sqlc.arg(message_id),sqlc.narg(parent_id),sqlc.arg(role),sqlc.arg(content),sqlc.narg(projection),sqlc.narg(created_at),sqlc.arg(attachments));

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
m.created_at,c.title,c.project,c.repository,c.created_at AS conversation_created_at,c.updated_at AS conversation_updated_at,e.evidence AS enrichment
FROM messages m JOIN conversations c ON c.id=m.conversation_id
LEFT JOIN conversation_enrichment e ON e.conversation_id=c.id WHERE m.storage_id=?1;

-- name: SearchDocument :one
SELECT COALESCE(search_projection,content) AS content FROM messages WHERE id=?1;

-- name: Anchor :one
SELECT conversation_id,ordinal,context_parent FROM messages WHERE id=?1;

-- name: MessageAttachments :one
SELECT attachments FROM messages WHERE id=?1;

-- name: AttachmentCandidates :many
SELECT id,attachments FROM messages WHERE attachments<>'[]' ORDER BY id;

-- name: AttachmentPolicy :one
SELECT fingerprint FROM attachment_policy WHERE singleton=1;

-- name: SetAttachmentPolicy :exec
INSERT INTO attachment_policy(singleton,fingerprint) VALUES(1,sqlc.arg(fingerprint)) ON CONFLICT(singleton) DO UPDATE SET fingerprint=excluded.fingerprint;

-- name: ViewMessages :many
SELECT id,ordinal,role,content,COALESCE(search_projection,content) AS projection,created_at FROM messages
WHERE conversation_id=sqlc.arg(conversation_id) AND ordinal>=sqlc.arg(first_ordinal) AND ordinal<=sqlc.arg(last_ordinal) ORDER BY ordinal;

-- name: ContextMessage :one
SELECT id,ordinal,role,content,COALESCE(search_projection,content) AS projection,created_at,context_parent FROM messages
WHERE conversation_id=sqlc.arg(conversation_id) AND ordinal=sqlc.arg(ordinal);

-- name: FirstChild :one
SELECT ordinal FROM messages WHERE conversation_id=sqlc.arg(conversation_id) AND context_parent=sqlc.arg(parent) ORDER BY ordinal LIMIT 1;

-- name: SilverCertificate :one
SELECT s.revision, s.interpretation_version,
    CAST(EXISTS(SELECT 1 FROM silver_scans WHERE state=0) AS INTEGER) AS pending,
    CAST(s.interpretation_version<>sqlc.arg(current_version)
        OR EXISTS(SELECT 1 FROM silver_scans WHERE interpretation_version<>sqlc.arg(current_version))
        OR EXISTS(SELECT 1 FROM source_checkpoints WHERE interpretation_version<>sqlc.arg(current_version))
        OR EXISTS(SELECT 1 FROM conversations c WHERE NOT EXISTS(
            SELECT 1 FROM silver_scans p WHERE p.provider=c.provider
              AND p.interpretation_version=sqlc.arg(current_version))) AS INTEGER) AS unverified,
    CAST(EXISTS(SELECT 1 FROM silver_scans WHERE state=2) AS INTEGER) AS incomplete,
    (SELECT count(*) FROM silver_rejections) AS rejected_sources
FROM silver_state s WHERE s.singleton=1;

-- name: SilverRejections :many
SELECT source_key,source_path,modified_ns FROM silver_rejections
WHERE provider=sqlc.arg(provider) ORDER BY source_key;

-- name: GoldManifest :one
SELECT manifest FROM gold_manifest WHERE singleton=1;

-- name: LexicalManifest :one
SELECT manifest FROM lexical_manifest WHERE singleton=1;

-- name: SourceEnrichment :many
SELECT e.evidence,c.title,c.project,c.repository,c.created_at,c.updated_at FROM conversations AS c
LEFT JOIN conversation_enrichment AS e ON e.conversation_id=c.id
WHERE c.provider=sqlc.arg(provider) AND c.source_key=sqlc.arg(source_key);

-- name: ConversationEnrichment :one
SELECT evidence FROM conversation_enrichment WHERE conversation_id=sqlc.arg(conversation_id);

-- name: ConnectorReceipt :one
SELECT fingerprint,receipt FROM connector_receipts
WHERE source_id=sqlc.arg(source_id) AND batch_id=sqlc.arg(batch_id);

-- name: ConnectorRevision :one
SELECT revision FROM connector_sources WHERE id=sqlc.arg(id);

-- name: ConnectorSource :exec
INSERT INTO connector_sources(id,revision) VALUES(sqlc.arg(id),sqlc.arg(revision))
ON CONFLICT(id) DO UPDATE SET revision=excluded.revision;

-- name: ConnectorChat :one
SELECT record FROM connector_chats WHERE source_id=sqlc.arg(source_id) AND id=sqlc.arg(id);

-- name: ConnectorMessage :one
SELECT record FROM connector_messages WHERE source_id=sqlc.arg(source_id) AND id=sqlc.arg(id);

-- name: ConnectorDocument :one
SELECT record FROM connector_documents WHERE source_id=sqlc.arg(source_id) AND id=sqlc.arg(id);

-- name: ConnectorPutChat :execrows
INSERT INTO connector_chats(source_id,id,record) VALUES(sqlc.arg(source_id),sqlc.arg(id),sqlc.arg(record))
ON CONFLICT(source_id,id) DO UPDATE SET record=excluded.record
WHERE connector_chats.record<>excluded.record;

-- name: ConnectorPutMessage :execrows
INSERT INTO connector_messages(source_id,id,chat_id,reply_to,record)
VALUES(sqlc.arg(source_id),sqlc.arg(id),sqlc.arg(chat_id),sqlc.narg(reply_to),sqlc.arg(record))
ON CONFLICT(source_id,id) DO UPDATE SET chat_id=excluded.chat_id,reply_to=excluded.reply_to,record=excluded.record
WHERE connector_messages.record<>excluded.record;

-- name: ConnectorPutDocument :execrows
INSERT INTO connector_documents(source_id,id,record) VALUES(sqlc.arg(source_id),sqlc.arg(id),sqlc.arg(record))
ON CONFLICT(source_id,id) DO UPDATE SET record=excluded.record
WHERE connector_documents.record<>excluded.record;

-- name: ConnectorDeleteChat :execrows
DELETE FROM connector_chats WHERE source_id=sqlc.arg(source_id) AND id=sqlc.arg(id);

-- name: ConnectorDeleteMessage :execrows
DELETE FROM connector_messages WHERE source_id=sqlc.arg(source_id) AND id=sqlc.arg(id);

-- name: ConnectorDeleteDocument :execrows
DELETE FROM connector_documents WHERE source_id=sqlc.arg(source_id) AND id=sqlc.arg(id);

-- name: ConnectorEdge :one
SELECT chat_id,reply_to FROM connector_messages WHERE source_id=sqlc.arg(source_id) AND id=sqlc.arg(id);

-- name: ConnectorChildren :many
SELECT chat_id FROM connector_messages WHERE source_id=sqlc.arg(source_id) AND reply_to=sqlc.arg(id);

-- name: ConnectorSilverRevision :one
SELECT revision FROM silver_state WHERE singleton=1;

-- name: ConnectorSetSilverRevision :exec
UPDATE silver_state SET revision=sqlc.arg(revision) WHERE singleton=1;

-- name: ConnectorPutReceipt :exec
INSERT INTO connector_receipts(source_id,batch_id,fingerprint,receipt)
VALUES(sqlc.arg(source_id),sqlc.arg(batch_id),sqlc.arg(fingerprint),sqlc.arg(receipt));

-- name: ConnectorInputs :one
SELECT CAST(EXISTS(SELECT 1 FROM connector_chats) OR EXISTS(SELECT 1 FROM connector_documents) AS INTEGER) AS present;

-- name: ConnectorAllEdges :many
SELECT source_id,id,chat_id FROM connector_messages ORDER BY source_id,chat_id,id;
