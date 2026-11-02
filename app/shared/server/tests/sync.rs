//! Real authenticated WebSocket transport and durable resume without Qdrant.
use super::*;
use crate::shared::protocol::{Event, Start};
use crate::shared::sync::{
    catalog::{self, Catalog},
    wire,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::shared::sync::client::scoped_operation_id;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct SyncClient {
    sink: futures_util::stream::SplitSink<Socket, Message>,
    stream: futures_util::stream::SplitStream<Socket>,
}

impl SyncClient {
    async fn connect(service: &Service, token: &str, instance: &str) -> Self {
        let url = format!("{}/v1/sync", service.url.replacen("http://", "ws://", 1));
        let mut request = url.into_client_request().unwrap();
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        request
            .headers_mut()
            .insert("x-mash-group", service.first.group_id.parse().unwrap());
        request
            .headers_mut()
            .insert("x-mash-node", service.first.node_id.parse().unwrap());
        let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let (mut sink, mut stream) = socket.split();
        let hello = wire::ClientFrame {
            operation_id: String::new(),
            body: Some(wire::client_frame::Body::Hello(wire::Hello {
                protocol_version: wire::PROTOCOL_VERSION,
                database_instance: instance.into(),
                zstd_publication_data: true,
                max_decoded_bytes: wire::DEFAULT_MAX_DECODED_BYTES as u64,
            })),
        };
        sink.send(Message::Binary(wire::encode_client(&hello).unwrap().into()))
            .await
            .unwrap();
        let welcome = SocketReader::receive(&mut stream).await;
        assert!(
            matches!(welcome.body, Some(wire::server_frame::Body::Welcome(_))),
            "expected welcome"
        );
        Self { sink, stream }
    }

    async fn send(&mut self, operation: &str, body: wire::client_frame::Body) {
        let frame = wire::ClientFrame {
            operation_id: operation.into(),
            body: Some(body),
        };
        self.sink
            .send(Message::Binary(wire::encode_client(&frame).unwrap().into()))
            .await
            .unwrap();
    }

    async fn receive(&mut self) -> wire::ServerFrame {
        SocketReader::receive(&mut self.stream).await
    }
}

struct SocketReader;

impl SocketReader {
    async fn receive(stream: &mut futures_util::stream::SplitStream<Socket>) -> wire::ServerFrame {
        let message = tokio::time::timeout(std::time::Duration::from_secs(30), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Binary(bytes) = message else {
            panic!("expected binary synchronization frame");
        };
        wire::decode_server(&bytes).unwrap()
    }
}

fn publication_events() -> (Start, Vec<Event>) {
    let mut values = snapshot(1);
    let template = values[2]["records"][0].clone();
    for index in 0..140 {
        let mut record = template.clone();
        record["id"] = json!(format!("bulk-{index}"));
        record["ordinal"] = json!(index + 100);
        record["content"] = json!("repeated content ".repeat(1024));
        record["projection"] = record["content"].clone();
        values[2]["records"].as_array_mut().unwrap().push(record);
    }
    let parsed = values
        .iter()
        .cloned()
        .map(|value| serde_json::from_value::<Event>(value).unwrap())
        .collect::<Vec<_>>();
    let mut parsed = parsed.into_iter();
    let Some(Event::Start(start)) = parsed.next() else {
        panic!("snapshot must start with metadata");
    };
    let mut events = parsed.collect::<Vec<_>>();
    assert!(matches!(events.pop(), Some(Event::Finish)));
    (start, events)
}

fn publication() -> (Start, wire::EncodedBatch) {
    let (start, events) = publication_events();
    let encoded = wire::encode_publication(&events, wire::DEFAULT_MAX_DECODED_BYTES).unwrap();
    (start, encoded)
}

fn begin(start: &Start, encoded: &wire::EncodedBatch) -> wire::BeginPublication {
    wire::BeginPublication {
        start: Some(wire::PublicationStart::from(start)),
        mode: wire::PublicationMode::ReplaceSnapshot as i32,
        backfill_id: String::new(),
        source_revision: start.revision,
        journal_from_exclusive: None,
        journal_through_inclusive: None,
        manifest_digest: encoded.manifest_digest.clone(),
        decoded_bytes: encoded.decoded_bytes,
        record_count: encoded.record_count,
    }
}

#[tokio::test]
async fn websocket_requires_authentication_before_upgrade() {
    let service = Service::new(false).await;
    let url = format!("{}/v1/sync", service.url.replacen("http://", "ws://", 1));
    let request = url.into_client_request().unwrap();
    let error = tokio_tungstenite::connect_async(request).await.unwrap_err();
    let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
        panic!("expected HTTP rejection, got {error:?}");
    };
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one end-to-end flow proves resume, apply failure and durable receipt"
)]
async fn websocket_resumes_durable_checkpoints_and_reports_application_failure() {
    let service = Service::new(false).await;
    let (start, encoded) = publication();
    let operation = scoped_operation_id(
        "publication",
        &service.first.node_id,
        &encoded.manifest_digest,
    );
    assert!(encoded.pieces.len() > 1, "test needs multiple pieces");

    let mut client = SyncClient::connect(&service, FIRST, &start.instance).await;
    client
        .send(
            &operation,
            wire::client_frame::Body::BeginPublication(begin(&start, &encoded)),
        )
        .await;
    assert!(matches!(
        client.receive().await.body,
        Some(wire::server_frame::Body::Accepted(_))
    ));
    client
        .send(
            &operation,
            wire::client_frame::Body::Chunk(wire::EncodedBatch::chunk(&encoded.pieces[0])),
        )
        .await;
    let checkpoint = client.receive().await;
    let Some(wire::server_frame::Body::Checkpoint(checkpoint)) = checkpoint.body else {
        panic!("expected durable checkpoint");
    };
    assert_eq!(checkpoint.sequence, 0);
    drop(client);

    // A new connection and catalog instance recover the acknowledged prefix.
    let mut resumed = SyncClient::connect(&service, FIRST, &start.instance).await;
    resumed
        .send(
            &operation,
            wire::client_frame::Body::Resume(wire::Resume {
                manifest_digest: encoded.manifest_digest.clone(),
            }),
        )
        .await;
    let outcome = resumed.receive().await;
    let Some(wire::server_frame::Body::Outcome(outcome)) = outcome.body else {
        panic!("expected resume outcome");
    };
    assert_eq!(outcome.state, wire::OutcomeState::Receiving as i32);
    assert_eq!(outcome.checkpoint_sequence, 0);

    resumed
        .send(
            &operation,
            wire::client_frame::Body::BeginPublication(begin(&start, &encoded)),
        )
        .await;
    let resent = resumed.receive().await;
    let Some(wire::server_frame::Body::Checkpoint(resent)) = resent.body else {
        panic!("expected recovered checkpoint");
    };
    assert_eq!(resent.sequence, 0);
    assert!(matches!(
        resumed.receive().await.body,
        Some(wire::server_frame::Body::Accepted(_))
    ));
    for piece in &encoded.pieces[1..] {
        resumed
            .send(
                &operation,
                wire::client_frame::Body::Chunk(wire::EncodedBatch::chunk(piece)),
            )
            .await;
        loop {
            match resumed.receive().await.body {
                Some(wire::server_frame::Body::Checkpoint(_)) => break,
                Some(wire::server_frame::Body::Credit(_)) => {}
                other => panic!("unexpected chunk response: {other:?}"),
            }
        }
    }
    resumed
        .send(
            &operation,
            wire::client_frame::Body::FinishUpload(wire::FinishUpload {
                manifest_digest: encoded.manifest_digest.clone(),
            }),
        )
        .await;
    let failure = loop {
        match resumed.receive().await.body {
            Some(
                wire::server_frame::Body::SilverCommitted(_)
                | wire::server_frame::Body::Progress(_)
                | wire::server_frame::Body::Credit(_)
                | wire::server_frame::Body::Checkpoint(_),
            ) => {}
            Some(wire::server_frame::Body::Failed(failed)) => break failed,
            other => panic!("unexpected finish response: {other:?}"),
        }
    };
    assert_eq!(failure.phase, "applying");
    assert_eq!(failure.commit_state, wire::CommitState::NotCommitted as i32);
    drop(resumed);

    // The durable receipt survives the connection and reports the failed state.
    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    let operation = catalog
        .select(&operation, &service.first.group_id, "first")
        .unwrap();
    assert_eq!(operation.state, catalog::State::Failed);
    assert_eq!(operation.decoded_offset, encoded.decoded_bytes);
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_websocket_publishes_typed_batch_and_certifies_views() {
    let service = Service::qdrant(384).await;
    let (start, encoded) = publication();
    let operation = scoped_operation_id(
        "publication",
        &service.first.node_id,
        &encoded.manifest_digest,
    );
    let mut client = SyncClient::connect(&service, FIRST, &start.instance).await;

    client
        .send(
            &operation,
            wire::client_frame::Body::BeginPublication(begin(&start, &encoded)),
        )
        .await;
    assert!(matches!(
        client.receive().await.body,
        Some(wire::server_frame::Body::Accepted(_))
    ));
    for piece in &encoded.pieces {
        client
            .send(
                &operation,
                wire::client_frame::Body::Chunk(wire::EncodedBatch::chunk(piece)),
            )
            .await;
    }
    client
        .send(
            &operation,
            wire::client_frame::Body::FinishUpload(wire::FinishUpload {
                manifest_digest: encoded.manifest_digest.clone(),
            }),
        )
        .await;

    let mut committed_receipt = None;
    let published = loop {
        match client.receive().await.body {
            Some(wire::server_frame::Body::Published(published)) => break published,
            Some(wire::server_frame::Body::SilverCommitted(committed)) => {
                committed_receipt = Some(committed.receipt_digest);
            }
            Some(wire::server_frame::Body::Failed(failed)) => {
                panic!("publication failed: {failed:?}");
            }
            Some(
                wire::server_frame::Body::Checkpoint(_)
                | wire::server_frame::Body::Credit(_)
                | wire::server_frame::Body::Progress(_)
                | wire::server_frame::Body::Queued(_),
            ) => {}
            other => panic!("unexpected finish response: {other:?}"),
        }
    };
    assert!(published.views_current);
    assert!(published.revision >= 1);

    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    let operation = catalog
        .select(&operation, &service.first.group_id, "first")
        .unwrap();
    assert_eq!(operation.state, catalog::State::Published);
    assert_eq!(operation.revision, Some(published.revision));

    // The canonical receipt lives on the silver head, written in the same
    // transaction as the history it describes.
    let (committed, manifest, receipt_digest) = service
        .store
        .sync_receipt(&service.first)
        .await
        .unwrap()
        .expect("silver head records the transfer receipt");
    assert_eq!(committed, operation.operation_id);
    assert_eq!(manifest, encoded.manifest_digest);
    assert_eq!(Some(receipt_digest), committed_receipt);
    drop(client);
    service.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_native_publisher_spools_and_uploads_over_websocket() {
    use std::io::Write;

    let service = Service::qdrant(384).await;
    let (start, events) = publication_events();
    let file = tempfile::NamedTempFile::new().unwrap();
    {
        let mut writer = std::io::BufWriter::new(file.as_file());
        let policy = crate::redaction::Policy::new(false);
        crate::shared::write_event(&mut writer, &Event::Start(start), policy).unwrap();
        for event in &events {
            crate::shared::write_event(&mut writer, event, policy).unwrap();
        }
        crate::shared::write_event(&mut writer, &Event::Finish, policy).unwrap();
        writer.flush().unwrap();
    }
    let config = crate::config::SharedConfig {
        group_id: service.first.group_id.clone(),
        node_id: service.first.node_id.clone(),
        api_url: service.url.clone(),
        token_env: "unused-in-test".into(),
        publication_redaction: crate::redaction::Policy::new(false),
    };
    let indexed = crate::shared::sync::client::upload_with_token(&config, &file, FIRST.to_owned())
        .await
        .unwrap();
    assert!(indexed.revision >= 1);

    let freshness: crate::shared::Freshness =
        serde_json::from_value(service.get("freshness", FIRST).await).unwrap();
    let node = freshness
        .nodes
        .iter()
        .find(|node| node.node_id == "first")
        .expect("freshness includes the publishing node");
    assert_eq!(node.revision, indexed.revision);
    assert!(node.views_current);
    service.cleanup().await;
}

/// Encode one background operation body as a single zstd-compressed piece and
/// its manifest digest.
fn transfer(bytes: &[u8], records: u32) -> (wire::Piece, String) {
    let encoded = wire::compress(bytes).unwrap();
    let piece = wire::Piece {
        sequence: 0,
        decoded_offset: 0,
        decoded_length: bytes.len() as u64,
        digest: blake3::hash(bytes).to_hex().to_string(),
        encoded,
        compression: wire::Compression::Zstd as i32,
    };
    let entry = wire::ManifestPiece {
        sequence: 0,
        decoded_offset: 0,
        decoded_length: piece.decoded_length,
        digest: &piece.digest,
    };
    let manifest = wire::manifest_digest(bytes.len() as u64, records, &[entry]);
    (piece, manifest)
}

async fn begin_upload(client: &mut SyncClient, operation: &str, body: wire::client_frame::Body) {
    client.send(operation, body).await;
    assert!(matches!(
        client.receive().await.body,
        Some(wire::server_frame::Body::Accepted(_))
    ));
}

async fn upload_piece(
    client: &mut SyncClient,
    operation: &str,
    piece: &wire::Piece,
    manifest: &str,
) {
    client
        .send(
            operation,
            wire::client_frame::Body::Chunk(wire::EncodedBatch::chunk(piece)),
        )
        .await;
    assert!(matches!(
        client.receive().await.body,
        Some(wire::server_frame::Body::Checkpoint(_))
    ));
    // Each accepted piece is acknowledged with a fresh credit grant.
    assert!(matches!(
        client.receive().await.body,
        Some(wire::server_frame::Body::Credit(_))
    ));
    client
        .send(
            operation,
            wire::client_frame::Body::FinishUpload(wire::FinishUpload {
                manifest_digest: manifest.into(),
            }),
        )
        .await;
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one flow proves manifest installation, coverage and rejection"
)]
async fn websocket_installs_backfill_manifests_and_refuses_premature_completion() {
    use prost::Message;

    // Baseline opening compares the frozen revision with the node's head, so
    // this service needs real silver; no views are written by this flow.
    let service = Service::canonical().await;
    let members = vec![
        wire::BackfillMember {
            conversation_id: "a".into(),
            digest: "1".repeat(64),
        },
        wire::BackfillMember {
            conversation_id: "b".into(),
            digest: "2".repeat(64),
        },
    ];
    let mut bytes = Vec::new();
    for member in &members {
        member.encode_length_delimited(&mut bytes).unwrap();
    }
    let records = u32::try_from(members.len()).unwrap();
    let (piece, manifest) = transfer(&bytes, records);
    let operation = format!("backfill-manifest:{manifest}");

    let mut client = SyncClient::connect(&service, FIRST, "snapshot-instance").await;
    begin_upload(
        &mut client,
        &operation,
        wire::client_frame::Body::BeginBackfill(wire::BeginBackfill {
            backfill_id: "baseline".into(),
            database_instance: "snapshot-instance".into(),
            baseline_revision: 1,
            views_fingerprint: "generation".into(),
            manifest_digest: manifest.clone(),
            total_conversations: members.len() as u64,
            decoded_bytes: bytes.len() as u64,
        }),
    )
    .await;
    client
        .send(
            &operation,
            wire::client_frame::Body::Chunk(wire::EncodedBatch::chunk(&piece)),
        )
        .await;
    assert!(matches!(
        client.receive().await.body,
        Some(wire::server_frame::Body::Checkpoint(_))
    ));
    assert!(matches!(
        client.receive().await.body,
        Some(wire::server_frame::Body::Credit(_))
    ));
    client
        .send(
            &operation,
            wire::client_frame::Body::FinishUpload(wire::FinishUpload {
                manifest_digest: manifest.clone(),
            }),
        )
        .await;
    let frame = client.receive().await;
    assert!(
        matches!(frame.body, Some(wire::server_frame::Body::Progress(_))),
        "expected manifest progress, got {frame:?}"
    );
    let frame = client.receive().await;
    assert!(
        matches!(frame.body, Some(wire::server_frame::Body::Accepted(_))),
        "expected manifest acceptance, got {frame:?}"
    );

    // The manifest is durable and no member is covered yet.
    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    let backfill = catalog
        .backfill("baseline", &service.first.group_id, "first")
        .unwrap();
    assert_eq!(backfill.state, "ready");
    assert_eq!(backfill.total_conversations, 2);
    assert_eq!(backfill.covered, 0);

    // Completing an uncovered baseline fails closed.
    client
        .send(
            "finish-backfill",
            wire::client_frame::Body::FinishBackfill(wire::FinishBackfill {
                backfill_id: "baseline".into(),
                baseline_revision: 1,
                manifest_digest: manifest.clone(),
            }),
        )
        .await;
    let rejected = client.receive().await;
    let Some(wire::server_frame::Body::Rejected(failed)) = rejected.body else {
        panic!("expected rejected backfill completion");
    };
    assert_eq!(failed.code, "backfill_incomplete");

    // A batch cannot claim a backfill that this node never opened.
    client
        .send(
            "publication:unknown",
            wire::client_frame::Body::BeginPublication(wire::BeginPublication {
                start: None,
                mode: wire::PublicationMode::BackfillBatch as i32,
                backfill_id: "other".into(),
                source_revision: Some(1),
                journal_from_exclusive: None,
                journal_through_inclusive: None,
                manifest_digest: "3".repeat(64),
                decoded_bytes: 1,
                record_count: 1,
            }),
        )
        .await;
    let rejected = client.receive().await;
    let Some(wire::server_frame::Body::Rejected(failed)) = rejected.body else {
        panic!("expected rejected backfill batch");
    };
    assert_eq!(failed.code, "missing_publication_metadata");

    drop(client);
}

#[tokio::test]
async fn websocket_commits_connector_batches_without_the_http_route() {
    let service = Service::canonical().await;
    let batch = json!({
        "schema_version": 2,
        "source_id": "github:installation",
        "batch_id": "first",
        "expected_revision": 0,
        "changes": [
            {"op":"upsert-object","id":"note","role":"document","kind":"obsidian:note","content":"Wire note"}
        ]
    });
    let bytes = serde_json::to_vec(&batch).unwrap();
    let (piece, manifest) = transfer(&bytes, 0);
    let operation = format!("connector:{manifest}");

    let mut client = SyncClient::connect(&service, FIRST, "snapshot-instance").await;
    begin_upload(
        &mut client,
        &operation,
        wire::client_frame::Body::BeginConnectorBatch(wire::BeginConnectorBatch {
            source_id: "github:installation".into(),
            batch_id: "first".into(),
            expected_revision: "0".into(),
            manifest_digest: manifest.clone(),
            decoded_bytes: bytes.len() as u64,
        }),
    )
    .await;
    upload_piece(&mut client, &operation, &piece, &manifest).await;
    let committed = loop {
        match client.receive().await.body {
            Some(wire::server_frame::Body::SilverCommitted(committed)) => break committed,
            Some(wire::server_frame::Body::Failed(failed)) => {
                panic!("connector batch failed: {failed:?}");
            }
            Some(
                wire::server_frame::Body::Checkpoint(_)
                | wire::server_frame::Body::Credit(_)
                | wire::server_frame::Body::Progress(_),
            ) => {}
            other => panic!("unexpected connector response: {other:?}"),
        }
    };
    assert!(committed.revision >= 1);
    let published = loop {
        match client.receive().await.body {
            Some(wire::server_frame::Body::Published(published)) => break published,
            Some(wire::server_frame::Body::Credit(_) | wire::server_frame::Body::Checkpoint(_)) => {
            }
            other => panic!("unexpected connector response: {other:?}"),
        }
    };
    assert_eq!(published.revision, committed.revision);
    // The socket returns the same source-scoped receipt the HTTP route did.
    let receipt = published.connector.expect("connector receipt");
    assert_eq!(receipt.source_id, "github:installation");
    assert_eq!(receipt.batch_id, "first");
    assert_eq!(receipt.source_revision, 1);
    assert_eq!(
        receipt.silver_revision,
        u64::try_from(published.revision).unwrap()
    );

    // The canonical silver document is readable through the connector store.
    let identity = &service.first;
    let path = service
        .silver_dir
        .path()
        .join("silver")
        .join(&identity.group_id)
        .join(format!("{}.sqlite3", identity.node_id));
    let object = crate::connectors::silver::Store::open(&path)
        .unwrap()
        .object(
            &"github:installation".to_owned().try_into().unwrap(),
            &"note".to_owned().try_into().unwrap(),
        )
        .unwrap()
        .expect("connector object was committed");
    assert_eq!(object.content.as_deref(), Some("Wire note"));

    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    let receipt = catalog
        .select(&operation, &identity.group_id, "first")
        .unwrap();
    assert_eq!(receipt.state, catalog::State::Published);

    drop(client);
}

/// A non-snapshot baseline batch for the standard fixture conversation.
fn baseline_batch() -> (Start, Vec<Event>, Vec<wire::BackfillMember>) {
    let values = snapshot(1);
    let record: crate::shared::protocol::ConversationRecord =
        serde_json::from_value(values[1]["records"][0].clone()).unwrap();
    let messages = values[2]["records"]
        .as_array()
        .unwrap()
        .iter()
        .cloned()
        .map(serde_json::from_value::<crate::shared::protocol::MessageRecord>)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let start = Start {
        views_build: None,
        source_silver: None,
        instance: "snapshot-instance".into(),
        revision: Some(1),
        base_revision: Some(0),
        snapshot: false,
        forgets: Vec::new(),
    };
    let members = vec![wire::BackfillMember {
        conversation_id: record.id.clone(),
        digest: wire::member_digest(&record).unwrap(),
    }];
    let events = vec![
        Event::Batch(crate::shared::protocol::Batch::Conversations {
            records: vec![record],
        }),
        Event::Batch(crate::shared::protocol::Batch::Messages { records: messages }),
    ];
    (start, events, members)
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
#[allow(
    clippy::too_many_lines,
    reason = "one flow proves revision guard, coverage and baseline completion"
)]
async fn qdrant_backfill_manifest_coverage_completes_only_after_baseline_batches() {
    use prost::Message;

    let service = Service::qdrant(384).await;
    let (start, events, members) = baseline_batch();
    let mut manifest_bytes = Vec::new();
    for member in &members {
        member.encode_length_delimited(&mut manifest_bytes).unwrap();
    }
    let records = u32::try_from(members.len()).unwrap();
    let (manifest_piece, manifest) = transfer(&manifest_bytes, records);
    let manifest_operation = format!("backfill-manifest:{manifest}");
    let encoded = wire::encode_publication(&events, wire::DEFAULT_MAX_DECODED_BYTES).unwrap();
    let batch_operation = scoped_operation_id(
        "publication",
        &service.first.node_id,
        &encoded.manifest_digest,
    );

    let mut client = SyncClient::connect(&service, FIRST, "snapshot-instance").await;
    begin_upload(
        &mut client,
        &manifest_operation,
        wire::client_frame::Body::BeginBackfill(wire::BeginBackfill {
            backfill_id: "baseline".into(),
            database_instance: "snapshot-instance".into(),
            baseline_revision: 1,
            views_fingerprint: "generation".into(),
            manifest_digest: manifest.clone(),
            total_conversations: members.len() as u64,
            decoded_bytes: manifest_bytes.len() as u64,
        }),
    )
    .await;
    upload_piece(&mut client, &manifest_operation, &manifest_piece, &manifest).await;
    for _ in 0..2 {
        client.receive().await;
    }

    // A baseline batch must name the frozen revision and an installed baseline.
    client
        .send(
            &batch_operation,
            wire::client_frame::Body::BeginPublication(wire::BeginPublication {
                start: Some(wire::PublicationStart::from(&start)),
                mode: wire::PublicationMode::BackfillBatch as i32,
                backfill_id: "baseline".into(),
                source_revision: Some(2),
                journal_from_exclusive: None,
                journal_through_inclusive: None,
                manifest_digest: encoded.manifest_digest.clone(),
                decoded_bytes: encoded.decoded_bytes,
                record_count: encoded.record_count,
            }),
        )
        .await;
    let rejected = client.receive().await;
    let Some(wire::server_frame::Body::Rejected(failed)) = rejected.body else {
        panic!("expected revision mismatch rejection");
    };
    assert_eq!(failed.code, "backfill_revision_mismatch");

    client
        .send(
            &batch_operation,
            wire::client_frame::Body::BeginPublication(wire::BeginPublication {
                start: Some(wire::PublicationStart::from(&start)),
                mode: wire::PublicationMode::BackfillBatch as i32,
                backfill_id: "baseline".into(),
                source_revision: Some(1),
                journal_from_exclusive: None,
                journal_through_inclusive: None,
                manifest_digest: encoded.manifest_digest.clone(),
                decoded_bytes: encoded.decoded_bytes,
                record_count: encoded.record_count,
            }),
        )
        .await;
    assert!(matches!(
        client.receive().await.body,
        Some(wire::server_frame::Body::Accepted(_))
    ));
    for piece in &encoded.pieces {
        client
            .send(
                &batch_operation,
                wire::client_frame::Body::Chunk(wire::EncodedBatch::chunk(piece)),
            )
            .await;
    }
    client
        .send(
            &batch_operation,
            wire::client_frame::Body::FinishUpload(wire::FinishUpload {
                manifest_digest: encoded.manifest_digest.clone(),
            }),
        )
        .await;
    let published = loop {
        match client.receive().await.body {
            Some(wire::server_frame::Body::Published(published)) => break published,
            Some(wire::server_frame::Body::Failed(failed)) => {
                panic!("baseline batch failed: {failed:?}");
            }
            Some(
                wire::server_frame::Body::Checkpoint(_)
                | wire::server_frame::Body::Credit(_)
                | wire::server_frame::Body::Progress(_)
                | wire::server_frame::Body::SilverCommitted(_),
            ) => {}
            other => panic!("unexpected baseline response: {other:?}"),
        }
    };
    assert!(published.views_current);

    // Coverage is durable and complete only after the baseline batch committed.
    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    let backfill = catalog
        .backfill("baseline", &service.first.group_id, "first")
        .unwrap();
    assert_eq!(backfill.covered, backfill.total_conversations);

    client
        .send(
            "finish-backfill",
            wire::client_frame::Body::FinishBackfill(wire::FinishBackfill {
                backfill_id: "baseline".into(),
                baseline_revision: 1,
                manifest_digest: manifest,
            }),
        )
        .await;
    let completed = client.receive().await;
    let Some(wire::server_frame::Body::BackfillComplete(completed)) = completed.body else {
        panic!("expected completed baseline");
    };
    assert_eq!(completed.covered_conversations, 1);

    drop(client);
    service.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_attachment_upload_uses_the_websocket_transport() {
    let service = Service::qdrant(384).await;
    let source = crate::attachments::Source::Base64 {
        name: "notes.txt".into(),
        media_type: "text/plain".into(),
        data: base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            "password=\"hunter2\"\nuseful notes",
        ),
    };
    let cache = tempfile::tempdir().unwrap();
    let record = crate::attachments::retain(
        cache.path(),
        &crate::attachments::Configuration::default(),
        "searchable",
        &[source],
    )
    .unwrap()
    .pop()
    .unwrap();
    let mut events = snapshot(1);
    events[2]["records"][0]["attachments"] = serde_json::to_value(vec![&record]).unwrap();
    assert_eq!(
        service.upload(FIRST, &events).await.status(),
        StatusCode::OK
    );

    let bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        crate::attachments::read(cache.path(), &record).unwrap(),
    )
    .unwrap();
    let config = crate::config::SharedConfig {
        group_id: service.first.group_id.clone(),
        node_id: service.first.node_id.clone(),
        api_url: service.url.clone(),
        token_env: "unused-in-test".into(),
        publication_redaction: crate::redaction::Policy::new(false),
    };
    crate::shared::sync::client::upload_attachment_with_token(
        &config,
        FIRST.to_owned(),
        "searchable",
        &record,
        &bytes,
    )
    .await
    .unwrap();

    // A different node can retrieve the retained bytes.
    let request = json!({"node_id":"first","message_id":"searchable","attachment_id":record.id});
    let downloaded: crate::attachments::Retrieved = service
        .request("attachment", SECOND)
        .json(&request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let text = String::from_utf8(
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &downloaded.data)
            .unwrap(),
    )
    .unwrap();
    assert!(!text.contains("hunter2"));
    assert!(text.contains("useful notes"));

    service.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_startup_recovery_resumes_queued_publications() {
    let service = Service::qdrant(384).await;
    let (start, encoded) = publication();
    let operation = scoped_operation_id(
        "publication",
        &service.first.node_id,
        &encoded.manifest_digest,
    );
    let group = service.first.group_id.clone();
    let start_json = serde_json::to_string(&Event::Start(start)).unwrap();
    {
        let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
        catalog
            .begin(&catalog::Begin {
                operation_id: &operation,
                group_id: &group,
                node_id: "first",
                kind: "publication",
                manifest_digest: &encoded.manifest_digest,
                decoded_bytes: encoded.decoded_bytes,
                record_count: encoded.record_count,
                start_json: &start_json,
            })
            .unwrap();
        for piece in &encoded.pieces {
            catalog
                .append(
                    &operation,
                    &group,
                    "first",
                    &catalog::StoredChunk {
                        sequence: piece.sequence,
                        decoded_offset: piece.decoded_offset,
                        decoded_length: piece.decoded_length,
                        digest: piece.digest.clone(),
                        encoded: piece.encoded.clone(),
                        compression: piece.compression,
                    },
                )
                .unwrap();
        }
        // Complete receipt without applying: the shape a crash leaves behind.
        catalog
            .finish(&operation, &group, "first", &encoded.manifest_digest)
            .unwrap();
        assert_eq!(
            catalog.select(&operation, &group, "first").unwrap().state,
            catalog::State::QueuedForApply
        );
    }

    assert_eq!(service.recover().await, 1, "one durable operation resumed");

    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    let recovered = catalog.select(&operation, &group, "first").unwrap();
    assert_eq!(recovered.state, catalog::State::Published);
    assert!(recovered.revision.unwrap_or_default() >= 1);
    assert!(recovered.receipt_digest.is_some());

    // Silver and views both advanced, so a later reconnect reports readiness.
    let freshness: crate::shared::Freshness =
        serde_json::from_value(service.get("freshness", FIRST).await).unwrap();
    let node = freshness
        .nodes
        .iter()
        .find(|node| node.node_id == "first")
        .expect("freshness includes the recovered node");
    assert_eq!(node.revision, recovered.revision.unwrap());
    assert!(node.views_current);
    service.cleanup().await;
}

