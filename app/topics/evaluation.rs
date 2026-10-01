//! Read-only real-history diagnostic; private reports never enter stdout/traces.
use super::{AssignmentModel, Settings, TopicId, birch::Tree, labels, normalize};
use crate::provider::Provider;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

struct Thread {
    provider: Provider,
    title: String,
    vectors: Vec<Vec<f64>>,
    words: labels::Terms,
}

#[test]
fn corpus_reads_canonical_fallback_and_deduplicates_within_threads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("topics.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE conversations(id TEXT,provider TEXT,title TEXT);
         CREATE TABLE messages(id TEXT,conversation_id TEXT,ordinal INTEGER,
             content TEXT,search_projection TEXT);
         CREATE TABLE message_embeddings(message_id TEXT,vector BLOB,generation TEXT);
         INSERT INTO conversations VALUES ('a','codex',NULL),('b','claude-code','title');
         INSERT INTO messages VALUES ('a1','a',0,'database',NULL),
             ('a2','a',1,'hidden','database'),('b1','b',0,'database',NULL);",
        )
        .unwrap();
    let mut vector = vec![0_u8; 384];
    vector[0] = 127;
    for id in ["a1", "a2", "b1"] {
        connection
            .execute(
                "INSERT INTO message_embeddings VALUES (?1,?2,'model-v1')",
                rusqlite::params![id, vector],
            )
            .unwrap();
    }
    drop(connection);
    let (threads, generation) = corpus(&path);
    assert_eq!(generation, "model-v1");
    assert_eq!(threads.len(), 2);
    assert_eq!(threads["a"].vectors.len(), 1);
    assert_eq!(threads["b"].vectors.len(), 1);
    assert_eq!(threads["a"].provider, Provider::Codex);
    assert_eq!(threads["b"].provider, Provider::ClaudeCode);
    assert_eq!(threads["a"].words["database"], 1);
    assert!(!threads["a"].words.contains_key("hidden"));
}

fn corpus(path: &std::path::Path) -> (BTreeMap<String, Thread>, String) {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut statement = connection.prepare("SELECT c.id,c.provider,c.title,e.vector,e.generation,COALESCE(m.search_projection,m.content) FROM message_embeddings e JOIN messages m ON m.id=e.message_id JOIN conversations c ON c.id=m.conversation_id ORDER BY c.id,m.ordinal,m.id").unwrap();
    let mut rows = statement.query([]).unwrap();
    let mut threads: BTreeMap<String, Thread> = BTreeMap::new();
    let mut generations = BTreeSet::new();
    let mut seen = BTreeSet::new();
    while let Some(row) = rows.next().unwrap() {
        let id: String = row.get(0).unwrap();
        let provider_name: String = row.get(1).unwrap();
        let provider = *Provider::ALL
            .iter()
            .find(|provider| provider.as_str() == provider_name)
            .expect("known stored provider");
        let title: Option<String> = row.get(2).unwrap();
        let raw: Vec<u8> = row.get(3).unwrap();
        let generation: String = row.get(4).unwrap();
        let text: String = row.get(5).unwrap();
        generations.insert(generation);
        let fingerprint = *blake3::hash(text.as_bytes()).as_bytes();
        if !seen.insert((id.clone(), fingerprint)) {
            continue;
        }
        let vector = normalize(
            &raw.into_iter()
                .map(|x| f64::from(x.cast_signed()))
                .collect::<Vec<_>>(),
            384,
        )
        .unwrap();
        let thread = threads.entry(id).or_insert_with(|| Thread {
            provider,
            title: title.unwrap_or_default(),
            vectors: Vec::new(),
            words: labels::Terms::new(),
        });
        thread.vectors.push(vector);
        for word in labels::terms(&text).into_keys() {
            thread.words.insert(word, 1);
        }
    }
    assert_eq!(generations.len(), 1, "do not mix embedding generations");
    (threads, generations.into_iter().next().unwrap())
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    ThreadMean,
    Segments,
}
fn mean(vectors: &[Vec<f64>]) -> Vec<f64> {
    let mut sum = vec![0.0; 384];
    for vector in vectors {
        for (sum, value) in sum.iter_mut().zip(vector) {
            *sum += value;
        }
    }
    normalize(&sum, 384).unwrap()
}

#[derive(Serialize)]
struct Cluster {
    id: TopicId,
    samples: u64,
    threads: usize,
    providers: BTreeMap<Provider, usize>,
    terms: Vec<labels::Term>,
    examples: Vec<String>,
}
#[derive(Serialize)]
struct Report {
    mode: Mode,
    radius: f64,
    reversed: bool,
    generation: String,
    dimensions: usize,
    threads: usize,
    samples: usize,
    duplicates_across_threads: usize,
    topics: usize,
    cf_depth: usize,
    fit_ms: f64,
    assignment_ms: f64,
    label_ms: f64,
    assigned_threads: usize,
    overlapping_threads: usize,
    maximum_topics_per_thread: usize,
    clusters: Vec<Cluster>,
    memberships: BTreeMap<String, BTreeSet<TopicId>>,
}

#[derive(Serialize)]
struct RejectedRun {
    mode: Mode,
    radius: f64,
    reversed: bool,
    error: crate::AppError,
}

