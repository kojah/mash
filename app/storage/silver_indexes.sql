-- Silver indexes and revision triggers, applied after every column exists.
CREATE INDEX IF NOT EXISTS objects_context_parent
    ON objects(conversation_id, context_parent, ordinal) WHERE context_parent IS NOT NULL;
-- Keep readiness counts off canonical bodies and projection overflow pages.
-- Native views cover native entries with text; queries that rely on these
-- indexes must repeat this predicate exactly.
CREATE INDEX IF NOT EXISTS objects_searchable_role ON objects(role)
    WHERE origin = 'provider' AND object_role = 'entry' AND COALESCE(search_projection, content) <> '';
-- Kind scopes must also stay on metadata pages: fetching kind from objects
-- otherwise reads canonical rows even for connector-only searches.
CREATE INDEX IF NOT EXISTS objects_searchable_metadata
    ON objects(created_at, conversation_id, role, kind)
    WHERE origin = 'provider' AND object_role = 'entry' AND COALESCE(search_projection, content) <> '';
CREATE INDEX IF NOT EXISTS objects_searchable_identity ON objects(id, role)
    WHERE origin = 'provider' AND object_role = 'entry' AND COALESCE(search_projection, content) <> '';
-- View writes and scan bookkeeping do not change the canonical revision.
CREATE TRIGGER IF NOT EXISTS silver_turns_INSERT AFTER INSERT ON turns BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_turns_UPDATE AFTER UPDATE ON turns BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_turns_DELETE AFTER DELETE ON turns BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_objects_INSERT AFTER INSERT ON objects BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_objects_UPDATE AFTER UPDATE ON objects BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_objects_DELETE AFTER DELETE ON objects BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_native_chats_INSERT AFTER INSERT ON native_chats BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_native_chats_UPDATE AFTER UPDATE ON native_chats BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_native_chats_DELETE AFTER DELETE ON native_chats BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_native_messages_INSERT AFTER INSERT ON native_messages BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_native_messages_UPDATE AFTER UPDATE ON native_messages BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_native_messages_DELETE AFTER DELETE ON native_messages BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_tombstones_INSERT AFTER INSERT ON tombstones BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_tombstones_UPDATE AFTER UPDATE ON tombstones BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_tombstones_DELETE AFTER DELETE ON tombstones BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_claude_sources_INSERT AFTER INSERT ON claude_sources BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_claude_sources_UPDATE AFTER UPDATE ON claude_sources BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_claude_sources_DELETE AFTER DELETE ON claude_sources BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_claude_source_messages_INSERT AFTER INSERT ON claude_source_messages BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_claude_source_messages_UPDATE AFTER UPDATE ON claude_source_messages BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_claude_source_messages_DELETE AFTER DELETE ON claude_source_messages BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_conversation_enrichment_INSERT AFTER INSERT ON conversation_enrichment BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_conversation_enrichment_UPDATE AFTER UPDATE ON conversation_enrichment BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_conversation_enrichment_DELETE AFTER DELETE ON conversation_enrichment BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
