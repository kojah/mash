-- Local views: retrieval artifacts derived from the paired silver database.
-- Every row can be rebuilt from silver with `mash index --rebuild-local-views --full`.
-- Statements stay unqualified; storage attaches this file as schema `views`.
CREATE TABLE IF NOT EXISTS silver_pairing (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    instance TEXT NOT NULL
);
-- View build identity: exactly one active projection, without copied history.
CREATE TABLE IF NOT EXISTS views_manifest (
    singleton INTEGER PRIMARY KEY CHECK (singleton=1),
    manifest TEXT
);
INSERT OR IGNORE INTO views_manifest(singleton) VALUES (1);
-- Independent local lexical publication; contains no copied canonical text.
CREATE TABLE IF NOT EXISTS lexical_manifest (
    singleton INTEGER PRIMARY KEY CHECK (singleton=1),
    manifest TEXT
);
INSERT OR IGNORE INTO lexical_manifest(singleton) VALUES (1);
-- Readiness and invalidation; silver commits invalidate this state.
CREATE TABLE IF NOT EXISTS derived_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    search_dirty INTEGER NOT NULL CHECK (search_dirty IN (0, 1)),
    semantic_ready_generation TEXT,
    resume_generation TEXT,
    resume_silver_revision INTEGER,
    resume_interpretation_version INTEGER
);
INSERT OR IGNORE INTO derived_state(singleton, search_dirty) VALUES (1, 1);
-- Message rows live in silver; deleting them removes these rows by trigger.
CREATE TABLE IF NOT EXISTS message_embeddings (
    message_id TEXT PRIMARY KEY,
    generation TEXT NOT NULL DEFAULT '',
    dimensions INTEGER NOT NULL,
    norm REAL NOT NULL,
    vector BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS pending_embeddings (
    message_id TEXT PRIMARY KEY
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
-- Searchable units projected from connector objects: entries and documents.
-- Connector objects have no native message rowid; the anchor namespaces their
-- source and role, and the digest decides when text must be re-indexed.
CREATE TABLE IF NOT EXISTS connector_units (
    id INTEGER PRIMARY KEY,
    anchor TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL CHECK (role IN ('entry', 'document')),
    -- The source's kind label, such as discord:message.
    label TEXT NOT NULL,
    source_id TEXT NOT NULL,
    record_id TEXT NOT NULL,
    -- An entry's nearest container, or a document's own anchor.
    group_anchor TEXT NOT NULL,
    created_at INTEGER,
    digest BLOB NOT NULL
);
-- Rowid is connector_units.id.
CREATE VIRTUAL TABLE IF NOT EXISTS connector_fts USING fts5(
    content,
    tokenize = 'unicode61'
);
CREATE TABLE IF NOT EXISTS connector_embeddings (
    unit_id INTEGER PRIMARY KEY,
    generation TEXT NOT NULL,
    dimensions INTEGER NOT NULL CHECK (dimensions > 0),
    norm REAL NOT NULL,
    vector BLOB NOT NULL
);
