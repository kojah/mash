-- Explicit columns preserve databases created through earlier migrations.
INSERT INTO conversations(id,provider,source_path,source_key,source_session,title,created_at,updated_at,source_fingerprint,project,repository)
SELECT id,provider,source_path,source_key,source_session,title,created_at,updated_at,source_fingerprint,project,repository FROM history.conversations;
INSERT INTO messages(attachments,storage_id,id,conversation_id,ordinal,role,content,search_projection,context_parent,created_at,fingerprint)
SELECT attachments,storage_id,id,conversation_id,ordinal,role,content,search_projection,context_parent,created_at,fingerprint FROM history.messages;
INSERT INTO conversation_enrichment SELECT * FROM history.conversation_enrichment;
INSERT INTO tombstones SELECT * FROM history.tombstones;
INSERT INTO claude_sources(source_key,source_path,conversation_id,title,project,repository)
SELECT source_key,source_path,conversation_id,title,project,repository FROM history.claude_sources;
INSERT INTO claude_source_messages(attachments,source_key,message_id,parent_id,role,content,projection,created_at)
SELECT attachments,source_key,message_id,parent_id,role,content,projection,created_at FROM history.claude_source_messages;
INSERT INTO source_checkpoints(provider,source_path,source_key,size_bytes,modified_ns,fingerprint,interpretation_version)
SELECT provider,source_path,source_key,size_bytes,modified_ns,fingerprint,interpretation_version FROM history.source_checkpoints;
INSERT INTO silver_scans SELECT * FROM history.silver_scans;
INSERT INTO silver_rejections SELECT * FROM history.silver_rejections;
INSERT INTO attachment_policy SELECT * FROM history.attachment_policy;
INSERT INTO shared_forgets SELECT * FROM history.shared_forgets;
UPDATE silver_state SET revision=(SELECT revision FROM history.silver_state WHERE singleton=1),
interpretation_version=(SELECT interpretation_version FROM history.silver_state WHERE singleton=1) WHERE singleton=1;
DELETE FROM sqlite_sequence WHERE name='messages';
INSERT INTO sqlite_sequence(name,seq) SELECT name,seq FROM history.sqlite_sequence WHERE name='messages';
-- An imported index must reconcile anew, never reuse another installation's ACK.
UPDATE shared_publication SET revision=1,acknowledged=0,target='' WHERE singleton=1;
DELETE FROM shared_changes;
UPDATE shared_change_state SET floor=1 WHERE singleton=1;
