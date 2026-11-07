-- Native interpretations of agent history. These tables deliberately do not
-- participate in silver_state: changing a record does not change chat vectors.
CREATE TABLE record_owner (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    id TEXT NOT NULL CHECK (id <> '')
);
INSERT INTO record_owner VALUES (1, lower(hex(randomblob(16))));
CREATE TABLE records (
    id TEXT PRIMARY KEY NOT NULL CHECK (id <> ''),
    owner TEXT NOT NULL CHECK (owner <> ''),
    kind TEXT NOT NULL CHECK (kind IN ('decision','preference','fact','open_question')),
    scope TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    forgotten INTEGER NOT NULL DEFAULT 0 CHECK (forgotten IN (0,1))
) WITHOUT ROWID;
CREATE INDEX records_scope ON records(owner, scope, kind);
CREATE TABLE record_revisions (
    record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision > 0),
    -- Typed revision document, including per-kind lifecycle and attribution.
    body TEXT NOT NULL,
    recorded_at INTEGER NOT NULL,
    PRIMARY KEY(record_id, revision)
) WITHOUT ROWID;
CREATE TABLE record_evidence (
    record_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    -- No message FK: ordinary source replacement must preserve stale evidence.
    chat_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    quote TEXT NOT NULL CHECK (quote <> ''),
    PRIMARY KEY(record_id, revision, position),
    FOREIGN KEY(record_id, revision) REFERENCES record_revisions(record_id, revision) ON DELETE CASCADE
) WITHOUT ROWID;
CREATE INDEX record_evidence_chat ON record_evidence(chat_id, record_id);
CREATE TABLE record_links (
    source_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    target_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
    relation TEXT NOT NULL CHECK (relation IN ('supersedes','resolves','supports','contradicts')),
    CHECK (source_id <> target_id),
    PRIMARY KEY(source_id, target_id, relation)
) WITHOUT ROWID;
CREATE UNIQUE INDEX record_single_successor ON record_links(target_id) WHERE relation='supersedes';
CREATE TABLE record_confirmations (
    record_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    message_id TEXT NOT NULL,
    confirmed_at INTEGER NOT NULL,
    PRIMARY KEY(record_id, revision, message_id),
    FOREIGN KEY(record_id, revision) REFERENCES record_revisions(record_id, revision) ON DELETE CASCADE
) WITHOUT ROWID;
CREATE TABLE record_operations (
    operation_id TEXT PRIMARY KEY NOT NULL,
    request_digest TEXT NOT NULL,
    -- Receipt contains only record IDs/revisions; never copied record text.
    result TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE record_checkpoints (
    scope TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    source_fingerprint TEXT NOT NULL,
    PRIMARY KEY(scope, chat_id)
) WITHOUT ROWID;
CREATE TABLE record_changes (
    record_id TEXT PRIMARY KEY NOT NULL,
    revision INTEGER NOT NULL
) WITHOUT ROWID;