/// One complete-conversation delta with a stable id and changed content, in
/// both wire shapes the two transports take.
#[allow(clippy::cast_precision_loss)]
fn delta(revision: i64, messages: usize) -> (Start, Vec<Value>, Vec<Event>) {
    // Deterministic but varied vectors: identical vectors would let zstd
    // collapse the payload and overstate the transport comparison.
    let vector = |ordinal: u64| {
        let mut state = revision
            .cast_unsigned()
            .wrapping_mul(1_000_003)
            .wrapping_add(ordinal)
            .wrapping_add(1);
        (0..384)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((state >> 33) as f64 / f64::from(u32::MAX)) - 0.5
            })
            .collect::<Vec<f64>>()
    };
    let messages = (0..messages)
        .map(|ordinal| {
            serde_json::json!({
                "id": format!("message-{ordinal}"),
                "conversation_id": "conversation",
                "ordinal": ordinal,
                "role": "user",
                "content": format!("revision {revision} message {ordinal} payload"),
                "projection": format!("revision {revision} message {ordinal} payload"),
                "created_at": revision,
                "generation": crate::semantic::SHARED_EMBEDDING_GENERATION,
                "vector": vector(u64::try_from(ordinal).unwrap())
            })
        })
        .collect::<Vec<_>>();
    // The silver contract accepts at most 128 records per batch, so a large
    // publication is a sequence of bounded message batches.
    let mut values = vec![
        serde_json::json!({
            "type": "start",
            "instance": "snapshot-instance",
            "revision": revision,
            "base_revision": revision - 1,
            "snapshot": false,
            "forgets": []
        }),
        serde_json::json!({
            "type": "batch",
            "kind": "conversations",
            "records": [{
                "id": "conversation",
                "provider": "codex",
                "title": format!("Shared history {revision}"),
                "project": "/work/mash",
                "repository": "repo://mash",
                "created_at": 1,
                "updated_at": revision
            }]
        }),
    ];
    for chunk in messages.chunks(64) {
        values.push(serde_json::json!({
            "type": "batch",
            "kind": "messages",
            "records": chunk.to_vec()
        }));
    }
    values.push(serde_json::json!({"type": "finish"}));

    let parsed = values
        .iter()
        .cloned()
        .map(|value| serde_json::from_value::<Event>(value).unwrap())
        .collect::<Vec<_>>();
    let mut parsed = parsed.into_iter();
    let Some(Event::Start(start)) = parsed.next() else {
        panic!("delta starts with metadata");
    };
    let events = parsed
        .filter(|event| !matches!(event, Event::Finish))
        .collect::<Vec<_>>();
    (start, values, events)
}

