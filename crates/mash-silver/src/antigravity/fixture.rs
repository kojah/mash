//! Writes an Antigravity conversation database from the recorded native
//! fixtures, for this crate's tests and those of the crates above it.
use super::proto;
use base64::{Engine, engine::general_purpose::STANDARD};
use prost::Message;
use rusqlite::{Connection, params};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
pub struct Blob {
    base64: String,
}
impl Blob {
    pub fn bytes(&self) -> Vec<u8> {
        STANDARD.decode(&self.base64).unwrap()
    }
}
#[derive(Deserialize)]
pub struct RootRow {
    pub trajectory_id: String,
    pub cascade_id: String,
    pub trajectory_type: i32,
    pub source: i32,
}
#[derive(Deserialize)]
pub struct MetadataRow {
    pub id: String,
    pub data: Blob,
}
#[derive(Deserialize)]
pub struct StepRow {
    pub idx: i64,
    pub step_type: i32,
    pub status: i32,
    pub step_format: i32,
    pub metadata: Option<Blob>,
    pub step_payload: Blob,
}
#[derive(Deserialize)]
pub struct Header {
    pub idx: i64,
    pub step_type: i32,
    pub status: i32,
    pub step_format: i32,
}
#[derive(Deserialize)]
pub struct Case {
    pub trajectory_meta: Vec<RootRow>,
    pub trajectory_metadata_blob: Vec<MetadataRow>,
    pub dialogue_steps: Vec<StepRow>,
    pub step_headers: Vec<Header>,
}
#[derive(Deserialize)]
pub struct Fixture {
    pub root_before_mutation: Case,
    pub fork: Case,
}
#[derive(Deserialize)]
pub struct Schema {
    pub create_sql: String,
}
#[derive(Deserialize)]
pub struct Tables {
    pub trajectory_meta: Schema,
    pub trajectory_metadata_blob: Schema,
    pub steps: Schema,
}
#[derive(Deserialize)]
pub struct Schemas {
    pub native_tables: Tables,
}
pub fn fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "../../../../fixtures/antigravity-native-mutations-2.5.5.json"
    ))
    .unwrap()
}
pub fn write(directory: &Path) -> PathBuf {
    write_case(directory, &fixture().root_before_mutation)
}
pub fn write_case(directory: &Path, case: &Case) -> PathBuf {
    let path = directory.join(format!("{}.db", case.trajectory_meta[0].cascade_id));
    let schemas: Schemas = serde_json::from_str(include_str!(
        "../../../../fixtures/antigravity-native-runtime-2.5.5.json"
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
