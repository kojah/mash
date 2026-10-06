-- Shared silver: published node history. No gold (vectors, documents, readiness).
CREATE TABLE IF NOT EXISTS nodes (
    group_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    head TEXT NOT NULL,
    PRIMARY KEY (group_id, node_id)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS conversations (
    group_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    id TEXT NOT NULL,
    provider TEXT NOT NULL,
    record TEXT NOT NULL,
    -- Ordered dependency digest of this thread's silver; never a gold identity.
    fingerprint TEXT NOT NULL,
    PRIMARY KEY (group_id, node_id, id)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS messages (
    group_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    parent INTEGER,
    searchable INTEGER NOT NULL,
    user INTEGER NOT NULL,
    record TEXT NOT NULL,
    PRIMARY KEY (group_id, node_id, id),
    UNIQUE (group_id, node_id, conversation_id, ordinal)
);
CREATE INDEX IF NOT EXISTS messages_parent
    ON messages(group_id, node_id, conversation_id, parent) WHERE parent IS NOT NULL;
CREATE TABLE IF NOT EXISTS sources (
    group_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    key TEXT NOT NULL,
    provider TEXT NOT NULL,
    conversation_id TEXT,
    interpretation_version INTEGER NOT NULL,
    record TEXT NOT NULL,
    PRIMARY KEY (group_id, node_id, key)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS sources_conversation
    ON sources(group_id, node_id, conversation_id) WHERE conversation_id IS NOT NULL;
CREATE TABLE IF NOT EXISTS tombstones (
    group_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    key TEXT NOT NULL,
    record TEXT NOT NULL,
    PRIMARY KEY (group_id, node_id, key)
) WITHOUT ROWID;