fn median(samples: &mut [std::time::Duration]) -> std::time::Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[allow(clippy::cast_precision_loss)]
fn summarize(label: &str, mut samples: Vec<std::time::Duration>, bytes: usize) -> String {
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let p90 = samples[(samples.len() * 9 / 10).min(samples.len() - 1)];
    format!(
        "{label}: median {:.1} ms, p90 {:.1} ms, {:.1} KiB payload per batch",
        median.as_secs_f64() * 1000.0,
        p90.as_secs_f64() * 1000.0,
        bytes as f64 / 1024.0
    )
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
#[allow(
    clippy::too_many_lines,
    reason = "one comparative measurement of both mutation transports"
)]
async fn qdrant_publish_latency_and_bandwidth_http_versus_websocket() {
    use std::time::Instant;

    const BATCHES: i64 = 6;
    let service = Service::qdrant(384).await;
    let seed = snapshot(1);
    assert_eq!(
        service.upload(FIRST, &seed).await.status(),
        StatusCode::OK,
        "seed snapshot must publish"
    );

    // Warm both transports once, so connection setup and the first views write
    // are not attributed to either path.
    let mut revision = 2;
    let (_, warm_values, _) = delta(revision, 6);
    assert_eq!(
        service.upload(FIRST, &warm_values).await.status(),
        StatusCode::OK
    );
    revision += 1;
    let (warm_start, _, warm_events) = delta(revision, 6);
    revision += 1;
    let warm_encoded =
        wire::encode_publication(&warm_events, wire::DEFAULT_MAX_DECODED_BYTES).unwrap();
    let publisher = crate::config::SharedConfig {
        group_id: service.first.group_id.clone(),
        node_id: service.first.node_id.clone(),
        api_url: service.url.clone(),
        token_env: "unused-in-test".into(),
        publication_redaction: crate::redaction::Policy::new(false),
    };
    crate::shared::sync::client::upload_encoded_with_token(
        &publisher,
        FIRST.to_owned(),
        &warm_start,
        &warm_encoded,
    )
    .await
    .expect("warm-up publication");

    for (label, messages) in [("small", 6usize), ("large", 160usize)] {
        let mut http = Vec::new();
        let mut http_bytes = 0usize;
        for _ in 0..BATCHES {
            let (_, values, _) = delta(revision, messages);
            let mut body = String::new();
            for value in &values {
                use std::fmt::Write as _;
                let _ = writeln!(body, "{value}");
            }
            http_bytes = http_bytes.saturating_add(body.len());
            let started = Instant::now();
            let response = service.upload(FIRST, &values).await;
            assert_eq!(response.status(), StatusCode::OK);
            http.push(started.elapsed());
            revision += 1;
        }

        let mut socket = Vec::new();
        let mut socket_bytes = 0usize;
        for _ in 0..BATCHES {
            let (start, _, events) = delta(revision, messages);
            let encoded =
                wire::encode_publication(&events, wire::DEFAULT_MAX_DECODED_BYTES).unwrap();
            socket_bytes = socket_bytes.saturating_add(
                encoded
                    .pieces
                    .iter()
                    .map(|piece| piece.encoded.len())
                    .sum::<usize>(),
            );
            let started = Instant::now();
            // The production publisher path: one session is reused across the
            // measured batches, so the handshake is amortized like watch mode.
            crate::shared::sync::client::upload_encoded_with_token(
                &publisher,
                FIRST.to_owned(),
                &start,
                &encoded,
            )
            .await
            .expect("websocket publication must publish");
            socket.push(started.elapsed());
            revision += 1;
        }

        assert_eq!(http.len(), usize::try_from(BATCHES).unwrap());
        assert_eq!(socket.len(), usize::try_from(BATCHES).unwrap());
        let http_average = http_bytes / http.len().max(1);
        let socket_average = socket_bytes / socket.len().max(1);
        #[allow(clippy::cast_precision_loss)]
        let reduction = 100.0 * (1.0 - socket_average as f64 / http_average.max(1) as f64);
        let mut http_median_samples = http.clone();
        let mut socket_median_samples = socket.clone();
        let http_median = median(&mut http_median_samples).as_secs_f64();
        let socket_median = median(&mut socket_median_samples).as_secs_f64();
        println!(
            "{label} batches ({messages} messages): {} | {} | {reduction:.1}% fewer payload bytes",
            summarize("legacy HTTP /v1/index", http, http_average),
            summarize("/v1/sync WebSocket", socket, socket_average)
        );
        assert!(reduction > 0.0, "the typed transport must send fewer bytes");
        // Measured service time plus the wire time of the payload at each link
        // rate. Loopback has no link cost, so this is where fewer bytes pay off.
        for rate in [10.0f64, 50.0, 200.0] {
            #[allow(clippy::cast_precision_loss)]
            let bits = |bytes: usize| bytes as f64 * 8.0;
            let http_total = http_median + bits(http_average) / (rate * 1_000_000.0);
            let socket_total = socket_median + bits(socket_average) / (rate * 1_000_000.0);
            println!(
                "  {label} at {rate:.0} Mbit/s: legacy {:.2} s -> sync {:.2} s ({:.0}% lower)",
                http_total,
                socket_total,
                100.0 * (1.0 - socket_total / http_total)
            );
        }
    }
    service.cleanup().await;
}

