-- Older records did not retain these values. NULL means unknown; do not infer
-- a historical endorsement or role from a source that may since have changed.
ALTER TABLE record_evidence ADD COLUMN purpose TEXT
    CHECK (purpose IN ('context', 'endorsement', 'report'));
ALTER TABLE record_evidence ADD COLUMN source_role TEXT;
