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
    PRIMARY KEY (destination_id, source_id),
    FOREIGN KEY (destination_id, source_id) REFERENCES scopes(destination_id, source_id)
);