async fn publish_over_socket(
    service: &Service,
    operation: &str,
    start: &Start,
    encoded: &wire::EncodedBatch,
) -> Option<wire::Published> {
    let mut client = SyncClient::connect(service, FIRST, &start.instance).await;
    client
        .send(
            operation,
            wire::client_frame::Body::BeginPublication(wire::BeginPublication {
                start: Some(wire::PublicationStart::from(start)),
                mode: if start.snapshot {
                    wire::PublicationMode::ReplaceSnapshot as i32
                } else {
                    wire::PublicationMode::Delta as i32
                },
                backfill_id: String::new(),
                source_revision: start.revision,
                journal_from_exclusive: None,
                journal_through_inclusive: None,
                manifest_digest: encoded.manifest_digest.clone(),
                decoded_bytes: encoded.decoded_bytes,
                record_count: encoded.record_count,
            }),
        )
        .await;
    assert!(matches!(
        client.receive().await.body,
        Some(wire::server_frame::Body::Accepted(_))
    ));
    for piece in &encoded.pieces {
        client
            .send(
                operation,
                wire::client_frame::Body::Chunk(wire::EncodedBatch::chunk(piece)),
            )
            .await;
    }
    client
        .send(
            operation,
            wire::client_frame::Body::FinishUpload(wire::FinishUpload {
                manifest_digest: encoded.manifest_digest.clone(),
            }),
        )
        .await;
    loop {
        match client.receive().await.body {
            Some(wire::server_frame::Body::Published(published)) => return Some(published),
            Some(wire::server_frame::Body::Failed(failed)) => {
                panic!("websocket publication failed: {failed:?}");
            }
            Some(
                wire::server_frame::Body::Checkpoint(_)
                | wire::server_frame::Body::Credit(_)
                | wire::server_frame::Body::Progress(_)
                | wire::server_frame::Body::Queued(_)
                | wire::server_frame::Body::SilverCommitted(_),
            ) => {}
            other => panic!("unexpected publication response: {other:?}"),
        }
    }
}

