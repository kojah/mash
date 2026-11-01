//! Deterministic protocol benchmark: legacy NDJSON upload versus typed
//! Protobuf plus unconditional zstd publication pieces.
//!
//! This measures the production codecs on one representative batch. It does
//! not include TCP, Qdrant, or model inference, and it is not a deployment
//! claim. The legacy HTTP upload body was uncompressed NDJSON; whole-batch
//! zstd of that same NDJSON is reported only to separate encoding from
//! compression gains.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "benchmark metrics and deterministic corpus generator use bounded values"
)]

use super::wire;
use crate::shared::protocol::{Batch, ConversationRecord, Event, MessageRecord};
use std::fmt::Write as _;
use std::time::{Duration, Instant};

const CONVERSATIONS: usize = 48;
const MESSAGES_PER_CONVERSATION: usize = 16;
const ITERATIONS: usize = 12;
const LINK_MBPS: &[f64] = &[10.0, 50.0, 200.0];

/// Deterministic but varied text; identical bodies would let zstd collapse the
/// whole batch and overstate compression gains.
#[allow(
    clippy::too_many_lines,
    reason = "the word and path tables are the corpus definition"
)]
fn body(seed: u64, tokens: usize) -> String {
    const WORDS: &[&str] = &[
        "canonical",
        "publication",
        "checkpoint",
        "resume",
        "conversation",
        "embedding",
        "revision",
        "transaction",
        "silver",
        "views",
        "journal",
        "operation",
        "schema",
        "vector",
        "fingerprint",
        "manifest",
        "durable",
        "bounded",
        "compression",
        "acknowledgement",
        "server",
        "client",
        "socket",
        "frame",
        "protobuf",
        "zstd",
        "handshake",
        "credit",
        "sequence",
        "digest",
        "receipt",
        "catalog",
        "spool",
        "chunk",
        "deadline",
        "capacity",
        "admission",
        "watcher",
        "connector",
        "attachment",
        "tombstone",
        "certificate",
        "native",
        "projection",
        "reranking",
        "sparse",
        "dense",
        "qdrant",
        "sqlite",
        "fts",
        "provider",
        "ordinal",
        "generation",
        "context",
        "refactor",
        "diagnose",
        "regression",
        "validation",
        "benchmark",
        "latency",
        "bandwidth",
        "stream",
    ];
    const PATHS: &[&str] = &[
        "app/shared/sync/wire.rs",
        "app/shared/sync/catalog.rs",
        "app/shared/server/sync.rs",
        "app/shared/silver_store/apply.rs",
        "protocol/SyncTransfer.tla",
        "app/shared/publication.rs",
        "crates/mash-query/src/lib.rs",
        "app/storage/silver.sql",
    ];
    let mut state = seed | 1;
    let mut text = String::with_capacity(tokens * 8);
    for index in 0..tokens {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };
        match next() % 32 {
            0 => {
                let _ = write!(text, "`{}`", PATHS[(next() as usize) % PATHS.len()]);
            }
            1 => {
                let _ = write!(text, "{:#010x}", next());
            }
            2 => {
                let _ = write!(text, "v{}.{}.{}", next() % 9, next() % 9, next() % 20);
            }
            _ => text.push_str(WORDS[(next() as usize) % WORDS.len()]),
        }
        if index % 14 == 13 {
            text.push('\n');
        } else {
            text.push(' ');
        }
    }
    text
}

fn vector(seed: u64) -> Vec<f64> {
    let mut state = seed | 1;
    (0..384)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 40) as f64) / ((1u64 << 24) as f64) - 0.5
        })
        .collect()
}

