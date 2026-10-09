//! Explicit phase benchmark; fixture creation and RSS measurement run separately.
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

struct PriorState {
    fingerprint: String,
    ordinal: i64,
    search_text: String,
}

fn digest<'a>(rows: impl Iterator<Item = (&'a str, i64, &'a str, blake3::Hash)>) -> String {
    let mut hasher = blake3::Hasher::new();
    for (id, ordinal, fingerprint, text) in rows {
        update_hash(&mut hasher, id);
        hasher.update(&ordinal.to_le_bytes());
        update_hash(&mut hasher, fingerprint);
        hasher.update(text.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

#[test]
#[ignore = "explicit MASH_BENCHMARK_DB and MASH_BENCHMARK_STATE_MODE; measure RSS outside Cargo"]
fn benchmark_prior_message_state_memory() {
    let database = std::env::var_os("MASH_BENCHMARK_DB").expect("explicit fixture database");
    let mode = match std::env::var("MASH_BENCHMARK_STATE_MODE").as_deref() {
        Ok("baseline") => Mode::Baseline,
        Ok("streamed") => Mode::Streamed,
        _ => panic!("choose baseline or streamed explicitly"),
    };
    let storage = Storage::open_existing(Path::new(&database)).unwrap();
    let started = std::time::Instant::now();
    let (messages, digest) = match mode {
        Mode::Baseline => {
            // The previous production algorithm: collect generated rows, then
            // move the complete searchable strings into the retained state map.
            let states = sql::ConversationMessageState::builder()
                .conversation_id("memory-fixture")
                .build()
                .query_many(&storage.connection)
                .unwrap()
                .into_iter()
                .map(|row| {
                    (
                        row.id,
                        PriorState {
                            fingerprint: row.fingerprint,
                            ordinal: row.ordinal.unwrap(),
                            search_text: row.search_text.unwrap(),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let checksum = digest(states.iter().map(|(id, state)| {
                (
                    id.as_str(),
                    state.ordinal,
                    state.fingerprint.as_str(),
                    blake3::hash(state.search_text.as_bytes()),
                )
            }));
            (states.len(), checksum)
        }
        Mode::Streamed => {
            let states = storage.message_states("memory-fixture").unwrap();
            let checksum = digest(states.iter().map(|(id, state)| {
                (
                    id.as_str(),
                    state.ordinal,
                    state.fingerprint.as_str(),
                    state.search_text_hash,
                )
            }));
            (states.len(), checksum)
        }
    };
    assert!(messages > 0, "benchmark fixture must not be empty");
    println!(
        "{}",
        serde_json::to_string(&Measurement {
            mode,
            messages,
            digest,
            elapsed_milliseconds: crate::progress::milliseconds(started),
        })
        .unwrap()
    );
}