/// Baseline records that arrive after a newer live publication must not apply:
/// the live conversation wins, and a live tombstone keeps the deletion.
#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
#[allow(
    clippy::too_many_lines,
    reason = "one flow freezes a baseline, edits live, deletes live and replays the baseline"
)]
async fn qdrant_late_baseline_cannot_overwrite_live_state_or_resurrect_deletions() {
    use prost::Message;

    let service = Service::qdrant(384).await;
    let vector = |seed: f64| {
        let mut values = vec![0.0; 384];
        values[0] = seed;
        values
    };
    let conversation = |id: &str, title: &str| {
        serde_json::json!({
            "id": id,
            "provider": "codex",
            "title": title,
            "project": "/work/mash",
            "repository": "repo://mash",
            "created_at": 1,
            "updated_at": 1
        })
    };
    let message = |id: &str, conversation: &str, ordinal: i64, content: &str, seed: f64| {
        serde_json::json!({
            "id": id,
            "conversation_id": conversation,
            "ordinal": ordinal,
            "role": "user",
            "content": content,
            "projection": content,
            "created_at": 1,
            "generation": crate::semantic::SHARED_EMBEDDING_GENERATION,
            "vector": vector(seed)
        })
    };

    // Frozen baseline revision 1: conversations a and b.
    let baseline = vec![
        serde_json::json!({"type":"start","instance":"snapshot-instance","revision":1,"base_revision":null,"snapshot":true,"forgets":[]}),
        serde_json::json!({"type":"batch","kind":"conversations","records":[
            conversation("a", "baseline a"),
            conversation("b", "baseline b")
        ]}),
        serde_json::json!({"type":"batch","kind":"messages","records":[
            message("a-1", "a", 0, "baseline a message", 1.0),
            message("b-1", "b", 0, "baseline b message", 2.0)
        ]}),
        serde_json::json!({"type":"finish"}),
    ];
    assert_eq!(
        service.upload(FIRST, &baseline).await.status(),
        StatusCode::OK
    );

    let members = ["a", "b"]
        .iter()
        .map(|id| wire::BackfillMember {
            conversation_id: (*id).into(),
            digest: wire::member_digest(
                &serde_json::from_value::<crate::shared::protocol::ConversationRecord>(
                    conversation(id, "unused"),
                )
                .unwrap(),
            )
            .unwrap(),
        })
        .collect::<Vec<_>>();

    // Open the baseline at revision 1 before any live edit, as the client does.
    let mut manifest_bytes = Vec::new();
    for member in &members {
        member.encode_length_delimited(&mut manifest_bytes).unwrap();
    }
    let records = u32::try_from(members.len()).unwrap();
    let (piece, manifest) = transfer(&manifest_bytes, records);
    let mut client = SyncClient::connect(&service, FIRST, "snapshot-instance").await;
    let operation = format!("backfill-manifest:{manifest}");
    begin_upload(
        &mut client,
        &operation,
        wire::client_frame::Body::BeginBackfill(wire::BeginBackfill {
            backfill_id: "baseline".into(),
            database_instance: "snapshot-instance".into(),
            baseline_revision: 1,
            views_fingerprint: "generation".into(),
            manifest_digest: manifest.clone(),
            total_conversations: members.len() as u64,
            decoded_bytes: manifest_bytes.len() as u64,
        }),
    )
    .await;
    upload_piece(&mut client, &operation, &piece, &manifest).await;
    for _ in 0..2 {
        client.receive().await;
    }

    // Live edit of a at revision 2, then a live deletion of b at revision 3,
    // both through the native transport that records ordering evidence.
    let edit = vec![
        Event::Batch(crate::shared::protocol::Batch::Conversations {
            records: vec![
                serde_json::from_value(conversation("a", "live a")).expect("live conversation"),
            ],
        }),
        Event::Batch(crate::shared::protocol::Batch::Messages {
            records: vec![
                serde_json::from_value(message("a-2", "a", 1, "live a message", 3.0)).unwrap(),
            ],
        }),
    ];
    let edit_start = Start {
        views_build: None,
        source_silver: None,
        instance: "snapshot-instance".into(),
        revision: Some(2),
        base_revision: Some(1),
        snapshot: false,
        forgets: Vec::new(),
    };
    let edit_encoded = wire::encode_publication(&edit, wire::DEFAULT_MAX_DECODED_BYTES).unwrap();
    let edit_operation = scoped_operation_id(
        "publication",
        &service.first.node_id,
        &edit_encoded.manifest_digest,
    );
    assert!(
        publish_over_socket(&service, &edit_operation, &edit_start, &edit_encoded)
            .await
            .is_some()
    );

    let delete = vec![Event::Batch(crate::shared::protocol::Batch::Tombstones {
        records: vec![
            serde_json::from_value(serde_json::json!({
                "provider": "codex",
                "conversation_id": "b"
            }))
            .unwrap(),
        ],
    })];
    let delete_start = Start {
        views_build: None,
        source_silver: None,
        instance: "snapshot-instance".into(),
        revision: Some(3),
        base_revision: Some(2),
        snapshot: false,
        forgets: Vec::new(),
    };
    let delete_encoded =
        wire::encode_publication(&delete, wire::DEFAULT_MAX_DECODED_BYTES).unwrap();
    let delete_operation = scoped_operation_id(
        "publication",
        &service.first.node_id,
        &delete_encoded.manifest_digest,
    );
    assert!(
        publish_over_socket(&service, &delete_operation, &delete_start, &delete_encoded)
            .await
            .is_some()
    );

    let before = service.state().await;
    assert_eq!(before.revision, 3);
    assert_eq!(before.tombstones.len(), 1);

    // The late baseline batch carries the revision-1 conversation records.
    let late = vec![
        Event::Batch(crate::shared::protocol::Batch::Conversations {
            records: ["a", "b"]
                .into_iter()
                .map(|id| {
                    serde_json::from_value(conversation(id, "baseline"))
                        .expect("baseline conversation record")
                })
                .collect(),
        }),
        Event::Batch(crate::shared::protocol::Batch::Messages {
            records: vec![
                serde_json::from_value(message("a-1", "a", 0, "baseline a message", 1.0)).unwrap(),
                serde_json::from_value(message("b-1", "b", 0, "baseline b message", 2.0)).unwrap(),
            ],
        }),
    ];
    let encoded = wire::encode_publication(&late, wire::DEFAULT_MAX_DECODED_BYTES).unwrap();
    let operation = scoped_operation_id(
        "publication",
        &service.first.node_id,
        &encoded.manifest_digest,
    );
    client
        .send(
            &operation,
            wire::client_frame::Body::BeginPublication(wire::BeginPublication {
                start: Some(wire::PublicationStart::from(&Start {
                    views_build: None,
                    source_silver: None,
                    instance: "snapshot-instance".into(),
                    revision: Some(1),
                    base_revision: Some(0),
                    snapshot: false,
                    forgets: Vec::new(),
                })),
                mode: wire::PublicationMode::BackfillBatch as i32,
                backfill_id: "baseline".into(),
                source_revision: Some(1),
                journal_from_exclusive: None,
                journal_through_inclusive: None,
                manifest_digest: encoded.manifest_digest.clone(),
                decoded_bytes: encoded.decoded_bytes,
                record_count: encoded.record_count,
            }),
        )
        .await;
    assert!(matches!(
        client.receive().await.body,
        Some(wire::server_frame::Body::Accepted(_))
    ));
    for piece in &encoded.pieces {
        client
            .send(
                &operation,
                wire::client_frame::Body::Chunk(wire::EncodedBatch::chunk(piece)),
            )
            .await;
    }
    client
        .send(
            &operation,
            wire::client_frame::Body::FinishUpload(wire::FinishUpload {
                manifest_digest: encoded.manifest_digest.clone(),
            }),
        )
        .await;
    let published = loop {
        match client.receive().await.body {
            Some(wire::server_frame::Body::Published(published)) => break published,
            Some(wire::server_frame::Body::Failed(failed)) => {
                panic!("late baseline failed: {failed:?}");
            }
            Some(
                wire::server_frame::Body::Checkpoint(_)
                | wire::server_frame::Body::Credit(_)
                | wire::server_frame::Body::SilverCommitted(_),
            ) => {}
            other => panic!("unexpected late baseline response: {other:?}"),
        }
    };
    // Nothing was applied: the live revision and deletion both survive.
    assert_eq!(published.revision, before.revision);
    let after = service.state().await;
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.tombstones.len(), 1);
    assert_eq!(after.tombstones[0].conversation_id, "b");

    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    let backfill = catalog
        .backfill("baseline", &service.first.group_id, "first")
        .unwrap();
    assert_eq!(backfill.covered, backfill.total_conversations);

    client
        .send(
            "finish-backfill",
            wire::client_frame::Body::FinishBackfill(wire::FinishBackfill {
                backfill_id: "baseline".into(),
                baseline_revision: 1,
                manifest_digest: manifest,
            }),
        )
        .await;
    let completed = client.receive().await;
    let Some(wire::server_frame::Body::BackfillComplete(completed)) = completed.body else {
        panic!("expected baseline completion");
    };
    assert_eq!(completed.covered_conversations, 2);

    // A baseline frozen behind the node's current revision is refused instead
    // of being rebased over newer live state.
    client
        .send(
            "stale-backfill",
            wire::client_frame::Body::BeginBackfill(wire::BeginBackfill {
                backfill_id: "stale".into(),
                database_instance: "snapshot-instance".into(),
                baseline_revision: 1,
                views_fingerprint: "generation".into(),
                manifest_digest: "d".repeat(64),
                total_conversations: 1,
                decoded_bytes: 1,
            }),
        )
        .await;
    let rejected = client.receive().await;
    let Some(wire::server_frame::Body::Rejected(failed)) = rejected.body else {
        panic!("expected stale baseline rejection");
    };
    assert_eq!(failed.code, "backfill_baseline_stale");

    drop(client);
    service.cleanup().await;
}

