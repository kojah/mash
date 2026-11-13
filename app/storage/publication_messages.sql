-- Journal the messages a publication delta changed, so a few new messages in a
-- long chat amend the published chat instead of replacing it. A chat stays a
-- whole replacement when it was created, removed or tombstoned, when fields
-- its search documents carry changed, or when it was journaled before this
-- schema (the column default).
ALTER TABLE shared_changes ADD COLUMN whole INTEGER NOT NULL DEFAULT 1 CHECK (whole IN (0, 1));
CREATE TABLE IF NOT EXISTS shared_message_changes (
    chat_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    PRIMARY KEY (chat_id, message_id)
) WITHOUT ROWID;