fn label_clusters(
    threads: &BTreeMap<String, Thread>,
    memberships: &BTreeMap<String, BTreeSet<TopicId>>,
    leaves: &[(TopicId, Vec<f64>, u64)],
) -> Vec<Cluster> {
    let mut classes = BTreeMap::new();
    let mut members: BTreeMap<TopicId, Vec<&Thread>> = BTreeMap::new();
    for (id, topics) in memberships {
        for topic in topics {
            let words = classes.entry(*topic).or_insert_with(labels::Terms::new);
            for word in threads[id].words.keys() {
                *words.entry(word.clone()).or_default() += 1;
            }
            members.entry(*topic).or_default().push(&threads[id]);
        }
    }
    let mut labels = labels::labels(&classes, 8);
    leaves
        .iter()
        .map(|(id, _, count)| {
            let members = members.remove(id).unwrap_or_default();
            let mut providers = BTreeMap::new();
            for member in &members {
                *providers.entry(member.provider).or_default() += 1;
            }
            Cluster {
                id: *id,
                samples: *count,
                threads: members.len(),
                providers,
                terms: labels.remove(id).unwrap_or_default(),
                examples: members.iter().take(6).map(|t| t.title.clone()).collect(),
            }
        })
        .collect()
}

fn run(
    threads: &BTreeMap<String, Thread>,
    generation: &str,
    mode: Mode,
    radius: f64,
    reversed: bool,
) -> Result<Report, crate::AppError> {
    let mut order: Vec<_> = threads.iter().collect();
    if reversed {
        order.reverse();
    }
    let mut tree = Tree::new(Settings {
        dimensions: 384,
        radius,
        branching: 32,
        max_topics: 8192,
    })?;
    let mut seen = BTreeSet::new();
    let mut samples = 0;
    let mut duplicates = 0;
    let start = Instant::now();
    for (_, thread) in &order {
        let vectors = match mode {
            Mode::ThreadMean => vec![mean(&thread.vectors)],
            Mode::Segments => thread.vectors.clone(),
        };
        for vector in vectors {
            let mut hash = blake3::Hasher::new();
            for value in &vector {
                hash.update(&value.to_le_bytes());
            }
            if !seen.insert(*hash.finalize().as_bytes()) {
                duplicates += 1;
                continue;
            }
            tree.insert(&vector)?;
            samples += 1;
        }
    }
    let fit_ms = start.elapsed().as_secs_f64() * 1000.0;
    let leaves = tree.leaves();
    let model = AssignmentModel::new(
        384,
        leaves.iter().map(|(id, v, _)| (*id, v.clone())).collect(),
    )?;
    let start = Instant::now();
    let mut memberships = BTreeMap::new();
    for (id, thread) in threads {
        let vectors = match mode {
            Mode::ThreadMean => vec![mean(&thread.vectors)],
            Mode::Segments => thread.vectors.clone(),
        };
        let mut topics = BTreeSet::new();
        for vector in vectors {
            topics.extend(
                model
                    .assign(&vector, 0.55, 0.9)?
                    .into_iter()
                    .map(|a| a.topic),
            );
        }
        memberships.insert(id.clone(), topics);
    }
    let assignment_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let clusters = label_clusters(threads, &memberships, &leaves);
    let label_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(Report {
        mode,
        radius,
        reversed,
        generation: generation.into(),
        dimensions: 384,
        threads: threads.len(),
        samples,
        duplicates_across_threads: duplicates,
        topics: leaves.len(),
        cf_depth: tree.depth(),
        fit_ms,
        assignment_ms,
        label_ms,
        assigned_threads: memberships.values().filter(|t| !t.is_empty()).count(),
        overlapping_threads: memberships.values().filter(|t| t.len() > 1).count(),
        maximum_topics_per_thread: memberships.values().map(BTreeSet::len).max().unwrap_or(0),
        clusters,
        memberships,
    })
}

#[test]
#[ignore = "manual real-corpus topic benchmark requires read-only SQLite and private report directory"]
fn benchmark_topic_discovery() {
    let database =
        PathBuf::from(std::env::var_os("MASH_TEST_SEARCH_DB").expect("explicit frozen SQLite"));
    let output = PathBuf::from(
        std::env::var_os("MASH_TEST_TOPIC_REPORT_DIR").expect("private report directory"),
    );
    std::fs::create_dir_all(&output).unwrap();
    let (threads, generation) = corpus(&database);
    assert!(!threads.is_empty());
    for mode in [Mode::ThreadMean, Mode::Segments] {
        for radius in [0.4, 0.6] {
            for reversed in [false, true] {
                let path = output.join(format!("{mode:?}-{radius}-{reversed}.json"));
                let report = match run(&threads, &generation, mode, radius, reversed) {
                    Ok(report) => report,
                    Err(error) => {
                        let rejected = RejectedRun {
                            mode,
                            radius,
                            reversed,
                            error,
                        };
                        std::fs::write(path, serde_json::to_vec_pretty(&rejected).unwrap())
                            .unwrap();
                        println!(
                            "{mode:?} radius={radius} reversed={reversed} rejected; see private report"
                        );
                        continue;
                    }
                };
                std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
                println!(
                    "{mode:?} radius={radius} reversed={reversed} threads={} samples={} topics={} fit_ms={:.1} assignment_ms={:.1} overlap={}",
                    report.threads,
                    report.samples,
                    report.topics,
                    report.fit_ms,
                    report.assignment_ms,
                    report.overlapping_threads
                );
            }
        }
    }
}
