-- Silver indexes and revision triggers, applied after every column exists.
CREATE INDEX IF NOT EXISTS messages_context_parent
    ON messages(conversation_id, context_parent, ordinal) WHERE context_parent IS NOT NULL;
-- Keep readiness counts off canonical bodies and projection overflow pages.
-- The partial predicate must match semantic coverage's eligibility rule.
CREATE INDEX IF NOT EXISTS messages_searchable_role ON messages(role)
    WHERE COALESCE(search_projection, content) <> '';
CREATE INDEX IF NOT EXISTS messages_searchable_metadata
    ON messages(created_at, conversation_id, role)
    WHERE COALESCE(search_projection, content) <> '';
CREATE INDEX IF NOT EXISTS messages_searchable_identity ON messages(id, role)
    WHERE COALESCE(search_projection, content) <> '';
CREATE INDEX IF NOT EXISTS source_checkpoints_conversation
    ON source_checkpoints(conversation_id) WHERE conversation_id IS NOT NULL;
-- Gold writes and scan bookkeeping do not change the canonical revision.
CREATE TRIGGER IF NOT EXISTS silver_conversations_INSERT AFTER INSERT ON conversations BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_conversations_UPDATE AFTER UPDATE ON conversations BEGIN
    UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
END;
CREATE TRIGGER IF NOT EXISTS silver_conversations_DELETE AFTER DELETE ON conversations BEGIN
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
