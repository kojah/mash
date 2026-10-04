-- Silver publication ownership and durable reconciliation state.
-- Gold build identity: exactly one active projection, without copied history.
CREATE TABLE IF NOT EXISTS gold_manifest (
    singleton INTEGER PRIMARY KEY CHECK (singleton=1),
    manifest TEXT
);
INSERT OR IGNORE INTO gold_manifest(singleton) VALUES (1);
-- Independent local lexical publication; contains no copied canonical text.
CREATE TABLE IF NOT EXISTS lexical_manifest (
    singleton INTEGER PRIMARY KEY CHECK (singleton=1),
    manifest TEXT
);
INSERT OR IGNORE INTO lexical_manifest(singleton) VALUES (1);
-- Silver metadata evidence; no copied canonical content or retrieval settings.
CREATE TABLE IF NOT EXISTS conversation_enrichment (
    conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    evidence TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS silver_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
    interpretation_version INTEGER NOT NULL DEFAULT 0 CHECK (interpretation_version >= 0)
);
INSERT OR IGNORE INTO silver_state(singleton) VALUES (1);
CREATE TABLE IF NOT EXISTS silver_scans (
    provider TEXT PRIMARY KEY,
    interpretation_version INTEGER NOT NULL CHECK (interpretation_version > 0),
    -- 0=pending, 1=complete, 2=incomplete.
    state INTEGER NOT NULL CHECK (state IN (0, 1, 2))
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS silver_rejections (
    provider TEXT NOT NULL,
    source_key TEXT NOT NULL,
    source_path TEXT NOT NULL,
    modified_ns INTEGER NOT NULL,
    PRIMARY KEY(provider, source_key)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS shared_publication (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    instance TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    acknowledged INTEGER NOT NULL DEFAULT 0,
    target TEXT NOT NULL DEFAULT ''
);
INSERT OR IGNORE INTO shared_publication(singleton, instance)
VALUES (1, lower(hex(randomblob(16))));
CREATE TABLE IF NOT EXISTS shared_forgets (
    scope TEXT NOT NULL, conversation_id TEXT NOT NULL,
    PRIMARY KEY(scope, conversation_id)
);
-- Silver canonical history and provider-native interpretation.
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL CHECK (
        provider IN (
            'claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot', 'gemini-cli', 'cline', 'cursor', 'windsurf', 'antigravity'
        )
    ),
    source_path TEXT NOT NULL,
    source_key TEXT NOT NULL,
    source_session TEXT,
    title TEXT,
    created_at INTEGER,
    updated_at INTEGER,
    source_fingerprint TEXT NOT NULL DEFAULT '',
    project TEXT,
    repository TEXT,
    UNIQUE (provider, source_key)
);
CREATE TABLE IF NOT EXISTS messages (
    attachments TEXT NOT NULL DEFAULT '[]',
    storage_id INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    -- Silver's interpreted visible dialogue; the Rust model calls this dialogue_text.
    -- The existing column name is retained without copying bodies or changing schema.
    search_projection TEXT,
    context_parent INTEGER CHECK (context_parent IS NULL OR (context_parent >= -1 AND context_parent < ordinal)),
    created_at INTEGER,
    fingerprint TEXT NOT NULL DEFAULT '',
    UNIQUE (conversation_id, ordinal)
);
-- Gold retrieval state, rebuilt from silver without provider source files.
CREATE TABLE IF NOT EXISTS message_embeddings (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    generation TEXT NOT NULL DEFAULT '',
    dimensions INTEGER NOT NULL,
    norm REAL NOT NULL,
    vector BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS pending_embeddings (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS semantic_chunks (
    chunk_id INTEGER PRIMARY KEY CHECK (chunk_id >= 0),
    generation TEXT NOT NULL,
    dimensions INTEGER NOT NULL CHECK (dimensions > 0),
    vector_count INTEGER NOT NULL CHECK (vector_count > 0 AND vector_count <= 4096),
    message_rowids BLOB NOT NULL,
    norms BLOB NOT NULL,
    providers BLOB NOT NULL,
    created_ats BLOB NOT NULL,
    vectors BLOB NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
    content,
    message_id UNINDEXED,
    conversation_id UNINDEXED,
    tokenize = 'unicode61'
);
-- Silver provenance, deletion intent and provider replay checkpoints.
CREATE TABLE IF NOT EXISTS tombstones (
    provider TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    forgotten_at INTEGER NOT NULL,
    PRIMARY KEY (provider, conversation_id)
);
CREATE TABLE IF NOT EXISTS source_checkpoints (
    provider TEXT NOT NULL,
    source_path TEXT NOT NULL,
    source_key TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    modified_ns INTEGER NOT NULL,
    fingerprint TEXT NOT NULL DEFAULT '',
    interpretation_version INTEGER NOT NULL DEFAULT 0 CHECK (interpretation_version >= 0),
    PRIMARY KEY (provider, source_key)
);
CREATE TABLE IF NOT EXISTS claude_sources (
    source_key TEXT PRIMARY KEY,
    source_path TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    title TEXT,
    project TEXT,
    repository TEXT
);
CREATE INDEX IF NOT EXISTS claude_sources_conversation ON claude_sources(conversation_id, source_key);
CREATE TABLE IF NOT EXISTS claude_source_messages (
    attachments TEXT NOT NULL DEFAULT '[]',
    source_key TEXT NOT NULL REFERENCES claude_sources(source_key) ON DELETE CASCADE,
    message_id TEXT NOT NULL,
    parent_id TEXT,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    projection TEXT,
    created_at INTEGER,
    PRIMARY KEY(source_key, message_id)
) WITHOUT ROWID;
-- Gold readiness/invalidation metadata; silver commits invalidate this state.
CREATE TABLE IF NOT EXISTS attachment_policy (
    singleton INTEGER PRIMARY KEY CHECK (singleton=1),
    fingerprint TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS derived_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    search_dirty INTEGER NOT NULL CHECK (search_dirty IN (0, 1)),
    semantic_ready_generation TEXT,
    resume_generation TEXT,
    resume_silver_revision INTEGER,
    resume_interpretation_version INTEGER
);
INSERT OR IGNORE INTO derived_state(singleton, search_dirty) VALUES (1, 1);