/// The production backfill sender drives manifest, batch and completion over
/// one `/v1/sync` session, without the test harness speaking the protocol.
#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_client_backfill_sender_completes_a_baseline_end_to_end() {
    use std::io::Write;

    let service = Service::qdrant(384).await;
    let values = snapshot(1);
    let mut events = values
        .iter()
        .cloned()
        .map(|value| serde_json::from_value::<Event>(value).unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(events.remove(0), Event::Start(_)));
    assert!(matches!(events.pop(), Some(Event::Finish)));

    // A backfill batch is a delta from a frozen baseline, not a snapshot.
    let start = Start {
        views_build: None,
        source_silver: None,
        instance: "snapshot-instance".into(),
        revision: Some(1),
        base_revision: Some(0),
        snapshot: false,
        forgets: Vec::new(),
    };
    let file = tempfile::NamedTempFile::new().unwrap();
    {
        let mut writer = std::io::BufWriter::new(file.as_file());
        let policy = crate::redaction::Policy::new(false);
        crate::shared::write_event(&mut writer, &Event::Start(start), policy).unwrap();
        for event in &events {
            crate::shared::write_event(&mut writer, event, policy).unwrap();
        }
        crate::shared::write_event(&mut writer, &Event::Finish, policy).unwrap();
        writer.flush().unwrap();
    }

    let config = crate::config::SharedConfig {
        group_id: service.first.group_id.clone(),
        node_id: service.first.node_id.clone(),
        api_url: service.url.clone(),
        token_env: "unused-in-test".into(),
        publication_redaction: crate::redaction::Policy::new(false),
    };
    let plan = crate::shared::sync::client::plan_backfill_file(
        &file,
        crate::shared::sync::client::BackfillLimits::default(),
    )
    .expect("plan the baseline");
    let indexed = crate::shared::sync::client::upload_backfill_plan(
        &config,
        FIRST.to_owned(),
        &plan,
        "baseline",
        crate::shared::sync::client::BackfillResume {
            completed: &std::collections::BTreeSet::new(),
            last_revision: 0,
        },
        None,
        1,
        |_, _| Ok(()),
    )
    .await
    .expect("client backfill completes");
    assert!(indexed.revision >= 1);

    // Manifest installed, batch covered, baseline complete.
    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    let backfill = catalog
        .backfill("baseline", &service.first.group_id, "first")
        .unwrap();
    assert_eq!(backfill.state, "complete");
    assert_eq!(backfill.covered, backfill.total_conversations);
    assert_eq!(backfill.total_conversations, 1);

    // The published batch is searchable through certified views.
    let freshness: crate::shared::Freshness =
        serde_json::from_value(service.get("freshness", FIRST).await).unwrap();
    let node = freshness
        .nodes
        .iter()
        .find(|node| node.node_id == "first")
        .expect("freshness includes the publishing node");
    assert_eq!(node.revision, indexed.revision);
    assert!(node.views_current);
    service.cleanup().await;
}

/// Silver and its receipt commit first; a views failure is reported separately
/// and never as a published batch.
#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_reports_silver_committed_before_a_certification_failure() {
    let service = Service::qdrant_views_failure(384).await;
    let (start, encoded) = publication();
    let operation = scoped_operation_id(
        "publication",
        &service.first.node_id,
        &encoded.manifest_digest,
    );
    let mut client = SyncClient::connect(&service, FIRST, &start.instance).await;

    begin_upload(
        &mut client,
        &operation,
        wire::client_frame::Body::BeginPublication(begin(&start, &encoded)),
    )
    .await;
    for piece in &encoded.pieces {
        client
            .send(
                &operation,
                wire::client_frame::Body::Chunk(wire::EncodedBatch::chunk(piece)),
            )
            .await;
    }
    client
        .send(
            &operation,
            wire::client_frame::Body::FinishUpload(wire::FinishUpload {
                manifest_digest: encoded.manifest_digest.clone(),
            }),
        )
        .await;

    let mut committed = None;
    let failure = loop {
        match client.receive().await.body {
            Some(wire::server_frame::Body::SilverCommitted(frame)) => committed = Some(frame),
            Some(wire::server_frame::Body::Failed(failed)) => break failed,
            Some(wire::server_frame::Body::Published(published)) => {
                panic!("certification must not be reported as published: {published:?}");
            }
            Some(
                wire::server_frame::Body::Checkpoint(_)
                | wire::server_frame::Body::Credit(_)
                | wire::server_frame::Body::Progress(_)
                | wire::server_frame::Body::Queued(_),
            ) => {}
            other => panic!("unexpected publication response: {other:?}"),
        }
    };

    let committed = committed.expect("silver commit is reported before the failure");
    assert_eq!(failure.commit_state, wire::CommitState::Committed as i32);
    assert_eq!(failure.code, "publication_failed");

    // Silver and the receipt are durable; views are deliberately not certified.
    let state = service.state().await;
    assert_eq!(state.revision, committed.revision);
    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    assert_eq!(
        catalog
            .select(&operation, &service.first.group_id, "first")
            .unwrap()
            .state,
        catalog::State::ViewsPending
    );
    let freshness: crate::shared::Freshness =
        serde_json::from_value(service.get("freshness", FIRST).await).unwrap();
    let node = freshness
        .nodes
        .iter()
        .find(|node| node.node_id == "first")
        .expect("freshness includes the publishing node");
    assert_eq!(node.revision, committed.revision);
    assert!(!node.views_current);

    // A reconnect reports the split outcome: committed, not certified.
    client
        .send(
            &operation,
            wire::client_frame::Body::Resume(wire::Resume {
                manifest_digest: encoded.manifest_digest.clone(),
            }),
        )
        .await;
    let resumed = client.receive().await;
    let Some(wire::server_frame::Body::Outcome(outcome)) = resumed.body else {
        panic!("expected resume outcome");
    };
    assert_eq!(outcome.state, wire::OutcomeState::ViewsPending as i32);
    assert_eq!(outcome.commit_state, wire::CommitState::Committed as i32);

    drop(client);
    service.cleanup().await;
}

