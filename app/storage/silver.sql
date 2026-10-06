-- The one silver schema: local nodes and the shared server use it unchanged.
-- Silver is canonical history and the evidence needed to keep it current; it
-- holds no retrieval artifacts. Some tables are populated by only one side:
-- ingestion and publication state locally, shared_head on the server.

-- Canonical history.
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL CHECK (
        provider IN (
            'claude-code', 'codex', 'opencode', 'copilot', 'cursor', 'antigravity'
        )
    ),
    -- Empty on the server: published history never carries local paths.
    source_path TEXT NOT NULL,
    title TEXT,
    created_at INTEGER,
    updated_at INTEGER,
    source_fingerprint TEXT NOT NULL DEFAULT '',
    project TEXT,
    repository TEXT,
    -- Provider-native identity; NULL on the server.
    source_key TEXT,
    source_session TEXT,
    -- Ordered digest of the thread's published silver; set by the server.
    thread_fingerprint TEXT,
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
    search_projection TEXT,
    created_at INTEGER,
    -- NULL for linear threads, -1 for a tree root, otherwise the parent ordinal.
    context_parent INTEGER CHECK (context_parent IS NULL OR (context_parent >= -1 AND context_parent < ordinal)),
    fingerprint TEXT NOT NULL DEFAULT '',
    UNIQUE (conversation_id, ordinal)
);
CREATE TABLE IF NOT EXISTS conversation_enrichment (
    conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    evidence TEXT NOT NULL
) WITHOUT ROWID;
-- Deletion intent.
CREATE TABLE IF NOT EXISTS tombstones (
    provider TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    forgotten_at INTEGER NOT NULL,
    PRIMARY KEY (provider, conversation_id)
);
-- Provider replay checkpoints. The server keys published checkpoints by path
-- hash, leaves source_path empty and records the owning thread and readiness.
CREATE TABLE IF NOT EXISTS source_checkpoints (
    provider TEXT NOT NULL,
    source_key TEXT NOT NULL,
    source_path TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    modified_ns INTEGER NOT NULL,
    fingerprint TEXT NOT NULL DEFAULT '',
    interpretation_version INTEGER NOT NULL DEFAULT 0 CHECK (interpretation_version >= 0),
    conversation_id TEXT,
    ready INTEGER NOT NULL DEFAULT 1 CHECK (ready IN (0, 1)),
    PRIMARY KEY (provider, source_key)
);
-- Ingestion evidence.
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
CREATE TABLE IF NOT EXISTS attachment_policy (
    singleton INTEGER PRIMARY KEY CHECK (singleton=1),
    fingerprint TEXT NOT NULL
);
-- Node-side publication journal.
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
CREATE TABLE IF NOT EXISTS shared_changes (
    conversation_id TEXT PRIMARY KEY,
    revision INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS shared_change_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    floor INTEGER NOT NULL
);
INSERT OR IGNORE INTO shared_change_state SELECT singleton, revision FROM shared_publication;
-- Server-side publication head for the node this silver belongs to.
CREATE TABLE IF NOT EXISTS shared_head (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    head TEXT NOT NULL
);
