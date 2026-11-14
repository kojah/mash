-- Owner-local batch receipts contain only fingerprints and record identities.
CREATE TABLE record_extractions (
    operation_id TEXT PRIMARY KEY NOT NULL,
    request_digest TEXT NOT NULL,
    result TEXT NOT NULL
) WITHOUT ROWID;