#[allow(
    clippy::too_many_lines,
    reason = "one deterministic corpus definition documents every field the codec carries"
)]
fn corpus() -> Vec<Event> {
    let mut conversations = Vec::with_capacity(CONVERSATIONS);
    let mut messages = Vec::with_capacity(CONVERSATIONS * MESSAGES_PER_CONVERSATION);
    for conversation in 0..CONVERSATIONS {
        let id = format!("conversation-{conversation:03}");
        conversations.push(ConversationRecord {
            enrichment: None,
            id: id.clone(),
            provider: crate::provider::Provider::Codex,
            title: Some(format!("Refactor slice {conversation}")),
            project: Some("/work/mash".into()),
            repository: Some("repo://mash".into()),
            created_at: Some(1_760_000_000 + conversation as i64),
            updated_at: Some(1_760_100_000 + conversation as i64),
        });
        for ordinal in 0..MESSAGES_PER_CONVERSATION {
            let agent = ordinal % 3 != 0;
            let seed = (conversation as u64) << 32 | ordinal as u64;
            let tokens = 90 + (seed as usize * 37) % 150;
            let text = body(seed, tokens);
            messages.push(MessageRecord {
                attachments: if ordinal == 3 {
                    vec![crate::attachments::Metadata {
                        id: format!("attachment-{conversation}-{ordinal}"),
                        name: "patch.diff".into(),
                        media_type: "text/x-diff".into(),
                        availability: crate::attachments::Availability::Retained,
                        bytes: Some(4096),
                        digest: Some("a".repeat(64)),
                        redacted: false,
                    }]
                } else {
                    Vec::new()
                },
                id: format!("message-{conversation:03}-{ordinal:02}"),
                conversation_id: id.clone(),
                ordinal: ordinal as i64,
                context: crate::message_context::MessageContext::Linear,
                role: if agent {
                    crate::role::Role::Assistant
                } else {
                    crate::role::Role::User
                },
                content: text.clone(),
                projection: text,
                created_at: Some(1_760_000_000 + conversation as i64 + ordinal as i64),
                generation: agent.then(|| crate::semantic::SHARED_EMBEDDING_GENERATION.into()),
                vector: agent.then(|| vector(seed ^ 0x0005_deec_e66d)),
                tools: if agent && ordinal % 5 == 1 {
                    crate::tools::Activity {
                        tool_calls: vec![crate::tools::ToolCall {
                            name: Some("shell".into()),
                            call_id: Some(format!("call-{conversation}-{ordinal}")),
                            input: Some(serde_json::json!({
                                "command": format!(
                                    "cargo test --lib shared::sync::case_{conversation}_{ordinal}"
                                ),
                                "workdir": "/home/james/scratch/mash",
                                "timeout_ms": 1000 + ordinal * 250
                            })),
                        }],
                        tool_results: vec![crate::tools::ToolResult {
                            call_id: Some(format!("call-{conversation}-{ordinal}")),
                            is_error: Some(false),
                        }],
                    }
                } else {
                    crate::tools::Activity::default()
                },
            });
        }
    }
    vec![
        Event::Batch(Batch::Conversations {
            records: conversations,
        }),
        Event::Batch(Batch::Messages { records: messages }),
    ]
}

fn legacy(events: &[Event]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for event in events {
        serde_json::to_writer(&mut bytes, event).unwrap();
        bytes.push(b'\n');
    }
    bytes
}

fn legacy_decode(bytes: &[u8]) -> Vec<Event> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect()
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn transfer_seconds(bytes: usize, mbps: f64) -> f64 {
    (bytes as f64 * 8.0) / (mbps * 1_000_000.0)
}

fn records(events: &[Event]) -> Vec<(String, serde_json::Value)> {
    let mut records = Vec::new();
    let mut push = |kind: &str, value: serde_json::Value| records.push((kind.to_owned(), value));
    for event in events {
        match event {
            Event::Batch(Batch::Tombstones { records: batch }) => {
                for record in batch {
                    push("tombstone", serde_json::to_value(record).unwrap());
                }
            }
            Event::Batch(Batch::Conversations { records: batch }) => {
                for record in batch {
                    push("conversation", serde_json::to_value(record).unwrap());
                }
            }
            Event::Batch(Batch::Messages { records: batch }) => {
                for record in batch {
                    push("message", serde_json::to_value(record).unwrap());
                }
            }
            Event::Remove { id } => push("remove", serde_json::json!({"id": id})),
            Event::Start(_) | Event::Finish => {}
        }
    }
    records
}

fn assert_same_records(label: &str, decoded: &[Event], source: &[Event]) {
    let decoded = records(decoded);
    let source = records(source);
    assert_eq!(decoded.len(), source.len(), "{label} record count");
    for (index, ((decoded_kind, decoded), (source_kind, source))) in
        decoded.iter().zip(source.iter()).enumerate()
    {
        assert_eq!(decoded_kind, source_kind, "{label} record {index} kind");
        assert!(
            equivalent(decoded, source),
            "{label} record {index} ({decoded_kind}) differs:\n{decoded}\n{source}"
        );
    }
}

