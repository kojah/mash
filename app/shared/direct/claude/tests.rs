use super::*;
use crate::ingestion::{self, IndexOptions, SourceIdentity};

struct Fixture {
    directory: tempfile::TempDir,
    database: std::path::PathBuf,
    root: std::path::PathBuf,
    options: IndexOptions,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("sources");
        std::fs::create_dir(&root).unwrap();
        let database = directory.path().join("index.sqlite3");
        let options = IndexOptions {
            providers: [(Provider::ClaudeCode, vec![root.clone()])].into(),
            roots_are_authoritative: true,
            since_days: None,
        };
        let fixture = Self {
            directory,
            database,
            root,
            options,
        };
        fixture.write("a", false);
        fixture.write("b", true);
        fixture
    }

    fn write(&self, file: &str, tail: bool) {
        let mut rows = vec![serde_json::json!({"type":"user","sessionId":"thread","uuid":"common","parentUuid":null,"message":{"role":"user","content":"common sentinel"}}).to_string()];
        if tail {
            rows.push(serde_json::json!({"type":"user","sessionId":"thread","uuid":"tail","parentUuid":"common","message":{"role":"user","content":"tail sentinel"}}).to_string());
        }
        std::fs::write(self.root.join(format!("{file}.jsonl")), rows.join("\n")).unwrap();
    }

    fn refresh(&self) -> i64 {
        let mut storage = Storage::open_writer(&self.database).unwrap();
        ingestion::index(&mut storage, &self.options).unwrap();
        storage.commit_writer().unwrap();
        storage.direct_owner("fixture").unwrap().revision
    }

    fn scan(&self, days: Option<u32>) -> DirectScan {
        let options = IndexOptions {
            since_days: days,
            ..self.options.clone()
        };
        ingestion::direct_scans(&options).unwrap().remove(0)
    }

    fn messages(&self, plan: &Plan) -> Vec<String> {
        let storage = Storage::open_existing(&self.database).unwrap();
        let mut messages = Vec::new();
        storage
            .with_publication_snapshot(|snapshot| {
                messages = snapshot
                    .claude_thread("thread", &plan.selected)?
                    .unwrap()
                    .messages
                    .inline()
                    .to_vec()
                    .into_iter()
                    .map(|message| message.content)
                    .collect();
                Ok(())
            })
            .unwrap();
        messages
    }
}

#[test]
fn routed_claude_plan_reconciles_changed_deleted_and_checkpointed_copies() {
    let fixture = Fixture::new();
    let revision = fixture.refresh();
    let scan = fixture.scan(None);
    let first = plan(&fixture.database, revision, &scan, &[], false).unwrap();
    assert_eq!(first.dirty, ["thread".to_owned()].into());
    assert_eq!(fixture.messages(&first).len(), 2);
    let unchanged = plan(
        &fixture.database,
        revision,
        &scan,
        &first.checkpoints,
        false,
    )
    .unwrap();
    assert!(unchanged.dirty.is_empty());
    assert_eq!(unchanged.skipped, 2);
    std::fs::remove_file(fixture.root.join("b.jsonl")).unwrap();
    let revision = fixture.refresh();
    let scan = fixture.scan(None);
    let deleted = plan(
        &fixture.database,
        revision,
        &scan,
        &first.checkpoints,
        false,
    )
    .unwrap();
    assert_eq!(deleted.dirty, ["thread".to_owned()].into());
    assert_eq!(fixture.messages(&deleted), ["common sentinel"]);
}

#[test]
fn routed_claude_plan_rejects_stale_database_revision_and_source_content() {
    let fixture = Fixture::new();
    let revision = fixture.refresh();
    let scan = fixture.scan(None);
    assert!(plan(&fixture.database, revision - 1, &scan, &[], false).is_err());
    fixture.write("a", true);
    assert_eq!(
        plan(&fixture.database, revision, &scan, &[], false)
            .err()
            .unwrap()
            .error
            .kind,
        crate::ErrorKind::SourceChanged
    );
}

#[test]
fn routed_claude_horizon_omits_new_old_sources_but_retains_published_contributions() {
    let fixture = Fixture::new();
    let path = fixture.root.join("b.jsonl");
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000)),
        )
        .unwrap();
    let revision = fixture.refresh();
    let scan = fixture.scan(Some(90));
    let first = plan(&fixture.database, revision, &scan, &[], false).unwrap();
    assert_eq!(fixture.messages(&first), ["common sentinel"]);
    let full = plan(&fixture.database, revision, &fixture.scan(None), &[], false).unwrap();
    let retained = plan(&fixture.database, revision, &scan, &full.checkpoints, false).unwrap();
    assert_eq!(fixture.messages(&retained).len(), 2);
    let old_hash = SourceIdentity::file(path).publication_hash();
    assert!(
        !retained
            .checkpoints
            .iter()
            .any(|source| source.path_hash == old_hash)
    );
}

#[test]
fn routed_claude_legacy_checkpoints_require_explicit_complete_replay() {
    let fixture = Fixture::new();
    let revision = fixture.refresh();
    let mut previous = plan(&fixture.database, revision, &fixture.scan(None), &[], false)
        .unwrap()
        .checkpoints;
    for source in &mut previous {
        source.fingerprint.clear();
    }
    assert!(
        plan(
            &fixture.database,
            revision,
            &fixture.scan(Some(90)),
            &previous,
            false
        )
        .is_err()
    );
    assert!(
        plan(
            &fixture.database,
            revision,
            &fixture.scan(None),
            &previous,
            false
        )
        .is_ok()
    );
    assert!(fixture.directory.path().exists());
}
