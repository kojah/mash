-- Open questions are no longer a kind of record, and nothing resolves one.
-- SQLite cannot change the checks on records.kind and record_links.relation in
-- place, so these triggers refuse the retired values instead. Deleting a
-- record removes its revisions, evidence, confirmations and links with it.
DELETE FROM record_links WHERE relation = 'resolves';
DELETE FROM records WHERE kind = 'open_question';
CREATE TRIGGER records_retired_kind_insert BEFORE INSERT ON records
WHEN NEW.kind = 'open_question'
BEGIN SELECT RAISE(ABORT, 'open questions are no longer a kind of record'); END;
CREATE TRIGGER records_retired_kind_update BEFORE UPDATE OF kind ON records
WHEN NEW.kind = 'open_question'
BEGIN SELECT RAISE(ABORT, 'open questions are no longer a kind of record'); END;
CREATE TRIGGER record_links_retired_relation_insert BEFORE INSERT ON record_links
WHEN NEW.relation = 'resolves'
BEGIN SELECT RAISE(ABORT, 'resolution links are no longer kept'); END;
CREATE TRIGGER record_links_retired_relation_update BEFORE UPDATE OF relation ON record_links
WHEN NEW.relation = 'resolves'
BEGIN SELECT RAISE(ABORT, 'resolution links are no longer kept'); END;
