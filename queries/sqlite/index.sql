-- name: Counts :one
-- Every count reads only an index, never message bodies: see NativeMessageCount.
SELECT (SELECT count(*) FROM objects INDEXED BY objects_native_container
        WHERE origin='provider' AND object_role='container') AS conversations,
CAST((SELECT count(*) FROM objects INDEXED BY objects_native_id
        WHERE origin='provider' AND object_role<>'container')
    - (SELECT count(*) FROM objects INDEXED BY objects_conversation
        WHERE object_role IN ('segment','document') AND origin='provider') AS INTEGER) AS messages,
(SELECT count(*) FROM objects INDEXED BY objects_searchable_role
        WHERE origin='provider' AND object_role='entry' AND COALESCE(search_projection,content)<>'') AS searchable_messages,
(SELECT count(*) FROM message_embeddings) AS embeddings;

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
SELECT c.id,c.source AS provider,c.title,c.project,c.repository,c.created_at,c.updated_at,e.evidence AS enrichment
FROM objects c CROSS JOIN scope s LEFT JOIN conversation_enrichment e ON e.conversation_id=c.id
WHERE c.origin='provider' AND c.object_role='container' AND (s.all_providers = 1 OR c.source IN (
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
c.source COLLATE BINARY ASC,c.id COLLATE BINARY ASC
LIMIT sqlc.arg(page_size) OFFSET sqlc.arg(page_offset);

-- name: ConversationMessageState :many
SELECT m.id,COALESCE(n.fingerprint,'') AS fingerprint,m.ordinal,COALESCE(m.search_projection,m.content) AS search_text
FROM objects m LEFT JOIN native_messages n ON n.storage_id=m.storage_id
WHERE m.origin='provider' AND m.object_role='entry' AND m.conversation_id=sqlc.arg(conversation_id);

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
conversation_id=excluded.conversation_id,title=excluded.title,project=excluded.project,repository=excluded.repository
WHERE source_path IS NOT excluded.source_path OR conversation_id IS NOT excluded.conversation_id
   OR title IS NOT excluded.title OR project IS NOT excluded.project OR repository IS NOT excluded.repository;

-- name: ClaudeSourceMessageIds :many
SELECT message_id FROM claude_source_messages WHERE source_key=sqlc.arg(source_key);

-- name: DeleteClaudeSourceMessage :exec
DELETE FROM claude_source_messages WHERE source_key=sqlc.arg(source_key) AND message_id=sqlc.arg(message_id);

-- name: UpsertClaudeSourceMessage :exec
-- An unchanged row is not rewritten, so appending to a transcript writes only its new rows.
INSERT INTO claude_source_messages(source_key,message_id,parent_id,role,content,projection,created_at,attachments)
VALUES(sqlc.arg(source_key),sqlc.arg(message_id),sqlc.narg(parent_id),sqlc.arg(role),sqlc.arg(content),sqlc.narg(projection),sqlc.narg(created_at),sqlc.arg(attachments))
ON CONFLICT(source_key,message_id) DO UPDATE SET parent_id=excluded.parent_id,role=excluded.role,
content=excluded.content,projection=excluded.projection,created_at=excluded.created_at,attachments=excluded.attachments
WHERE parent_id IS NOT excluded.parent_id OR role IS NOT excluded.role OR content IS NOT excluded.content
OR projection IS NOT excluded.projection OR created_at IS NOT excluded.created_at OR attachments IS NOT excluded.attachments;

-- name: DeleteClaudeSource :exec
DELETE FROM claude_sources WHERE source_key=sqlc.arg(source_key);

-- name: DeleteClaudeThreadSources :exec
DELETE FROM claude_sources WHERE conversation_id=sqlc.arg(conversation_id);

-- name: ClaudeTombstone :one
SELECT conversation_id FROM tombstones WHERE provider='claude-code' AND conversation_id=sqlc.arg(conversation_id);

-- name: CanonicalThreadMessageCount :one
SELECT count(*) AS messages FROM objects WHERE origin='provider' AND object_role='entry' AND conversation_id=sqlc.arg(conversation_id);

-- name: ParkMessageOrdinal :exec
UPDATE objects SET ordinal=sqlc.arg(ordinal)
WHERE origin='provider' AND object_role='entry' AND conversation_id=sqlc.arg(conversation_id) AND id=sqlc.arg(message_id);

-- name: SearchHit :one
SELECT m.id,m.conversation_id,c.source AS provider,m.role,COALESCE(m.search_projection,m.content) AS content,
m.created_at,c.title,c.project,c.repository,c.created_at AS conversation_created_at,c.updated_at AS conversation_updated_at,e.evidence AS enrichment,
m.kind,m.parent_storage_id
FROM objects m JOIN objects c ON c.origin='provider' AND c.object_role='container' AND c.id=m.conversation_id
LEFT JOIN conversation_enrichment e ON e.conversation_id=c.id WHERE m.storage_id=?1;

-- name: SearchDocument :one
SELECT COALESCE(search_projection,content) AS content FROM objects WHERE origin='provider' AND object_role='entry' AND id=?1;

-- name: Anchor :one
SELECT conversation_id,ordinal,context_parent FROM objects WHERE origin='provider' AND object_role='entry' AND id=?1;

-- name: MessageAttachments :one
SELECT attachments FROM objects WHERE origin='provider' AND object_role='entry' AND id=?1;

-- name: AttachmentCandidates :many
SELECT id,attachments FROM objects WHERE origin='provider' AND object_role='entry' AND attachments<>'[]' ORDER BY id;

-- name: AttachmentPolicy :one
SELECT fingerprint FROM attachment_policy WHERE singleton=1;

-- name: SetAttachmentPolicy :exec
INSERT INTO attachment_policy(singleton,fingerprint) VALUES(1,sqlc.arg(fingerprint)) ON CONFLICT(singleton) DO UPDATE SET fingerprint=excluded.fingerprint;

-- name: ViewMessages :many
SELECT id,ordinal,role,content,COALESCE(search_projection,content) AS projection,created_at FROM objects
WHERE origin='provider' AND object_role='entry' AND conversation_id=sqlc.arg(conversation_id) AND ordinal>=sqlc.arg(first_ordinal) AND ordinal<=sqlc.arg(last_ordinal) ORDER BY ordinal;

-- name: ContextMessage :one
SELECT id,ordinal,role,content,COALESCE(search_projection,content) AS projection,created_at,context_parent FROM objects
WHERE origin='provider' AND object_role='entry' AND conversation_id=sqlc.arg(conversation_id) AND ordinal=sqlc.arg(ordinal);

-- name: FirstChild :one
SELECT ordinal FROM objects WHERE origin='provider' AND object_role='entry' AND conversation_id=sqlc.arg(conversation_id) AND context_parent=sqlc.arg(parent) ORDER BY ordinal LIMIT 1;

-- name: SilverCertificate :one
SELECT s.revision, s.interpretation_version,
    CAST(EXISTS(SELECT 1 FROM silver_scans WHERE state=0) AS INTEGER) AS pending,
    CAST(s.interpretation_version<>sqlc.arg(current_version)
        OR EXISTS(SELECT 1 FROM silver_scans WHERE interpretation_version<>sqlc.arg(current_version))
        OR EXISTS(SELECT 1 FROM source_checkpoints WHERE interpretation_version<>sqlc.arg(current_version))
        OR EXISTS(SELECT 1 FROM native_chats c WHERE NOT EXISTS(
            SELECT 1 FROM silver_scans p WHERE p.provider=c.provider
              AND p.interpretation_version=sqlc.arg(current_version))) AS INTEGER) AS unverified,
    CAST(EXISTS(SELECT 1 FROM silver_scans WHERE state=2) AS INTEGER) AS incomplete,
    (SELECT count(*) FROM silver_rejections) AS rejected_sources
FROM silver_state s WHERE s.singleton=1;

-- name: SilverRejections :many
SELECT source_key,source_path,modified_ns FROM silver_rejections
WHERE provider=sqlc.arg(provider) ORDER BY source_key;

-- name: ViewManifest :one
SELECT manifest FROM views_manifest WHERE singleton=1;

-- name: LexicalManifest :one
SELECT manifest FROM lexical_manifest WHERE singleton=1;

-- name: SourceEnrichment :many
SELECT e.evidence,c.title,c.project,c.repository,c.created_at,c.updated_at
FROM native_chats AS n JOIN objects AS c ON c.storage_id=n.storage_id
LEFT JOIN conversation_enrichment AS e ON e.conversation_id=c.id
WHERE n.provider=sqlc.arg(provider) AND n.source_key=sqlc.arg(source_key);

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

-- name: ConnectorObject :one
SELECT object_role AS role,parent_id,reply_to,record FROM objects
WHERE origin='connector' AND source=sqlc.arg(source_id) AND id=sqlc.arg(id);

-- name: ConnectorPutObject :execrows
-- conversation_id starts as the object's own ID and the parent edge unlinked;
-- the link queries and ConnectorSetConversation fix both.
INSERT INTO objects(origin,source,id,object_role,kind,parent_id,reply_to,conversation_id,
    author,title,content,created_at,updated_at,url,metadata,attachments,record)
VALUES('connector',sqlc.arg(source_id),sqlc.arg(id),sqlc.arg(role),sqlc.arg(kind),
    sqlc.narg(parent_id),sqlc.narg(reply_to),sqlc.arg(id),sqlc.narg(author),sqlc.narg(title),
    sqlc.narg(content),sqlc.narg(created_at),sqlc.narg(updated_at),sqlc.narg(url),
    sqlc.narg(metadata),sqlc.arg(attachments),sqlc.arg(record))
ON CONFLICT(source,id) WHERE origin='connector' DO UPDATE SET object_role=excluded.object_role,kind=excluded.kind,
parent_id=excluded.parent_id,reply_to=excluded.reply_to,author=excluded.author,
title=excluded.title,content=excluded.content,created_at=excluded.created_at,
updated_at=excluded.updated_at,url=excluded.url,metadata=excluded.metadata,
attachments=excluded.attachments,record=excluded.record
WHERE objects.record<>excluded.record;

-- name: ConnectorLinkParent :exec
-- Unchanged edges are left alone so revision triggers see no change.
UPDATE objects SET parent_storage_id=(SELECT p.storage_id FROM objects c
    JOIN objects p ON p.origin='connector' AND p.source=c.source AND p.id=c.parent_id
    WHERE c.origin='connector' AND c.source=sqlc.arg(source_id) AND c.id=sqlc.arg(id))
WHERE origin='connector' AND source=sqlc.arg(source_id) AND id=sqlc.arg(id)
  AND parent_storage_id IS NOT (SELECT p.storage_id FROM objects c
    JOIN objects p ON p.origin='connector' AND p.source=c.source AND p.id=c.parent_id
    WHERE c.origin='connector' AND c.source=sqlc.arg(source_id) AND c.id=sqlc.arg(id));

-- name: ConnectorLinkChildren :exec
-- Children may have arrived before their parent in the same batch.
UPDATE objects SET parent_storage_id=(SELECT p.storage_id FROM objects p
    WHERE p.origin='connector' AND p.source=sqlc.arg(source_id) AND p.id=sqlc.arg(id))
WHERE origin='connector' AND source=sqlc.arg(source_id) AND parent_id=sqlc.arg(id)
  AND parent_storage_id IS NOT (SELECT p.storage_id FROM objects p
    WHERE p.origin='connector' AND p.source=sqlc.arg(source_id) AND p.id=sqlc.arg(id));

-- name: ConnectorSetConversation :exec
UPDATE objects SET conversation_id=sqlc.arg(conversation_id)
WHERE origin='connector' AND source=sqlc.arg(source_id) AND id=sqlc.arg(id)
  AND conversation_id<>sqlc.arg(conversation_id);

-- name: ConnectorDeleteObject :execrows
DELETE FROM objects WHERE origin='connector' AND source=sqlc.arg(source_id) AND id=sqlc.arg(id);

-- name: ConnectorSchema :one
SELECT schema FROM connector_schemas WHERE source_id=sqlc.arg(source_id);

-- name: ConnectorPutSchema :execrows
INSERT INTO connector_schemas(source_id,schema) VALUES(sqlc.arg(source_id),sqlc.arg(schema))
ON CONFLICT(source_id) DO UPDATE SET schema=excluded.schema
WHERE connector_schemas.schema<>excluded.schema;

-- name: ConnectorSourceObjects :many
SELECT id FROM objects WHERE origin='connector' AND source=sqlc.arg(source_id) ORDER BY id;

-- name: ConnectorAllSchemas :many
SELECT source_id,schema FROM connector_schemas ORDER BY source_id;

-- name: ConnectorChildren :many
SELECT id,object_role AS role,record FROM objects
WHERE origin='connector' AND source=sqlc.arg(source_id) AND parent_id=sqlc.arg(parent_id) ORDER BY id;

-- name: ConnectorSilverRevision :one
SELECT revision FROM silver_state WHERE singleton=1;

-- name: ConnectorSetSilverRevision :exec
UPDATE silver_state SET revision=sqlc.arg(revision) WHERE singleton=1;

-- name: ConnectorPutReceipt :exec
INSERT INTO connector_receipts(source_id,batch_id,fingerprint,receipt)
VALUES(sqlc.arg(source_id),sqlc.arg(batch_id),sqlc.arg(fingerprint),sqlc.arg(receipt));

-- name: ConnectorAllObjects :many
SELECT source AS source_id,id,object_role AS role,parent_id FROM objects WHERE origin='connector' ORDER BY source,id;

-- name: ConnectorUnitDigests :many
SELECT anchor, id, digest FROM connector_units;

-- name: ConnectorSilverObjects :many
SELECT source AS source_id, id, object_role AS role, parent_id, record FROM objects
WHERE origin = 'connector' ORDER BY source, id;

-- name: ConnectorInsertUnit :one
INSERT INTO connector_units(anchor, role, label, source_id, record_id, group_anchor, created_at, digest)
VALUES (sqlc.arg(anchor), sqlc.arg(role), sqlc.arg(label), sqlc.arg(source_id), sqlc.arg(record_id),
        sqlc.arg(group_anchor), sqlc.narg(created_at), sqlc.arg(digest))
RETURNING id;

-- name: ConnectorUpdateUnit :exec
UPDATE connector_units
   SET label = sqlc.arg(label), group_anchor = sqlc.arg(group_anchor),
       created_at = sqlc.narg(created_at), digest = sqlc.arg(digest)
 WHERE id = sqlc.arg(id);

-- name: ConnectorDeleteUnit :exec
DELETE FROM connector_units WHERE id = sqlc.arg(id);

-- name: ConnectorDeleteUnitEmbedding :exec
DELETE FROM connector_embeddings WHERE unit_id = sqlc.arg(unit_id);

-- name: ConnectorUnit :one
SELECT source_id, record_id FROM connector_units WHERE id = sqlc.arg(id);

-- name: ConnectorPendingEmbeddings :many
SELECT u.id, u.source_id, u.record_id
  FROM connector_units u
  LEFT JOIN connector_embeddings e ON e.unit_id = u.id AND e.generation = sqlc.arg(generation)
 WHERE e.unit_id IS NULL
 ORDER BY u.id
 LIMIT sqlc.arg(page_size);

-- name: ConnectorPutEmbedding :exec
INSERT INTO connector_embeddings(unit_id, generation, dimensions, norm, vector)
VALUES (sqlc.arg(unit_id), sqlc.arg(generation), sqlc.arg(dimensions), sqlc.arg(norm), sqlc.arg(vector))
ON CONFLICT(unit_id) DO UPDATE SET generation = excluded.generation,
    dimensions = excluded.dimensions, norm = excluded.norm, vector = excluded.vector;

-- name: ConnectorEmbeddingsComplete :one
SELECT CAST(NOT EXISTS(
    SELECT 1 FROM connector_units u
      LEFT JOIN connector_embeddings e ON e.unit_id = u.id AND e.generation = sqlc.arg(generation)
     WHERE e.unit_id IS NULL) AS INTEGER) AS complete;

-- name: ConnectorClearEmbeddings :exec
DELETE FROM connector_embeddings;

-- name: ConnectorContainers :many
SELECT source AS source_id, id, record FROM objects
WHERE origin = 'connector' AND object_role = 'container';

-- name: NativeMessageCount :one
-- Entries are the native non-containers that are neither segments nor documents.
-- Counting a table predicate on object_role would read every row's body pages;
-- the native identity index and the few segment lookups are proportional to
-- the index, not the archive.
SELECT CAST((SELECT count(*) FROM objects INDEXED BY objects_native_id
        WHERE origin='provider' AND object_role<>'container')
    - (SELECT count(*) FROM objects INDEXED BY objects_conversation
        WHERE object_role IN ('segment','document') AND origin='provider') AS INTEGER) AS messages;

-- name: NativeSourceFingerprint :one
SELECT source_fingerprint FROM native_chats WHERE conversation_id=sqlc.arg(id);

-- name: UpsertNativeChat :exec
INSERT INTO objects(origin,source,id,object_role,kind,conversation_id,title,created_at,updated_at,project,repository)
VALUES('provider',sqlc.arg(provider),sqlc.arg(id),'container',sqlc.arg(kind),sqlc.arg(id),
    sqlc.narg(title),sqlc.narg(created_at),sqlc.narg(updated_at),sqlc.narg(project),sqlc.narg(repository))
ON CONFLICT(id) WHERE origin='provider' AND object_role='container' DO UPDATE SET kind=excluded.kind,title=excluded.title,
created_at=excluded.created_at,updated_at=excluded.updated_at,project=excluded.project,
repository=excluded.repository;

-- name: UpsertNativeChatSource :exec
INSERT INTO native_chats(storage_id,conversation_id,provider,source_path,source_fingerprint,source_key,source_session)
SELECT o.storage_id,o.id,o.source,sqlc.arg(source_path),sqlc.arg(source_fingerprint),sqlc.narg(source_key),sqlc.narg(source_session)
FROM objects o WHERE o.origin='provider' AND o.object_role='container' AND o.id=sqlc.arg(id)
ON CONFLICT(conversation_id) DO UPDATE SET provider=excluded.provider,source_path=excluded.source_path,
source_fingerprint=excluded.source_fingerprint,source_key=excluded.source_key,
source_session=excluded.source_session;

-- name: DisplacedNativeMessages :one
SELECT (SELECT count(*) FROM objects m WHERE m.origin='provider' AND m.object_role='entry'
    AND m.conversation_id=n.conversation_id) AS messages
FROM native_chats n
WHERE n.provider=sqlc.arg(provider) AND n.source_key=sqlc.arg(source_key) AND n.conversation_id<>sqlc.arg(id);

-- name: UpsertNativeMessage :exec
-- A native message takes its provider from its chat, and sits under it.
INSERT INTO objects(origin,source,id,object_role,kind,parent_id,parent_storage_id,conversation_id,
    ordinal,role,content,search_projection,created_at,context_parent,attachments)
SELECT 'provider',c.source,sqlc.arg(id),'entry',c.source||':message',c.id,c.storage_id,c.id,
    sqlc.arg(ordinal),sqlc.arg(role),sqlc.arg(content),sqlc.narg(search_projection),
    sqlc.narg(created_at),sqlc.narg(context_parent),sqlc.arg(attachments)
FROM objects c WHERE c.origin='provider' AND c.object_role='container' AND c.id=sqlc.arg(conversation_id)
ON CONFLICT(id) WHERE origin='provider' AND object_role<>'container' DO UPDATE SET
source=excluded.source,kind=excluded.kind,parent_id=excluded.parent_id,
parent_storage_id=excluded.parent_storage_id,
conversation_id=excluded.conversation_id,ordinal=excluded.ordinal,role=excluded.role,
content=excluded.content,search_projection=excluded.search_projection,created_at=excluded.created_at,
context_parent=excluded.context_parent,attachments=excluded.attachments;

-- name: UpsertNativeMessageFingerprint :exec
INSERT INTO native_messages(storage_id,id,fingerprint)
SELECT o.storage_id,o.id,sqlc.arg(fingerprint) FROM objects o WHERE o.origin='provider' AND o.object_role='entry' AND o.id=sqlc.arg(id)
ON CONFLICT(id) DO UPDATE SET fingerprint=excluded.fingerprint;

-- name: DeleteNativeChat :execrows
-- Its turns and messages go with it through the parent edge.
DELETE FROM objects WHERE origin='provider' AND object_role='container' AND id=sqlc.arg(id);

-- name: DeleteNativeMessage :exec
DELETE FROM objects WHERE origin='provider' AND object_role='entry' AND id=sqlc.arg(id);

-- name: NativeChatsForPurge :many
SELECT n.conversation_id AS id,n.source_path,n.source_key,k.modified_ns
FROM native_chats n LEFT JOIN source_checkpoints k ON k.provider=n.provider AND k.source_key=n.source_key
WHERE n.provider=sqlc.arg(provider);

-- name: NativeChatBySource :one
SELECT conversation_id AS id FROM native_chats WHERE provider=sqlc.arg(provider) AND source_key=sqlc.arg(source_key);

-- name: NativeChatProvider :one
SELECT provider FROM native_chats WHERE conversation_id=sqlc.arg(id);

-- name: TurnSources :many
SELECT id,ordinal,role,COALESCE(search_projection,content) AS projection,context_parent,created_at
FROM objects WHERE origin='provider' AND object_role='entry' AND conversation_id=sqlc.arg(conversation_id)
ORDER BY ordinal;

-- name: ChatTurns :many
SELECT id FROM objects WHERE origin='provider' AND object_role='segment' AND conversation_id=sqlc.arg(conversation_id);

-- name: UpsertTurn :exec
-- Turns sit under their chat; unchanged turns are left alone so revision
-- triggers see no change.
INSERT INTO objects(origin,source,id,object_role,kind,parent_id,parent_storage_id,conversation_id,
    created_at,updated_at,metadata)
SELECT 'provider',c.source,sqlc.arg(id),'segment',c.source||':turn',c.id,c.storage_id,c.id,
    sqlc.narg(created_at),sqlc.narg(updated_at),sqlc.arg(metadata)
FROM objects c WHERE c.origin='provider' AND c.object_role='container' AND c.id=sqlc.arg(conversation_id)
ON CONFLICT(id) WHERE origin='provider' AND object_role<>'container' DO UPDATE SET
created_at=excluded.created_at,updated_at=excluded.updated_at,metadata=excluded.metadata
WHERE objects.created_at IS NOT excluded.created_at OR objects.updated_at IS NOT excluded.updated_at
   OR objects.metadata IS NOT excluded.metadata;

-- name: SegmentStorage :many
SELECT id,storage_id FROM objects
WHERE origin='provider' AND object_role IN ('container','segment') AND conversation_id=sqlc.arg(conversation_id);

-- name: SetEntryParent :exec
-- A message's parent is its turn, or its chat when it belongs to none.
-- Repeat the partial native-ID uniqueness predicate: SQLite does not infer it
-- from object_role='entry'. Otherwise it scans the chat for every update.
UPDATE objects SET parent_id=sqlc.arg(parent_id),parent_storage_id=sqlc.arg(parent_storage_id)
WHERE origin='provider' AND object_role='entry' AND object_role<>'container'
  AND conversation_id=sqlc.arg(conversation_id)
  AND id=sqlc.arg(id) AND parent_storage_id IS NOT sqlc.arg(parent_storage_id);

-- name: DeleteTurn :exec
DELETE FROM objects WHERE origin='provider' AND object_role='segment' AND id=sqlc.arg(id);

-- name: NativeChatIds :many
SELECT id FROM objects WHERE origin='provider' AND object_role='container' ORDER BY id;

-- name: ObjectAncestor :one
SELECT id,object_role,kind,title,parent_storage_id FROM objects WHERE storage_id=sqlc.arg(storage_id);

-- name: NativeEntryParent :one
SELECT parent_storage_id FROM objects WHERE origin='provider' AND object_role='entry' AND id=sqlc.arg(id);

-- name: ConnectorRecordActivity :exec
-- Absent values keep what was stored: a batch records only its own delivery,
-- and replaces declared modes only when it states them.
INSERT INTO connector_activity(source_id,modes,snapshot_at,complete_snapshot_at,live_at,change_at)
VALUES(sqlc.arg(source_id),sqlc.arg(modes),sqlc.narg(snapshot_at),sqlc.narg(complete_snapshot_at),
    sqlc.narg(live_at),sqlc.narg(change_at))
ON CONFLICT(source_id) DO UPDATE SET
modes=CASE WHEN excluded.modes='[]' THEN connector_activity.modes ELSE excluded.modes END,
snapshot_at=COALESCE(excluded.snapshot_at,connector_activity.snapshot_at),
complete_snapshot_at=COALESCE(excluded.complete_snapshot_at,connector_activity.complete_snapshot_at),
live_at=COALESCE(excluded.live_at,connector_activity.live_at),
change_at=COALESCE(excluded.change_at,connector_activity.change_at);

-- name: ConnectorActivity :many
SELECT s.id AS source_id,a.modes,a.snapshot_at,a.complete_snapshot_at,a.live_at,a.change_at
FROM connector_sources s LEFT JOIN connector_activity a ON a.source_id=s.id ORDER BY s.id;