/// The periodic sweep reclaims abandoned spools on its own: no connection and
/// no restart are involved.
#[tokio::test]
async fn periodic_sweep_reclaims_an_abandoned_receiving_spool() {
    let service = Service::new(false).await;
    let (piece, manifest) = transfer(b"abandoned-bytes", 1);
    let group = service.first.group_id.clone();
    {
        let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
        catalog
            .begin(&catalog::Begin {
                operation_id: "abandoned",
                group_id: &group,
                node_id: "first",
                kind: "publication",
                manifest_digest: &manifest,
                decoded_bytes: piece.decoded_length,
                record_count: 1,
                start_json: "{}",
            })
            .unwrap();
        catalog
            .append(
                "abandoned",
                &group,
                "first",
                &catalog::StoredChunk {
                    sequence: 0,
                    decoded_offset: 0,
                    decoded_length: piece.decoded_length,
                    digest: piece.digest.clone(),
                    encoded: piece.encoded.clone(),
                    compression: piece.compression,
                },
            )
            .unwrap();
        assert_eq!(
            catalog.select("abandoned", &group, "first").unwrap().state,
            catalog::State::Receiving
        );
    }

    let sweep = service.spawn_expiry(
        std::time::Duration::from_millis(20),
        std::time::Duration::from_millis(0),
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
            let expired = catalog
                .select("abandoned", &group, "first")
                .is_ok_and(|operation| operation.state == catalog::State::Expired);
            if expired {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the sweep reclaims the abandoned row");
    sweep.abort();

    // The spool and its pieces are gone, so a reconnect cannot resume it.
    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    assert!(
        catalog
            .recoverable()
            .unwrap()
            .iter()
            .all(|row| row.operation_id != "abandoned")
    );
    assert!(
        !service
            .silver_dir
            .path()
            .join("sync")
            .join("pieces")
            .join(blake3::hash(b"abandoned").to_hex().as_str())
            .exists()
    );
}

/// The periodic apply retry finishes durable queued work without a restart and
/// without a client reconnecting.
#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn periodic_apply_retry_finishes_queued_publications() {
    let service = Service::qdrant(384).await;
    let (start, encoded) = publication();
    let operation = scoped_operation_id(
        "publication",
        &service.first.node_id,
        &encoded.manifest_digest,
    );
    let group = service.first.group_id.clone();
    let start_json = serde_json::to_string(&Event::Start(start)).unwrap();
    {
        let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
        catalog
            .begin(&catalog::Begin {
                operation_id: &operation,
                group_id: &group,
                node_id: "first",
                kind: "publication",
                manifest_digest: &encoded.manifest_digest,
                decoded_bytes: encoded.decoded_bytes,
                record_count: encoded.record_count,
                start_json: &start_json,
            })
            .unwrap();
        for piece in &encoded.pieces {
            catalog
                .append(
                    &operation,
                    &group,
                    "first",
                    &catalog::StoredChunk {
                        sequence: piece.sequence,
                        decoded_offset: piece.decoded_offset,
                        decoded_length: piece.decoded_length,
                        digest: piece.digest.clone(),
                        encoded: piece.encoded.clone(),
                        compression: piece.compression,
                    },
                )
                .unwrap();
        }
        catalog
            .finish(&operation, &group, "first", &encoded.manifest_digest)
            .unwrap();
        assert_eq!(
            catalog.select(&operation, &group, "first").unwrap().state,
            catalog::State::QueuedForApply
        );
    }

    let retry = service.spawn_apply_retry(std::time::Duration::from_millis(20));
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
            if catalog
                .select(&operation, &group, "first")
                .is_ok_and(|operation| operation.state == catalog::State::Published)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the apply retry publishes without a restart or reconnect");
    retry.abort();

    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    let published = catalog.select(&operation, &group, "first").unwrap();
    assert!(published.revision.unwrap_or_default() >= 1);
    let freshness: crate::shared::Freshness =
        serde_json::from_value(service.get("freshness", FIRST).await).unwrap();
    let node = freshness
        .nodes
        .iter()
        .find(|node| node.node_id == "first")
        .expect("freshness includes the recovered node");
    assert_eq!(node.revision, published.revision.unwrap());
    assert!(node.views_current);
    service.cleanup().await;
}

/// The coordinator commits batch by batch: the first commit is searchable
/// before the second batch is even sent.
#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
#[allow(
    clippy::too_many_lines,
    reason = "one flow proves per-batch commits and progressive visibility"
)]
async fn qdrant_backfill_commits_each_batch_progressively() {
    use crate::shared::sync::client::{BackfillLimits, plan_backfill, upload_backfill_plan};

    let service = Service::qdrant(384).await;
    let vector = |seed: f64| {
        let mut values = vec![0.0; 384];
        values[0] = seed;
        values
    };
    let chat = |id: &str, updated_at: i64| crate::shared::protocol::ConversationRecord {
        enrichment: None,
        id: id.into(),
        provider: crate::provider::Provider::Codex,
        title: Some(id.into()),
        project: None,
        repository: None,
        created_at: Some(1),
        updated_at: Some(updated_at),
    };
    let message =
        |conversation: &str, id: &str, seed: f64| crate::shared::protocol::MessageRecord {
            attachments: Vec::new(),
            id: id.into(),
            conversation_id: conversation.into(),
            ordinal: 0,
            context: crate::message_context::MessageContext::Linear,
            role: crate::role::Role::User,
            content: format!("{conversation} content"),
            projection: format!("{conversation} content"),
            created_at: Some(1),
            generation: Some(crate::semantic::SHARED_EMBEDDING_GENERATION.to_owned()),
            vector: Some(vector(seed)),
            tools: crate::tools::Activity::default(),
        };
    let start = Start {
        views_build: None,
        source_silver: None,
        instance: "snapshot-instance".into(),
        revision: Some(1),
        base_revision: Some(0),
        snapshot: false,
        forgets: Vec::new(),
    };
    let events = vec![
        Event::Batch(crate::shared::protocol::Batch::Conversations {
            records: vec![chat("newer", 20), chat("older", 10)],
        }),
        Event::Batch(crate::shared::protocol::Batch::Messages {
            records: vec![
                message("newer", "newer-1", 1.0),
                message("older", "older-1", 2.0),
            ],
        }),
    ];
    // One conversation per batch keeps the assertion deterministic.
    let plan = plan_backfill(
        start,
        events,
        BackfillLimits {
            conversations: 1,
            decoded_bytes: 1024 * 1024,
        },
    )
    .expect("plan two batches");
    assert_eq!(plan.batches.len(), 2);

    let config = crate::config::SharedConfig {
        group_id: service.first.group_id.clone(),
        node_id: service.first.node_id.clone(),
        api_url: service.url.clone(),
        token_env: "unused-in-test".into(),
        publication_redaction: crate::redaction::Policy::new(false),
    };
    let (progress, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let completed = std::collections::BTreeSet::new();
    let upload = upload_backfill_plan(
        &config,
        FIRST.to_owned(),
        &plan,
        "progressive",
        crate::shared::sync::client::BackfillResume {
            completed: &completed,
            last_revision: 0,
        },
        Some(progress),
        1,
        |_, _| Ok(()),
    );
    tokio::pin!(upload);
    let mut batches = 0;
    loop {
        tokio::select! {
            event = observed.recv() => {
                if let Some(event) = event {
                    batches += 1;
                    assert_eq!(event.index, batches - 1, "batches arrive in order");
                    assert_eq!(event.total, 2, "both batches are planned");
                    if batches == 1 {
                        // The gate holds the sender here: the second batch has
                        // not been sent, yet the first one is already durable.
                        let counts = service.counts(FIRST).await;
                        assert_eq!(counts.conversations, 1, "first batch committed alone");
                        assert_eq!(counts.messages, 1);
                        assert_eq!(service.state().await.revision, event.revision);
                        // Coverage is visible while the baseline is incomplete.
                        let freshness: crate::shared::Freshness =
                            serde_json::from_value(service.get("freshness", FIRST).await).unwrap();
                        let baseline = freshness.baseline.expect("baseline coverage");
                        assert_eq!(baseline.id, "progressive");
                        assert_eq!(baseline.state, "ready");
                        assert_eq!((baseline.covered, baseline.total), (1, 2));
                    }
                    let _ = event.acknowledge.send(());
                }
            }
            result = &mut upload => {
                result.expect("baseline completes");
                break;
            }
        }
    }
    assert_eq!(batches, 2, "each batch reported its own commit");
    let counts = service.counts(FIRST).await;
    assert_eq!(counts.conversations, 2);
    assert_eq!(counts.messages, 2);

    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    let backfill = catalog
        .backfill("progressive", &service.first.group_id, "first")
        .unwrap();
    assert_eq!(backfill.state, "complete");
    assert_eq!(backfill.covered, backfill.total_conversations);
    let freshness: crate::shared::Freshness =
        serde_json::from_value(service.get("freshness", FIRST).await).unwrap();
    let baseline = freshness.baseline.expect("baseline coverage");
    assert_eq!((baseline.covered, baseline.total), (2, 2));
    assert_eq!(baseline.state, "complete");
    service.cleanup().await;
}

/// Documents are written, certification fails, and a retry repairs and
/// certifies without re-committing silver.
#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_partial_views_are_repaired_by_resending_finish() {
    let service = Service::qdrant_partial_views(384).await;
    let (start, encoded) = publication();
    let operation = scoped_operation_id(
        "publication",
        &service.first.node_id,
        &encoded.manifest_digest,
    );
    let mut client = SyncClient::connect(&service, FIRST, &start.instance).await;

    begin_upload(
        &mut client,
        &operation,
        wire::client_frame::Body::BeginPublication(begin(&start, &encoded)),
    )
    .await;
    for piece in &encoded.pieces {
        client
            .send(
                &operation,
                wire::client_frame::Body::Chunk(wire::EncodedBatch::chunk(piece)),
            )
            .await;
    }
    client
        .send(
            &operation,
            wire::client_frame::Body::FinishUpload(wire::FinishUpload {
                manifest_digest: encoded.manifest_digest.clone(),
            }),
        )
        .await;
    let failure = loop {
        match client.receive().await.body {
            Some(wire::server_frame::Body::Failed(failed)) => break failed,
            Some(
                wire::server_frame::Body::Checkpoint(_)
                | wire::server_frame::Body::Credit(_)
                | wire::server_frame::Body::Progress(_)
                | wire::server_frame::Body::SilverCommitted(_),
            ) => {}
            other => panic!("unexpected response: {other:?}"),
        }
    };
    assert_eq!(failure.commit_state, wire::CommitState::Committed as i32);
    assert_eq!(failure.code, "publication_failed");
    // Silver committed, documents were written, certification did not run.
    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    assert_eq!(
        catalog
            .select(&operation, &service.first.group_id, "first")
            .unwrap()
            .state,
        catalog::State::ViewsPending
    );
    let freshness: crate::shared::Freshness =
        serde_json::from_value(service.get("freshness", FIRST).await).unwrap();
    assert!(!freshness.nodes[0].views_current);

    // A retry repairs views and certifies without a fresh silver commit.
    service.store.clear_views_partial_interruption();
    client
        .send(
            &operation,
            wire::client_frame::Body::FinishUpload(wire::FinishUpload {
                manifest_digest: encoded.manifest_digest.clone(),
            }),
        )
        .await;
    let published = loop {
        match client.receive().await.body {
            Some(wire::server_frame::Body::Published(published)) => break published,
            Some(
                wire::server_frame::Body::Checkpoint(_)
                | wire::server_frame::Body::Credit(_)
                | wire::server_frame::Body::Progress(_)
                | wire::server_frame::Body::SilverCommitted(_),
            ) => {}
            other => panic!("unexpected retry response: {other:?}"),
        }
    };
    assert!(published.views_current);
    let freshness: crate::shared::Freshness =
        serde_json::from_value(service.get("freshness", FIRST).await).unwrap();
    assert!(freshness.nodes[0].views_current);
    assert_eq!(freshness.nodes[0].revision, published.revision);
    drop(client);
    service.cleanup().await;
}

