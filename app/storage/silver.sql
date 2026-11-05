-- The one silver schema: local nodes and the shared server use it unchanged.
-- Silver is canonical history and the evidence needed to keep it current; it
-- holds no retrieval artifacts. Some tables are populated by only one side:
-- ingestion and publication state locally, shared_head on the server.

-- Canonical history for every source, native providers and connectors alike.
-- An object has a core role (container, segment, entry, document), its source's
-- kind label and a parent. Native-only columns are NULL for connector objects.
CREATE TABLE IF NOT EXISTS objects (
    -- Internal relational address; never exported.
    storage_id INTEGER PRIMARY KEY AUTOINCREMENT,
    origin TEXT NOT NULL CHECK (origin IN ('provider', 'connector')),
    -- The provider name, or the connector's source ID.
    source TEXT NOT NULL,
    id TEXT NOT NULL,
    object_role TEXT NOT NULL CHECK (object_role IN ('container', 'segment', 'entry', 'document')),
    kind TEXT NOT NULL,
    -- The parent's own ID, as submitted or parsed.
    parent_id TEXT,
    -- The same edge by storage address: native chat and message IDs are
    -- separate namespaces, so the logical ID alone cannot be a key.
    parent_storage_id INTEGER REFERENCES objects(storage_id) ON DELETE CASCADE,
    reply_to TEXT,
    -- The nearest container: a container's or document's own ID, or the
    -- container that holds an entry or segment. Results group by it.
    conversation_id TEXT NOT NULL,
    -- Native order and branch tree: NULL for linear threads, -1 for a tree
    -- root, otherwise the parent ordinal. NULL for connector objects.
    ordinal INTEGER,
    context_parent INTEGER CHECK (context_parent IS NULL OR (context_parent >= -1 AND context_parent < ordinal)),
    -- Native dialogue role, or a connector entry's author as JSON.
    role TEXT,
    author TEXT,
    title TEXT,
    -- Large text is stored zstd-packed (see app/storage/packed.rs); `content`
    -- decodes it, so readers, predicates and indexes see plain text.
    content_packed,
    content TEXT GENERATED ALWAYS AS (silver_text(content_packed)) VIRTUAL,
    -- Silver's interpreted visible dialogue; the Rust model calls this dialogue_text.
    search_projection TEXT,
    created_at INTEGER,
    updated_at INTEGER,
    project TEXT,
    repository TEXT,
    url TEXT,
    -- Connector metadata as submitted; a native turn's metadata; a native
    -- entry's structured tool calls and results (NULL when it has none).
    metadata TEXT,
    attachments TEXT NOT NULL DEFAULT '[]',
    -- A connector object's exact submitted record, kept for receipts and replay.
    record TEXT
);
-- A connector source's IDs span all of its roles.
CREATE UNIQUE INDEX IF NOT EXISTS objects_connector_id ON objects(source, id) WHERE origin = 'connector';
-- Native chat and message IDs are each global, as retrieval views and
-- enrichment address them alone, but a message may reuse a chat's ID.
CREATE UNIQUE INDEX IF NOT EXISTS objects_native_container ON objects(id)
    WHERE origin = 'provider' AND object_role = 'container';
CREATE UNIQUE INDEX IF NOT EXISTS objects_native_id ON objects(id)
    WHERE origin = 'provider' AND object_role <> 'container';
CREATE UNIQUE INDEX IF NOT EXISTS objects_native_order ON objects(conversation_id, ordinal)
    WHERE origin = 'provider' AND ordinal IS NOT NULL;
-- Lookups by ID; the partial unique indexes above only serve conflicts.
CREATE INDEX IF NOT EXISTS objects_id ON objects(id);
CREATE INDEX IF NOT EXISTS objects_parent ON objects(origin, source, parent_id);
CREATE INDEX IF NOT EXISTS objects_parent_storage ON objects(parent_storage_id);
CREATE INDEX IF NOT EXISTS objects_reply ON objects(origin, source, reply_to);
CREATE INDEX IF NOT EXISTS objects_conversation ON objects(conversation_id, object_role, ordinal);
-- Native ingestion and publication bookkeeping for each native chat.
CREATE TABLE IF NOT EXISTS native_chats (
    storage_id INTEGER PRIMARY KEY REFERENCES objects(storage_id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL UNIQUE,
    provider TEXT NOT NULL CHECK (
        provider IN (
            'claude-code', 'codex', 'opencode', 'copilot', 'cursor', 'antigravity'
        )
    ),
    -- Empty on the server: published history never carries local paths.
    source_path TEXT NOT NULL,
    source_fingerprint TEXT NOT NULL DEFAULT '',
    -- Provider-native identity; NULL on the server.
    source_key TEXT,
    source_session TEXT,
    -- Ordered digest of the thread's published silver; set by the server.
    thread_fingerprint TEXT,
    UNIQUE (provider, source_key)
);
-- Native message change detection; not published.
CREATE TABLE IF NOT EXISTS native_messages (
    storage_id INTEGER PRIMARY KEY REFERENCES objects(storage_id) ON DELETE CASCADE,
    id TEXT NOT NULL UNIQUE,
    fingerprint TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS conversation_enrichment (
    conversation_id TEXT PRIMARY KEY REFERENCES native_chats(conversation_id) ON DELETE CASCADE,
    evidence TEXT NOT NULL
) WITHOUT ROWID;
-- Deletion intent.
CREATE TABLE IF NOT EXISTS tombstones (
    provider TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    forgotten_at INTEGER NOT NULL,
    PRIMARY KEY (provider, conversation_id)
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

-- Source-neutral canonical records submitted by out-of-process connectors.
-- JSON columns decode into the Rust Object model; they preserve opaque metadata
-- and attachments without inventing agent roles.
CREATE TABLE IF NOT EXISTS connector_sources (
    id TEXT PRIMARY KEY,
    revision TEXT NOT NULL
) WITHOUT ROWID;
-- When each delivery mode last committed for a source, in Unix milliseconds,
-- and the modes its connector declares. Delivery metadata rather than
-- history: it is not exported and changes no silver revision.
CREATE TABLE IF NOT EXISTS connector_activity (
    source_id TEXT PRIMARY KEY REFERENCES connector_sources(id),
    modes TEXT NOT NULL DEFAULT '[]',
    snapshot_at INTEGER,
    complete_snapshot_at INTEGER,
    live_at INTEGER,
    change_at INTEGER
) WITHOUT ROWID;
-- A source's declared topology; its objects must conform once it exists.
CREATE TABLE IF NOT EXISTS connector_schemas (
    source_id TEXT PRIMARY KEY REFERENCES connector_sources(id),
    schema TEXT NOT NULL
) WITHOUT ROWID;
-- Receipts are committed with the mutations they acknowledge. A replayed
-- batch cannot apply again, nor reuse its identity for different contents.
CREATE TABLE IF NOT EXISTS connector_receipts (
    source_id TEXT NOT NULL REFERENCES connector_sources(id),
    batch_id TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    receipt TEXT NOT NULL,
    PRIMARY KEY (source_id, batch_id)
) WITHOUT ROWID;
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
    -- The contribution's structured tool activity, as on its canonical entry.
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
