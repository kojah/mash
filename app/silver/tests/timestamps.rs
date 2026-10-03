//! Shared replay contract for native timestamp and metadata evidence.
use super::*;
use crate::gold::config::{Build, Representations};
use crate::silver::enrichment::{Evidence, Origin, TimestampOrigin};

fn replay_defects(expected: &Evidence) -> Vec<Evidence> {
    let times = expected.timestamps.as_ref().unwrap();
    let mut defects = [
        None,
        Some(crate::silver::enrichment::Timestamps {
            created_at: times.created_at.as_ref().map(|_| TimestampOrigin::Unknown),
            updated_at: times.updated_at.as_ref().map(|_| TimestampOrigin::Unknown),
        }),
        Some(crate::silver::enrichment::Timestamps {
            created_at: None,
            updated_at: None,
        }),
    ]
    .into_iter()
    .map(|timestamps| {
        let mut legacy = expected.clone();
        legacy.timestamps = timestamps;
        legacy
    })
    .collect::<Vec<_>>();
    for field in [
        OriginField::Title,
        OriginField::Project,
        OriginField::Repository,
    ] {
        let mut legacy = expected.clone();
        let origin = match field {
            OriginField::Title => &mut legacy.title,
            OriginField::Project => &mut legacy.project,
            OriginField::Repository => &mut legacy.repository,
        };
        *origin = origin.is_none().then_some(Origin::Unknown);
        defects.push(legacy);
    }
    defects
}

enum OriginField {
    Title,
    Project,
    Repository,
}

pub(in crate::silver) fn assert_replay(
    provider: Provider,
    source: &SourceIdentity,
    options: &IndexOptions,
) {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("silver.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    let initial = index(&mut storage, options).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let parsed = provider
        .parser()
        .parse(&HistorySource {
            identity: source.clone(),
            activity_ns: None,
            estimated_bytes: None,
            dependencies: vec![],
        })
        .unwrap()
        .conversation
        .unwrap();
    let expected = Evidence::capture(&parsed).unwrap();
    let stamp = source_stamp_for(source).unwrap();
    let before = fs::read(&source.path).unwrap();
    for legacy in replay_defects(&expected) {
        assert!(legacy.is_current());
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute(
                "UPDATE conversation_enrichment SET evidence=?1 WHERE conversation_id=?2",
                rusqlite::params![serde_json::to_string(&legacy).unwrap(), parsed.id],
            )
            .unwrap();
        let mut storage = Storage::open_writer(&database).unwrap();
        let certificate = storage.silver_certificate().unwrap();
        assert!(
            !storage
                .source_checkpoint_matches(provider, source, &stamp)
                .unwrap()
        );
        let withheld = source.path.with_extension("withheld");
        fs::rename(&source.path, &withheld).unwrap();
        let result = crate::gold::local::build(
            &mut storage,
            &crate::semantic::Models::new(directory.path().join("missing-models")),
            &initial,
            &Build {
                representations: Representations::LexicalOnly,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(result.ready);
        assert_eq!(result.embeddings.model_inferences, 0);
        assert_eq!(storage.silver_certificate().unwrap(), certificate);
        assert_eq!(
            storage.conversation_enrichment(&parsed.id).unwrap(),
            Some(legacy.clone())
        );
        storage.commit_writer().unwrap();
        drop(storage);
        let stored = Storage::open_catalog(&database).unwrap();
        stored
            .with_publication_snapshot(|snapshot| {
                snapshot.visit(crate::shared::protocol::BatchKind::Conversations, |batch| {
                    let crate::shared::protocol::Batch::Conversations { records } = batch else {
                        panic!("conversation batch")
                    };
                    let row = records.iter().find(|row| row.id == parsed.id).unwrap();
                    assert_eq!(row.enrichment.as_ref(), Some(&legacy));
                    Ok(())
                })
            })
            .unwrap();
        drop(stored);
        fs::rename(withheld, &source.path).unwrap();
        assert_eq!(source_stamp_for(source).unwrap(), stamp);
        let mut storage = Storage::open_writer(&database).unwrap();
        let replay = index(&mut storage, options).unwrap();
        assert_eq!(replay.changed_messages, 0);
        assert_eq!(
            storage.conversation_enrichment(&parsed.id).unwrap(),
            Some(expected.clone())
        );
        assert!(storage.silver_certificate().unwrap().revision > certificate.revision);
        storage.commit_writer().unwrap();
        assert_eq!(fs::read(&source.path).unwrap(), before);
    }
}