fn equivalent(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    match (left, right) {
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
            match (left.as_f64(), right.as_f64()) {
                (Some(left), Some(right)) => (left - right).abs() <= 1e-12,
                _ => left == right,
            }
        }
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left, right)| equivalent(left, right))
        }
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(key, left)| right.get(key).is_some_and(|right| equivalent(left, right)))
        }
        _ => left == right,
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one benchmark reports every measured and projected metric together"
)]
fn sync_codec_reduces_wire_bytes_and_round_trip_latency() {
    let events = corpus();
    let records = wire::record_count(&events);
    let conversations = match &events[0] {
        Event::Batch(Batch::Conversations { records }) => records.len(),
        _ => panic!("conversation batch"),
    };

    // Warm both paths before timing.
    let old = legacy(&events);
    let new = wire::encode_publication(&events, wire::DEFAULT_MAX_DECODED_BYTES).unwrap();
    let parsed_old = legacy_decode(&old);
    let parsed_new =
        wire::decode_publication(&new.pieces, wire::DEFAULT_MAX_DECODED_BYTES).unwrap();
    // Wire decoding may re-group records into different batch boundaries, so
    // compare the ordered records instead of the batch envelopes.
    assert_same_records("legacy", &parsed_old, &events);
    assert_same_records("sync", &parsed_new, &events);

    let mut old_encode = Vec::new();
    let mut old_decode = Vec::new();
    let mut new_encode = Vec::new();
    let mut new_decode = Vec::new();
    let mut compressed_old = Vec::new();
    for _ in 0..ITERATIONS {
        let started = Instant::now();
        let bytes = legacy(&events);
        old_encode.push(started.elapsed());
        let started = Instant::now();
        let _ = legacy_decode(&bytes);
        old_decode.push(started.elapsed());
        let started = Instant::now();
        compressed_old.push(zstd::stream::encode_all(bytes.as_slice(), 1).unwrap());
        let _ = started.elapsed();

        let started = Instant::now();
        let encoded = wire::encode_publication(&events, wire::DEFAULT_MAX_DECODED_BYTES).unwrap();
        new_encode.push(started.elapsed());
        let started = Instant::now();
        let _ = wire::decode_publication(&encoded.pieces, wire::DEFAULT_MAX_DECODED_BYTES).unwrap();
        new_decode.push(started.elapsed());
    }
    let old_bytes = old.len();
    let new_bytes = new
        .pieces
        .iter()
        .map(|piece| piece.encoded.len())
        .sum::<usize>();
    let old_zstd_bytes = compressed_old.last().unwrap().len();
    let old_encode = median(old_encode);
    let old_decode = median(old_decode);
    let new_encode = median(new_encode);
    let new_decode = median(new_decode);
    let decoded = new.decoded_bytes as usize;

    println!(
        "\nsync codec benchmark: {conversations} conversations, {records} records, {:.2} MiB decoded, {} pieces",
        decoded as f64 / (1024.0 * 1024.0),
        new.pieces.len()
    );
    println!(
        "legacy NDJSON:        {:.2} MiB  encode {:7.2} ms  decode {:7.2} ms  total {:7.2} ms",
        old_bytes as f64 / (1024.0 * 1024.0),
        milliseconds(old_encode),
        milliseconds(old_decode),
        milliseconds(old_encode + old_decode)
    );
    println!(
        "NDJSON + zstd(1):     {:.2} MiB  (conservative legacy baseline, not the old wire)",
        old_zstd_bytes as f64 / (1024.0 * 1024.0)
    );
    println!(
        "Protobuf + zstd(1):   {:.2} MiB  encode {:7.2} ms  decode {:7.2} ms  total {:7.2} ms",
        new_bytes as f64 / (1024.0 * 1024.0),
        milliseconds(new_encode),
        milliseconds(new_decode),
        milliseconds(new_encode + new_decode)
    );
    println!(
        "wire reduction: {:.1}% versus NDJSON, {:.1}% versus NDJSON+zstd ({:.1}x versus NDJSON)",
        100.0 * (1.0 - new_bytes as f64 / old_bytes as f64),
        100.0 * (1.0 - new_bytes as f64 / old_zstd_bytes as f64),
        old_bytes as f64 / new_bytes as f64
    );
    println!(
        "codec latency: {:.1}% lower total encode+decode; compression throughput {:.1} MiB/s",
        100.0
            * (1.0
                - (new_encode + new_decode).as_secs_f64()
                    / (old_encode + old_decode).as_secs_f64()),
        decoded as f64 / (1024.0 * 1024.0) / new_encode.as_secs_f64().max(f64::EPSILON)
    );
    for mbps in LINK_MBPS {
        println!(
            "projected transfer at {mbps:>5.0} Mbit/s: legacy {:6.2} s -> sync {:6.2} s",
            transfer_seconds(old_bytes, *mbps),
            transfer_seconds(new_bytes, *mbps)
        );
    }

    assert!(
        new_bytes * 4 < old_bytes,
        "typed compression should beat NDJSON by >4x on this corpus"
    );
    assert!(
        new_bytes < old_zstd_bytes,
        "Protobuf should also beat compressed NDJSON"
    );
    assert!(
        new_encode + new_decode < old_encode + old_decode,
        "typed round trip should be faster than NDJSON round trip"
    );
}
