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
    id TEXT NOT NULL, provider TEXT NOT NULL CHECK (provider IN ('claude-code', 'codex', 'aider')),
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
-- Preserve history while adding project/repository metadata.
DO $$ BEGIN
    IF (SELECT version FROM version WHERE singleton) = 3 THEN
        ALTER TABLE conversations ADD COLUMN IF NOT EXISTS project TEXT;
        ALTER TABLE conversations ADD COLUMN IF NOT EXISTS repository TEXT;
        UPDATE version SET version=4 WHERE singleton;
    END IF;
END $$;
-- Routed shared ingestion keeps its independent source checkpoints here.
-- Source paths are hashed before upload.
CREATE TABLE IF NOT EXISTS direct_sources (
    node_id TEXT NOT NULL REFERENCES nodes(node_id),
    provider TEXT NOT NULL, path_hash TEXT NOT NULL,
    size_bytes BIGINT NOT NULL, modified_ns BIGINT NOT NULL,
    conversation_id TEXT,
    PRIMARY KEY(node_id, provider, path_hash)
);
-- New provider adapters do not require rebuilding the shared history tables.
DO $$ BEGIN
    IF (SELECT version FROM version WHERE singleton) = 4 THEN
        ALTER TABLE conversations DROP CONSTRAINT conversations_provider_check;
        ALTER TABLE conversations ADD CONSTRAINT conversations_provider_check
            CHECK (provider IN ('claude-code', 'codex', 'aider'));
        UPDATE version SET version=5 WHERE singleton;
    END IF;
END $$;
DO $$ BEGIN
    IF (SELECT version FROM version WHERE singleton) = 5 THEN
        ALTER TABLE conversations DROP CONSTRAINT conversations_provider_check;
        ALTER TABLE conversations ADD CONSTRAINT conversations_provider_check
            CHECK (provider IN ('claude-code', 'codex', 'aider', 'hermes'));
        ALTER TABLE direct_sources ADD COLUMN IF NOT EXISTS fingerprint TEXT NOT NULL DEFAULT '';
        UPDATE version SET version=6 WHERE singleton;
    END IF;
END $$;
DO $$ BEGIN
    IF (SELECT version FROM version WHERE singleton) = 6 THEN
        ALTER TABLE conversations DROP CONSTRAINT conversations_provider_check;
        ALTER TABLE conversations ADD CONSTRAINT conversations_provider_check
            CHECK (provider IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode'));
        UPDATE version SET version=7 WHERE singleton;
    END IF;
END $$;
DO $$ BEGIN
    IF (SELECT version FROM version WHERE singleton) = 7 THEN
        ALTER TABLE conversations DROP CONSTRAINT conversations_provider_check;
        ALTER TABLE conversations ADD CONSTRAINT conversations_provider_check
            CHECK (provider IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw'));
        UPDATE version SET version=8 WHERE singleton;
    END IF;
END $$;

DO $$ BEGIN
    IF (SELECT version FROM version WHERE singleton) = 8 THEN
        ALTER TABLE conversations DROP CONSTRAINT conversations_provider_check;
        ALTER TABLE conversations ADD CONSTRAINT conversations_provider_check
            CHECK (provider IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi'));
        ALTER TABLE messages ADD COLUMN IF NOT EXISTS context_parent BIGINT CHECK (context_parent IS NULL OR (context_parent >= -1 AND context_parent < ordinal));
        CREATE INDEX IF NOT EXISTS messages_context_parent ON messages(node_id,conversation_id,context_parent,ordinal) WHERE context_parent IS NOT NULL;
        CREATE INDEX IF NOT EXISTS messages_tree_context ON messages(node_id,conversation_id,ordinal,context_parent) WHERE context_parent>=0;
        UPDATE version SET version=9 WHERE singleton;
    END IF;
END $$;

DO $$ BEGIN
    IF (SELECT version FROM version WHERE singleton) = 9 THEN
        ALTER TABLE conversations DROP CONSTRAINT conversations_provider_check;
        ALTER TABLE conversations ADD CONSTRAINT conversations_provider_check
            CHECK (provider IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot'));
        UPDATE version SET version=10 WHERE singleton;
    END IF;
END $$;

DO $$ BEGIN
    IF (SELECT version FROM version WHERE singleton) = 10 THEN
        ALTER TABLE conversations DROP CONSTRAINT conversations_provider_check;
        ALTER TABLE conversations ADD CONSTRAINT conversations_provider_check
            CHECK (provider IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot', 'gemini-cli'));
        UPDATE version SET version=11 WHERE singleton;
    END IF;
END $$;

DO $$ BEGIN
    IF (SELECT version FROM version WHERE singleton) = 11 THEN
        ALTER TABLE conversations DROP CONSTRAINT conversations_provider_check;
        ALTER TABLE conversations ADD CONSTRAINT conversations_provider_check
            CHECK (provider IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot', 'gemini-cli', 'cline'));
        UPDATE version SET version=12 WHERE singleton;
    END IF;
END $$;

DO $$ BEGIN
    IF (SELECT version FROM version WHERE singleton) = 12 THEN
        ALTER TABLE conversations DROP CONSTRAINT conversations_provider_check;
        ALTER TABLE conversations ADD CONSTRAINT conversations_provider_check
            CHECK (provider IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot', 'gemini-cli', 'cline', 'cursor'));
        UPDATE version SET version=13 WHERE singleton;
    END IF;
END $$;
