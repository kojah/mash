use super::*;
use crate::{
    ingestion::{IndexOptions, direct_scans, index},
    storage::Storage,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use prost::Message;
use rusqlite::{Connection, params};
use serde::Deserialize;
use std::path::PathBuf;

mod bounds;
mod history;
mod mutations;
mod retrieval;

#[derive(Deserialize)]
struct Blob {
    base64: String,
}
impl Blob {
    fn bytes(&self) -> Vec<u8> {
        STANDARD.decode(&self.base64).unwrap()
    }
}
#[derive(Deserialize)]
struct RootRow {
    trajectory_id: String,
    cascade_id: String,
    trajectory_type: i32,
    source: i32,
}
#[derive(Deserialize)]
struct MetadataRow {
    id: String,
    data: Blob,
}
#[derive(Deserialize)]
struct StepRow {
    idx: i64,
    step_type: i32,
    status: i32,
    step_format: i32,
    metadata: Option<Blob>,
    step_payload: Blob,
}
#[derive(Deserialize)]
struct Header {
    idx: i64,
    step_type: i32,
    status: i32,
    step_format: i32,
}
#[derive(Deserialize)]
struct Case {
    trajectory_meta: Vec<RootRow>,
    trajectory_metadata_blob: Vec<MetadataRow>,
    dialogue_steps: Vec<StepRow>,
    step_headers: Vec<Header>,
}
#[derive(Deserialize)]
struct Fixture {
    root_before_mutation: Case,
    fork: Case,
}
#[derive(Deserialize)]
struct Schema {
    create_sql: String,
}
#[derive(Deserialize)]
struct Tables {
    trajectory_meta: Schema,
    trajectory_metadata_blob: Schema,
    steps: Schema,
}
#[derive(Deserialize)]
struct Schemas {
    native_tables: Tables,
}
fn fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "../../../docs/fixtures/antigravity-native-mutations-2.5.5.json"
    ))
    .unwrap()
}
pub(crate) fn write(directory: &Path) -> PathBuf {
    write_case(directory, &fixture().root_before_mutation)
}
fn write_case(directory: &Path, case: &Case) -> PathBuf {
    let path = directory.join(format!("{}.db", case.trajectory_meta[0].cascade_id));
    let schemas: Schemas = serde_json::from_str(include_str!(
        "../../../docs/fixtures/antigravity-native-runtime-2.5.5.json"
    ))
    .unwrap();
    let connection = Connection::open(&path).unwrap();
    for schema in [
        schemas.native_tables.trajectory_meta,
        schemas.native_tables.trajectory_metadata_blob,
        schemas.native_tables.steps,
    ] {
        connection.execute_batch(&schema.create_sql).unwrap();
    }
    for row in &case.trajectory_meta {
        connection
            .execute(
                "INSERT INTO trajectory_meta VALUES(?1,?2,?3,?4)",
                params![
                    row.trajectory_id,
                    row.cascade_id,
                    row.trajectory_type,
                    row.source
                ],
            )
            .unwrap();
    }
    for row in &case.trajectory_metadata_blob {
        connection
            .execute(
                "INSERT INTO trajectory_metadata_blob VALUES(?1,?2)",
                params![row.id, row.data.bytes()],
            )
            .unwrap();
    }
    for header in &case.step_headers {
        // Exact native dialogue rows; unsupported tool bodies are synthetic
        // placeholders matching their captured headers, never search evidence.
        let row = case.dialogue_steps.iter().find(|r| r.idx == header.idx);
        let payload = row.map_or_else(
            || {
                proto::Step {
                    kind: header.step_type,
                    status: header.status,
                    ..Default::default()
                }
                .encode_to_vec()
            },
            |r| r.step_payload.bytes(),
        );
        let metadata = row.and_then(|r| r.metadata.as_ref().map(Blob::bytes));
        connection.execute("INSERT INTO steps(idx,step_type,status,step_format,metadata,step_payload) VALUES(?1,?2,?3,?4,?5,?6)", params![header.idx,header.step_type,header.status,header.step_format,metadata,payload]).unwrap();
        if let Some(row) = row {
            assert_eq!(
                (row.step_type, row.status, row.step_format),
                (header.step_type, header.status, header.step_format)
            );
        }
    }
    path
}
fn source(path: &Path) -> SourceIdentity {
    discover(path).unwrap().remove(0).identity
}
fn conversation(path: &Path) -> Conversation {
    parse(&source(path)).unwrap().conversation.unwrap()
}
fn options(path: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::Antigravity, vec![path.into()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    }
}

fn replace_step(path: &Path, index: i64, step: &proto::Step) {
    Connection::open(path)
        .unwrap()
        .execute(
            "UPDATE steps SET step_type=?1,status=?2,metadata=?3,step_payload=?4 WHERE idx=?5",
            params![
                step.kind,
                step.status,
                step.metadata.as_ref().map(Message::encode_to_vec),
                step.encode_to_vec(),
                index
            ],
        )
        .unwrap();
}
