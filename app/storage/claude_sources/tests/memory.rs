//! Compare contribution-loading phases; build fixtures and measure RSS externally.
use super::*;

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum Mode {
    Baseline,
    Streamed,
}

#[derive(serde::Serialize)]
struct Measurement {
    mode: Mode,
    messages: usize,
    digest: String,
    elapsed_milliseconds: u64,
}

fn digest(messages: &crate::silver::messages::Messages, lineage: &claude::Lineage) -> String {
    let mut hasher = blake3::Hasher::new();
    for message in messages.rows().unwrap() {
        let message = message.unwrap();
        // Include every canonical field and ancestry, not only retained counts.
        let record = serde_json::to_vec(&(
            &message.id,
            message.ordinal,
            message.context.clone(),
            &message.role,
            &message.content,
            &message.dialogue_text,
            message.created_at,
            lineage.get(&message.id).unwrap(),
        ))
        .unwrap();
        hasher.update(&record);
    }
    hasher.finalize().to_hex().to_string()
}

#[test]
#[ignore = "explicit MASH_BENCHMARK_DB and MASH_BENCHMARK_STATE_MODE; measure RSS outside Cargo"]
fn benchmark_claude_contribution_memory() {
    let database = std::env::var_os("MASH_BENCHMARK_DB").expect("explicit fixture database");
    let mode = match std::env::var("MASH_BENCHMARK_STATE_MODE").as_deref() {
        Ok("baseline") => Mode::Baseline,
        Ok("streamed") => Mode::Streamed,
        _ => panic!("choose baseline or streamed explicitly"),
    };
    let connection =
        rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let started = std::time::Instant::now();
    let (messages, lineage) = match mode {
        Mode::Baseline => {
            // The prior production algorithm: collect all physical payloads
            // before moving rows into native-identity reconciliation.
            let rows = sql::ClaudeContributionMessages::builder()
                .conversation_id("thread")
                .build()
                .query_many(&connection)
                .unwrap();
            claude::reconcile(rows.into_iter().map(|row| {
                Ok((
                    NormalizedMessage {
                        attachments: Vec::new(),
                        id: row.message_id,
                        ordinal: 0,
                        context: crate::message_context::MessageContext::Root,
                        role: row.role.into(),
                        content: row.content,
                        dialogue_text: row.projection,
                        created_at: row.created_at,
                        tools: crate::tools::Activity::default(),
                    },
                    row.parent_id,
                ))
            }))
            .unwrap()
        }
        Mode::Streamed => {
            let conversation = claude_thread(&connection, "thread", None).unwrap().unwrap();
            (conversation.messages, conversation.claude_lineage.unwrap())
        }
    };
    let elapsed_milliseconds = crate::progress::milliseconds(started);
    assert!(!messages.is_empty(), "benchmark fixture must not be empty");
    println!(
        "{}",
        serde_json::to_string(&Measurement {
            mode,
            messages: messages.len(),
            digest: digest(&messages, &lineage),
            elapsed_milliseconds,
        })
        .unwrap()
    );
}
