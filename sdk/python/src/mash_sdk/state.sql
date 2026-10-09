-- SDK delivery bookkeeping, not canonical Mash silver or a server outbox.
CREATE TABLE scopes (
    destination_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    revision TEXT NOT NULL,
    cursor BLOB,
    last_receipt BLOB,
    PRIMARY KEY (destination_id, source_id)
);
CREATE TABLE pending (
    destination_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    expected_revision TEXT NOT NULL,
    payload BLOB NOT NULL,
    next_cursor BLOB,
    -- Snapshot scope for each upserted document, recorded in `records` on ACK.
    document_scopes BLOB,
    PRIMARY KEY (destination_id, source_id),
    FOREIGN KEY (destination_id, source_id) REFERENCES scopes(destination_id, source_id)
);
-- Records the destination acknowledged, so unchanged upserts can be skipped
-- and complete snapshots can name what they omit. Never canonical history.
CREATE TABLE records (
    destination_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('container', 'segment', 'entry', 'document')),
    fingerprint BLOB NOT NULL,
    parent_id TEXT,
    created_at INTEGER,
    scope BLOB,
    PRIMARY KEY (destination_id, source_id, id)
);
CREATE INDEX records_parent ON records(destination_id, source_id, parent_id);
-- Captured source events awaiting debounce and refresh, coalesced per key.
CREATE TABLE events (
    destination_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    key TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    payload BLOB NOT NULL,
    first_seen REAL NOT NULL,
    last_seen REAL NOT NULL,
    PRIMARY KEY (destination_id, source_id, key)
);
