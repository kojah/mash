-- Keep context traversal and readiness counts off canonical body pages.
CREATE INDEX IF NOT EXISTS messages_context_parent
    ON messages(conversation_id, context_parent, ordinal) WHERE context_parent IS NOT NULL;
CREATE INDEX IF NOT EXISTS messages_searchable_role ON messages(role)
    WHERE COALESCE(search_projection, content) <> '';
CREATE INDEX IF NOT EXISTS messages_searchable_metadata
    ON messages(created_at, conversation_id, role)
    WHERE COALESCE(search_projection, content) <> '';
CREATE INDEX IF NOT EXISTS messages_searchable_identity ON messages(id, role)
    WHERE COALESCE(search_projection, content) <> '';
CREATE TRIGGER IF NOT EXISTS silver_sessions_INSERT AFTER INSERT ON sessions BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_sessions_UPDATE AFTER UPDATE ON sessions BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_sessions_DELETE AFTER DELETE ON sessions BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_messages_INSERT AFTER INSERT ON messages BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_messages_UPDATE AFTER UPDATE ON messages BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_messages_DELETE AFTER DELETE ON messages BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_turns_INSERT AFTER INSERT ON turns BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_turns_UPDATE AFTER UPDATE ON turns BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_turns_DELETE AFTER DELETE ON turns BEGIN
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
