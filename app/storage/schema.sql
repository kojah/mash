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
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL CHECK (
        provider IN (
            'claude-code', 'codex', 'aider', 'hermes', 'opencode'
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
    storage_id INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    search_projection TEXT,
    created_at INTEGER,
    fingerprint TEXT NOT NULL DEFAULT '',
    UNIQUE (conversation_id, ordinal)
);
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
    PRIMARY KEY (provider, source_key)
);
CREATE TABLE IF NOT EXISTS derived_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    search_dirty INTEGER NOT NULL CHECK (search_dirty IN (0, 1)),
    semantic_ready_generation TEXT
);
INSERT OR IGNORE INTO derived_state(singleton, search_dirty) VALUES (1, 1);
