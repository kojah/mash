CREATE TABLE IF NOT EXISTS version (
    singleton BOOLEAN PRIMARY KEY CHECK (singleton), version INTEGER NOT NULL
);
INSERT INTO version VALUES (TRUE, 3) ON CONFLICT DO NOTHING;
CREATE TABLE IF NOT EXISTS nodes (
    node_id TEXT PRIMARY KEY, instance TEXT NOT NULL, revision BIGINT NOT NULL
);
-- Additive metadata: older publications remain readable and have unknown freshness.
CREATE TABLE IF NOT EXISTS node_sync (
    node_id TEXT PRIMARY KEY REFERENCES nodes(node_id),
    last_synced_at TIMESTAMPTZ,
    last_updated_at TIMESTAMPTZ
);
CREATE TABLE IF NOT EXISTS tombstones (
    node_id TEXT NOT NULL REFERENCES nodes(node_id),
    provider TEXT NOT NULL, conversation_id TEXT NOT NULL,
    PRIMARY KEY(node_id, provider, conversation_id)
);
CREATE TABLE IF NOT EXISTS conversations (
    node_id TEXT NOT NULL REFERENCES nodes(node_id),
    id TEXT NOT NULL, provider TEXT NOT NULL CHECK (provider IN ('claude-code', 'codex')),
    title TEXT, created_at BIGINT, updated_at BIGINT,
    PRIMARY KEY(node_id, id)
);
CREATE TABLE IF NOT EXISTS messages (
    node_id TEXT NOT NULL, id TEXT NOT NULL, conversation_id TEXT NOT NULL,
    ordinal BIGINT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL,
    projection TEXT NOT NULL, created_at BIGINT, generation TEXT, vector public.vector,
    document TSVECTOR GENERATED ALWAYS AS (to_tsvector('simple', projection)) STORED,
    PRIMARY KEY(node_id, id), UNIQUE(node_id, conversation_id, ordinal),
    FOREIGN KEY(node_id, conversation_id) REFERENCES conversations(node_id, id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS messages_document ON messages USING GIN(document);
CREATE INDEX IF NOT EXISTS messages_generation ON messages(generation);
-- Preserve the original shared index while switching its vectors to pgvector.
-- Incomplete old embeddings remain NULL and are repaired by node reindexing.
DO $$ BEGIN
    IF (SELECT version FROM version WHERE singleton) = 1 THEN
        ALTER TABLE messages ALTER COLUMN vector TYPE public.vector(384)
        USING CASE WHEN cardinality(vector)=384 THEN vector::real[]::public.vector(384) END;
        UPDATE version SET version=2 WHERE singleton;
    END IF;
END $$;
-- Remove the dimension typmod while preserving existing vectors. The HTTP
-- service enforces the configured group's embedding dimensions on every write.
DO $$ BEGIN
    IF (SELECT version FROM version WHERE singleton) = 2 THEN
        ALTER TABLE messages ALTER COLUMN vector TYPE public.vector USING vector::public.vector;
        UPDATE version SET version=3 WHERE singleton;
    END IF;
END $$;
-- Shared-only nodes keep ingestion checkpoints in PostgreSQL, never SQLite.
-- Source paths are hashed before upload.
CREATE TABLE IF NOT EXISTS direct_sources (
    node_id TEXT NOT NULL REFERENCES nodes(node_id),
    provider TEXT NOT NULL, path_hash TEXT NOT NULL,
    size_bytes BIGINT NOT NULL, modified_ns BIGINT NOT NULL,
    conversation_id TEXT,
    PRIMARY KEY(node_id, provider, path_hash)
);
