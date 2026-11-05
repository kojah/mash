-- The one silver schema: local nodes and the shared server use it unchanged.
-- Silver is canonical history and the evidence needed to keep it current; it
-- holds no retrieval artifacts. Some tables are populated by only one side:
-- ingestion and publication state locally, shared_head on the server.

-- Agent chats and their canonical messages use separate identity namespaces.
CREATE TABLE IF NOT EXISTS chats (
    id TEXT PRIMARY KEY NOT NULL CHECK (id <> ''),
    provider TEXT NOT NULL CHECK (provider IN ('claude-code','codex','opencode','copilot','cursor','antigravity')),
    title TEXT,
    project TEXT,
    repository TEXT,
    created_at INTEGER,
    updated_at INTEGER,
    -- Empty on the server: published history never carries local paths.
    source_path TEXT NOT NULL DEFAULT '',
    source_fingerprint TEXT NOT NULL DEFAULT '',
    source_key TEXT,
    source_session TEXT,
    chat_fingerprint TEXT,
    UNIQUE (provider, source_key)
);
CREATE TABLE IF NOT EXISTS turns (
    id TEXT PRIMARY KEY NOT NULL CHECK (id <> ''),
    chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    revision TEXT NOT NULL,
    boundary TEXT NOT NULL CHECK (boundary IN ('start', 'user_reply', 'branch', 'orphan')),
    closed INTEGER NOT NULL CHECK (closed IN (0, 1)),
    created_at INTEGER,
    updated_at INTEGER
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS turns_chat ON turns(chat_id);
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY NOT NULL CHECK (id <> ''),
    chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    role TEXT NOT NULL,
    -- NULL means linear; empty means a tree root; otherwise a stable message ID.
    context_parent TEXT CHECK (context_parent IS NULL OR context_parent <> id),
    -- Derived grouping never owns canonical messages.
    turn_id TEXT REFERENCES turns(id) ON DELETE SET NULL,
    content_packed NOT NULL,
    content TEXT NOT NULL GENERATED ALWAYS AS (silver_text(content_packed)) VIRTUAL,
    search_projection TEXT,
    created_at INTEGER,
    -- Structured tool calls and results.
    metadata TEXT,
    attachments TEXT NOT NULL DEFAULT '[]',
    fingerprint TEXT NOT NULL DEFAULT ''
);
CREATE UNIQUE INDEX IF NOT EXISTS messages_chat ON messages(chat_id, ordinal);
CREATE INDEX IF NOT EXISTS messages_turn ON messages(turn_id) WHERE turn_id IS NOT NULL;
CREATE TABLE IF NOT EXISTS chat_enrichment (
    chat_id TEXT PRIMARY KEY REFERENCES chats(id) ON DELETE CASCADE,
    evidence TEXT NOT NULL
) WITHOUT ROWID;
-- Deletion intent.
CREATE TABLE IF NOT EXISTS tombstones (
    provider TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    forgotten_at INTEGER NOT NULL,
    PRIMARY KEY (provider, chat_id)
);
-- Provider replay checkpoints; local only.
CREATE TABLE IF NOT EXISTS source_checkpoints (
    provider TEXT NOT NULL,
    source_key TEXT NOT NULL,
    source_path TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    modified_ns INTEGER NOT NULL,
    fingerprint TEXT NOT NULL DEFAULT '',
    interpretation_version INTEGER NOT NULL DEFAULT 0 CHECK (interpretation_version >= 0),
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
    chat_id TEXT NOT NULL,
    title TEXT,
    project TEXT,
    repository TEXT
);
CREATE INDEX IF NOT EXISTS claude_sources_chat ON claude_sources(chat_id, source_key);
CREATE TABLE IF NOT EXISTS claude_source_messages (
    attachments TEXT NOT NULL DEFAULT '[]',
    -- The contribution's structured tool activity, as on its canonical message.
    metadata TEXT,

    source_key TEXT NOT NULL REFERENCES claude_sources(source_key) ON DELETE CASCADE,
    message_id TEXT NOT NULL,
    parent_id TEXT,
    role TEXT NOT NULL,
    content_packed NOT NULL,
    content TEXT NOT NULL GENERATED ALWAYS AS (silver_text(content_packed)) VIRTUAL,
    projection TEXT,
    created_at INTEGER,
    PRIMARY KEY(source_key, message_id)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS attachment_policy (
    singleton INTEGER PRIMARY KEY CHECK (singleton=1),
    fingerprint TEXT NOT NULL
);
-- Physical database identity is deliberately excluded from portable archives.
-- Import creates a new identity even when retaining node publication identity.
CREATE TABLE IF NOT EXISTS local_database_identity (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    instance TEXT NOT NULL
);
INSERT OR IGNORE INTO local_database_identity(singleton, instance)
VALUES (1, lower(hex(randomblob(16))));
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
CREATE TABLE IF NOT EXISTS shared_changes (
    chat_id TEXT PRIMARY KEY,
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
