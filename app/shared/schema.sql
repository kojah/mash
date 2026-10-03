-- Current PostgreSQL schema. Historical upgrades live in migrations.sql.
-- Silver: canonical identities, history, native interpretation and provenance.
CREATE TABLE IF NOT EXISTS version (
    singleton BOOLEAN PRIMARY KEY CHECK (singleton), version INTEGER NOT NULL
);
INSERT INTO version VALUES (TRUE, 17) ON CONFLICT DO NOTHING;
CREATE TABLE IF NOT EXISTS nodes (
    node_id TEXT PRIMARY KEY, instance TEXT NOT NULL, revision BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS node_sync (
    node_id TEXT PRIMARY KEY REFERENCES nodes(node_id),
    last_synced_at TIMESTAMPTZ, last_updated_at TIMESTAMPTZ
);
CREATE TABLE IF NOT EXISTS tombstones (
    node_id TEXT NOT NULL REFERENCES nodes(node_id),
    provider TEXT NOT NULL, conversation_id TEXT NOT NULL,
    PRIMARY KEY(node_id, provider, conversation_id)
);
CREATE TABLE IF NOT EXISTS conversations (
    node_id TEXT NOT NULL REFERENCES nodes(node_id),
    id TEXT NOT NULL, provider TEXT NOT NULL CHECK (provider IN
        ('claude-code','codex','aider','hermes','opencode','openclaw','pi',
         'copilot','gemini-cli','cline','cursor','windsurf','antigravity')),
    title TEXT, created_at BIGINT, updated_at BIGINT, project TEXT, repository TEXT,
    PRIMARY KEY(node_id, id)
);
CREATE TABLE IF NOT EXISTS messages (
    node_id TEXT NOT NULL, id TEXT NOT NULL, conversation_id TEXT NOT NULL,
    ordinal BIGINT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL,
    projection TEXT NOT NULL, created_at BIGINT,
    context_parent BIGINT CHECK (context_parent IS NULL OR (context_parent >= -1 AND context_parent < ordinal)),
    PRIMARY KEY(node_id, id), UNIQUE(node_id, conversation_id, ordinal),
    FOREIGN KEY(node_id,conversation_id) REFERENCES conversations(node_id,id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS messages_context_parent ON messages(node_id,conversation_id,context_parent,ordinal) WHERE context_parent IS NOT NULL;
CREATE INDEX IF NOT EXISTS messages_tree_context ON messages(node_id,conversation_id,ordinal,context_parent) WHERE context_parent>=0;
CREATE TABLE IF NOT EXISTS direct_sources (
    node_id TEXT NOT NULL REFERENCES nodes(node_id),
    provider TEXT NOT NULL, path_hash TEXT NOT NULL,
    size_bytes BIGINT NOT NULL, modified_ns BIGINT NOT NULL,
    conversation_id TEXT, fingerprint TEXT NOT NULL DEFAULT '',
    interpretation_version BIGINT NOT NULL DEFAULT 0 CHECK (interpretation_version BETWEEN 0 AND 4294967295),
    PRIMARY KEY(node_id,provider,path_hash)
);

-- Gold: replaceable retrieval data, owned by the corresponding silver message.
-- This table contains no canonical body, role, timestamp or thread metadata.
CREATE TABLE IF NOT EXISTS message_search (
    node_id TEXT NOT NULL, id TEXT NOT NULL,
    generation TEXT, vector public.vector, document TSVECTOR NOT NULL,
    PRIMARY KEY(node_id,id),
    FOREIGN KEY(node_id,id) REFERENCES messages(node_id,id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS message_search_document ON message_search USING GIN(document);
CREATE INDEX IF NOT EXISTS message_search_generation ON message_search(generation);