/// Two members of one group publish at the same time. Revisions, catalogs and
/// view certification stay per-node; the duplicate report still sees both.
#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn two_nodes_publish_concurrently_without_interference() {
    let service = Service::qdrant(384).await;
    let first_events = snapshot(11);
    let second_events = snapshot(22);
    let (first, second) = tokio::join!(
        service.upload(FIRST, &first_events),
        service.upload(SECOND, &second_events),
    );
    assert_eq!(first.status(), StatusCode::OK, "{:?}", first.body);
    assert_eq!(second.status(), StatusCode::OK, "{:?}", second.body);

    let freshness: crate::shared::Freshness =
        serde_json::from_value(service.get("freshness", FIRST).await).unwrap();
    let nodes: std::collections::BTreeMap<&str, i64> = freshness
        .nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node.revision))
        .collect();
    assert_eq!(nodes.len(), 2, "{freshness:?}");
    assert!(
        nodes.values().all(|revision| *revision > 0),
        "{freshness:?}"
    );
    assert!(
        freshness.nodes.iter().all(|node| node.views_current),
        "{freshness:?}"
    );
    assert_eq!(
        freshness.unpublished_nodes.as_deref().unwrap_or_default(),
        [] as [&str; 0],
        "{freshness:?}"
    );

    // The fixture publishes identical content from both nodes, so concurrent
    // writers must still be folded into one duplicate conversation.
    let report: crate::shared::FingerprintReport =
        serde_json::from_value(service.get("fingerprints", FIRST).await).unwrap();
    assert_eq!(report.duplicated_conversations, 1, "{report:?}");
    assert_eq!(report.nodes.len(), 2, "{report:?}");
    service.cleanup().await;
}

/// Lanes commit baseline batches concurrently. Each commit must still receive
/// the next revision under the node lock, and coverage must end complete.
#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
#[allow(
    clippy::too_many_lines,
    reason = "one end-to-end ordering scenario: plan, upload, coverage, freshness"
)]
async fn qdrant_backfill_lanes_assign_ordered_revisions() {
    use crate::shared::sync::client::{BackfillLimits, plan_backfill, upload_backfill_plan};

    let service = Service::qdrant(384).await;
    let vector = |seed: f64| {
        let mut values = vec![0.0; 384];
        values[0] = seed;
        values
    };
    let chat = |id: &str| crate::shared::protocol::ConversationRecord {
        enrichment: None,
        id: id.into(),
        provider: crate::provider::Provider::Codex,
        title: Some(id.into()),
        project: None,
        repository: None,
        created_at: Some(1),
        updated_at: Some(1),
    };
    let message = |conversation: &str, seed: f64| crate::shared::protocol::MessageRecord {
        attachments: Vec::new(),
        id: format!("{conversation}-1"),
        conversation_id: conversation.into(),
        ordinal: 0,
        context: crate::message_context::MessageContext::Linear,
        role: crate::role::Role::User,
        content: format!("{conversation} content"),
        projection: format!("{conversation} content"),
        created_at: Some(1),
        generation: Some(crate::semantic::SHARED_EMBEDDING_GENERATION.to_owned()),
        vector: Some(vector(seed)),
        tools: crate::tools::Activity::default(),
    };
    let ids = ["a", "b", "c", "d"];
    let start = Start {
        views_build: None,
        source_silver: None,
        instance: "snapshot-instance".into(),
        revision: Some(1),
        base_revision: Some(0),
        snapshot: false,
        forgets: Vec::new(),
    };
    let events = vec![
        Event::Batch(crate::shared::protocol::Batch::Conversations {
            records: ids.iter().map(|id| chat(id)).collect(),
        }),
        Event::Batch(crate::shared::protocol::Batch::Messages {
            records: ids
                .iter()
                .zip([1.0, 2.0, 3.0, 4.0])
                .map(|(id, seed)| message(id, seed))
                .collect(),
        }),
    ];
    // One conversation per batch: four batches, two lanes.
    let plan = plan_backfill(
        start,
        events,
        BackfillLimits {
            conversations: 1,
            decoded_bytes: 1024 * 1024,
        },
    )
    .expect("plan four batches");
    assert_eq!(plan.batches.len(), 4);

    let config = crate::config::SharedConfig {
        group_id: service.first.group_id.clone(),
        node_id: service.first.node_id.clone(),
        api_url: service.url.clone(),
        token_env: "unused-in-test".into(),
        publication_redaction: crate::redaction::Policy::new(false),
    };
    let completed = std::collections::BTreeSet::new();
    let indexed = upload_backfill_plan(
        &config,
        FIRST.to_owned(),
        &plan,
        "lanes",
        crate::shared::sync::client::BackfillResume {
            completed: &completed,
            last_revision: 0,
        },
        None,
        2,
        |_, _| Ok(()),
    )
    .await
    .expect("four batches commit concurrently");

    // Revisions are assigned under the node lock: four batches, revisions 1..4,
    // so the head is the batch count and no two commits shared a revision.
    assert_eq!(indexed.revision, 4, "{indexed:?}");
    let counts = service.counts(FIRST).await;
    assert_eq!(counts.conversations, 4);
    assert_eq!(counts.messages, 4);

    let catalog = Catalog::open(&service.silver_dir.path().join("sync")).unwrap();
    let backfill = catalog
        .backfill("lanes", &service.first.group_id, "first")
        .unwrap();
    assert_eq!(backfill.state, "complete");
    assert_eq!(backfill.covered, backfill.total_conversations);

    let freshness: crate::shared::Freshness =
        serde_json::from_value(service.get("freshness", FIRST).await).unwrap();
    assert_eq!(freshness.nodes[0].revision, 4);
    assert!(freshness.nodes[0].views_current);
    service.cleanup().await;
}
