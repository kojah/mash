//! Read-only real-history diagnostic; private reports never enter stdout/traces.
mod ancestry;
mod anchored;
mod anchors;
mod assignments;
mod context;
mod discovery;
mod input;
mod names;
mod partition;
mod provenance;
mod representatives;
mod review;
use super::segments::{Builder, Segment};
use super::{AssignmentModel, TopicId, labels, normalize};
use crate::provider::Provider;
use crate::role::Role;
use partition::held_out;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone)]
struct Thread {
    source_id: Option<partition::SourceId>,
    provider: Provider,
    title: String,
    vectors: Vec<Vec<f64>>,
    words: labels::Terms,
    user_messages: Vec<Segment>,
    user_sources: Vec<provenance::Source>,
    ignored_user_messages: usize,
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
             content TEXT,search_projection TEXT,role TEXT);
         CREATE TABLE message_embeddings(message_id TEXT,vector BLOB,generation TEXT);
         INSERT INTO conversations VALUES ('a','codex',NULL),('b','claude-code','title'),('c','codex',NULL);
         INSERT INTO messages VALUES ('a1','a',0,'database',NULL,'user'),
             ('a2','a',1,'hidden','database','user'),('b1','b',0,'database',NULL,'assistant'),('c1','c',0,'Hello!',NULL,'user'),
             ('a3','a',2,'<task-notification><task-id>fixture</task-id><summary>background event</summary></task-notification>',NULL,'user');",
        )
        .unwrap();
    let mut vector = vec![0_u8; 384];
    vector[0] = 127;
    for id in ["a1", "a2", "b1", "c1", "a3"] {
        connection
            .execute(
                "INSERT INTO message_embeddings VALUES (?1,?2,'model-v1')",
                rusqlite::params![id, vector],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO messages VALUES ('a4','a',3,'unembedded request',NULL,'user')",
            [],
        )
        .unwrap();
    drop(connection);
    let (threads, generation) = corpus(&path);
    assert_eq!(generation, "model-v1");
    assert_eq!(threads.len(), 3);
    assert_eq!(threads["a"].vectors.len(), 2);
    assert_eq!(threads["b"].vectors.len(), 1);
    assert_eq!(threads["a"].provider, Provider::Codex);
    assert_eq!(threads["b"].source_id, None);
    assert_eq!(threads["a"].source_id, partition::identity(&[
        "database".into(), "hidden".into(),
        "<task-notification><task-id>fixture</task-id><summary>background event</summary></task-notification>".into(),
        "unembedded request".into(),
    ]));
    assert_eq!(threads["b"].provider, Provider::ClaudeCode);
    assert_eq!(threads["a"].words["database"], 1);
    assert!(!threads["a"].words.contains_key("hidden"));
    assert_eq!(threads["a"].user_messages.len(), 1);
    assert_eq!(threads["a"].ignored_user_messages, 1);
    assert!(threads["a"].words.contains_key("notification")); // canonical baseline retained
    assert!(
        !threads["a"].user_messages[0]
            .words
            .contains_key("notification")
    );
    assert!(threads["b"].user_messages.is_empty());
    assert!(threads["c"].user_messages.is_empty());
    assert_eq!(threads["c"].ignored_user_messages, 1);
    assert_eq!(threads["c"].vectors.len(), 1); // canonical input remains available
}

fn corpus(path: &std::path::Path) -> (BTreeMap<String, Thread>, String) {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut statement = connection.prepare("SELECT c.id,c.provider,c.title,e.vector,e.generation,COALESCE(m.search_projection,m.content),m.role,m.id AS message_id,m.ordinal,m.content AS canonical_content,m.search_projection IS NOT NULL AS stored_projection FROM message_embeddings e JOIN messages m ON m.id=e.message_id JOIN conversations c ON c.id=m.conversation_id ORDER BY c.id,m.ordinal,m.id").unwrap();
    let mut rows = statement.query([]).unwrap();
    let mut threads: BTreeMap<String, Thread> = BTreeMap::new();
    let mut generations = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let phrases = std::env::var_os("MASH_TEST_TOPIC_PHRASES").is_some();
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
        let role: Role = row.get(6).unwrap();
        generations.insert(generation);
        let fingerprint = *blake3::hash(text.as_bytes()).as_bytes();
        if !seen.insert((id.clone(), role.as_str().to_owned(), fingerprint)) {
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
            source_id: None,
            provider,
            title: title.unwrap_or_default(),
            vectors: Vec::new(),
            words: labels::Terms::new(),
            user_messages: Vec::new(),
            user_sources: Vec::new(),
            ignored_user_messages: 0,
        });
        if role == Role::User && !labels::substantive(&text) {
            thread.ignored_user_messages += 1;
        } else if role == Role::User {
            thread
                .user_sources
                .push(provenance::Source::stored(row, text.clone()).unwrap());
            thread.user_messages.push(Segment {
                vector: vector.clone(),
                words: if phrases {
                    labels::candidates(&text)
                } else {
                    labels::terms(&text)
                },
                messages: 1,
            });
        }
        thread.vectors.push(vector);
        for word in labels::terms(&text).into_keys() {
            thread.words.insert(word, 1);
        }
    }
    partition::capture(&connection, &mut threads);
    assert_eq!(generations.len(), 1, "do not mix embedding generations");
    (threads, generations.into_iter().next().unwrap())
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    ThreadMean,
    Segments,
    UserMean,
    UserSegments,
    UserGroupedLoose,
    UserGroupedStrict,
    UserAnchored,
    UserRepresentatives,
}

impl Mode {
    fn segmented(self) -> bool {
        matches!(
            self,
            Self::UserSegments
                | Self::UserGroupedLoose
                | Self::UserGroupedStrict
                | Self::UserAnchored
                | Self::UserRepresentatives
        )
    }
    fn group_similarity(self) -> Option<f64> {
        match self {
            Self::UserGroupedLoose => Some(0.45),
            Self::UserGroupedStrict => Some(0.6),
            _ => None,
        }
    }
}

fn thread_samples(thread: &Thread, mode: Mode) -> Vec<Segment> {
    match mode {
        Mode::ThreadMean => vec![Segment {
            vector: mean(&thread.vectors),
            words: thread.words.clone(),
            messages: thread.vectors.len(),
        }],
        Mode::Segments => thread
            .vectors
            .iter()
            .map(|vector| Segment {
                vector: vector.clone(),
                words: labels::Terms::new(),
                messages: 1,
            })
            .collect(),
        Mode::UserMean => {
            if thread.user_messages.is_empty() {
                return Vec::new();
            }
            let vectors: Vec<_> = thread
                .user_messages
                .iter()
                .map(|m| m.vector.clone())
                .collect();
            let mut words = labels::Terms::new();
            for message in &thread.user_messages {
                for word in message.words.keys() {
                    words.insert(word.clone(), 1);
                }
            }
            vec![Segment {
                vector: mean(&vectors),
                words,
                messages: vectors.len(),
            }]
        }
        Mode::UserSegments
        | Mode::UserGroupedLoose
        | Mode::UserGroupedStrict
        | Mode::UserAnchored
        | Mode::UserRepresentatives => {
            let mut builder = Builder::new(384, 8, 0.75).unwrap();
            let mut output = Vec::new();
            for message in &thread.user_messages {
                if let Some(segment) = builder.push(&message.vector, &message.words).unwrap() {
                    output.push(segment);
                }
            }
            output.extend(builder.finish().unwrap());
            output
        }
    }
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

#[test]
fn held_out_copied_threads_do_not_train_the_discovery_model() {
    let thread = (0..384)
        .map(|axis| {
            let mut vector = vec![0.0; 384];
            vector[axis] = 1.0;
            Thread {
                source_id: partition::identity(&[format!("database source {axis}")]),
                provider: Provider::Codex,
                title: String::new(),
                vectors: vec![vector.clone()],
                words: labels::terms("database"),
                user_messages: vec![Segment {
                    vector,
                    words: labels::terms("database"),
                    messages: 1,
                }],
                user_sources: vec!["database".into()],
                ignored_user_messages: 0,
            }
        })
        .find(held_out)
        .unwrap();
    let threads = BTreeMap::from([("first".into(), thread.clone()), ("copy".into(), thread)]);
    let held = run(&threads, "model-v1", Mode::UserMean, 0.4, false, true, None).unwrap();
    assert_eq!(held.held_out_threads, 2);
    assert_eq!(held.samples, 0);
    assert_eq!(held.topics, 0);
    assert_eq!(held.held_out_assigned_threads, 0);
    let trained = run(
        &threads,
        "model-v1",
        Mode::UserMean,
        0.4,
        false,
        false,
        None,
    )
    .unwrap();
    assert_eq!(trained.samples, 1);
    assert_eq!(trained.assigned_threads, 2);
}

#[derive(Serialize)]
struct Cluster {
    id: TopicId,
    samples: u64,
    threads: usize,
    providers: BTreeMap<Provider, usize>,
    #[serde(flatten)]
    name: names::Name,
    examples: Vec<String>,
}
#[derive(Serialize)]
struct Report {
    runtime_context_policy: input::runtime_context::Policy,
    discovery_algorithm: discovery::Algorithm,
    discovery_input_fingerprint: String,
    extraction_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    grinch: Option<super::grinch::topology::Snapshot>,
    mode: Mode,
    label_representation_policy: representatives::Policy,
    label_text: labels::TextMode,
    label_discourse: labels::DiscourseMode,
    label_shortlist: labels::shortlist::Policy,
    radius: f64,
    routing: super::birch::Routing,
    assignment_weighting: assignments::Weighting,
    reversed: bool,
    generation: String,
    dimensions: usize,
    held_out: bool,
    partition: partition::Summary,
    held_out_threads: usize,
    held_out_assigned_threads: usize,
    threads: usize,
    ignored_user_messages: usize,
    samples: usize,
    duplicates_across_threads: usize,
    topics: usize,
    microtopics: usize,
    group_similarity: Option<f64>,
    grouping_ms: f64,
    cf_depth: usize,
    fit_ms: f64,
    assignment_ms: f64,
    label_ms: f64,
    semantic_labels: Option<names::Summary>,
    ancestry: Option<ancestry::Report>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assignment_evidence: Option<provenance::Report>,
    assigned_threads: usize,
    overlapping_threads: usize,
    maximum_topics_per_thread: usize,
    clusters: Vec<Cluster>,
    memberships: BTreeMap<String, BTreeSet<TopicId>>,
    #[serde(skip)]
    supporting_words: BTreeMap<String, BTreeMap<TopicId, labels::Terms>>,
}

#[derive(Serialize)]
struct RejectedRun {
    runtime_context_policy: input::runtime_context::Policy,
    discovery_algorithm: discovery::Algorithm,
    mode: Mode,
    label_representation_policy: representatives::Policy,
    label_text: labels::TextMode,
    label_discourse: labels::DiscourseMode,
    label_shortlist: labels::shortlist::Policy,
    radius: f64,
    routing: super::birch::Routing,
    assignment_weighting: assignments::Weighting,
    reversed: bool,
    error: crate::AppError,
}

fn label_clusters(
    threads: &BTreeMap<String, Thread>,
    memberships: &BTreeMap<String, BTreeSet<TopicId>>,
    leaves: &[(TopicId, Vec<f64>, u64)],
    classes: &BTreeMap<TopicId, labels::Terms>,
) -> Vec<Cluster> {
    let mut members: BTreeMap<TopicId, Vec<&Thread>> = BTreeMap::new();
    for (id, topics) in memberships {
        for topic in topics {
            members.entry(*topic).or_default().push(&threads[id]);
        }
    }
    let mut labels = labels::labels(classes, 64);
    leaves
        .iter()
        .map(|(id, center, count)| {
            let members = members.remove(id).unwrap_or_default();
            let mut providers = BTreeMap::new();
            for member in &members {
                *providers.entry(member.provider).or_default() += 1;
            }
            let candidates = labels.remove(id).unwrap_or_default();
            Cluster {
                id: *id,
                samples: *count,
                threads: members.len(),
                providers,
                name: names::Name::new(center.clone(), candidates),
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
    holdout: bool,
    evidence_selection: Option<&provenance::Selection>,
) -> Result<Report, crate::AppError> {
    let discovery::Fit {
        leaves,
        depth,
        grinch,
        algorithm,
        input_fingerprint,
        extraction_ms,
        routing,
        samples,
        duplicates,
        elapsed_ms: fit_ms,
    } = discovery::fit(threads, mode, radius, reversed, holdout)?;
    let microtopics = leaves.len();
    let group_similarity = mode.group_similarity();
    let start = Instant::now();
    let leaves = match group_similarity {
        Some(minimum) => super::grouping::group(384, &leaves, minimum)?,
        None => leaves,
    };
    let grouping_ms = start.elapsed().as_secs_f64() * 1000.0;
    let model = AssignmentModel::new(
        384,
        leaves.iter().map(|(id, v, _)| (*id, v.clone())).collect(),
    )?;
    let start = Instant::now();
    let assignment_weighting = assignments::weighting(mode);
    let assignments::Assignments {
        memberships,
        classes,
        supporting_words,
        supporting_segments,
        assignment_evidence,
    } = assignments::assign(
        threads,
        &model,
        mode,
        assignment_weighting,
        evidence_selection.map(|s| provenance::Collector::new(s, mode)),
    )?;
    let assignment_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let mut clusters = label_clusters(threads, &memberships, &leaves, &classes);
    if representatives::enabled() {
        representatives::apply(threads, &supporting_segments, mode, holdout, &mut clusters)?;
    }
    let label_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(Report {
        runtime_context_policy: input::runtime_context::policy(),
        discovery_algorithm: algorithm,
        discovery_input_fingerprint: input_fingerprint,
        extraction_ms,
        grinch,
        label_representation_policy: representatives::policy(),
        label_text: labels::text_mode(),
        label_discourse: labels::discourse_mode(),
        label_shortlist: labels::shortlist::policy(),
        mode,
        radius,
        routing,
        assignment_weighting,
        reversed,
        generation: generation.into(),
        dimensions: 384,
        held_out: holdout,
        partition: partition::summarize(threads, &memberships, mode, holdout),
        held_out_threads: threads.values().filter(|t| holdout && held_out(t)).count(),
        held_out_assigned_threads: memberships
            .iter()
            .filter(|(id, topics)| holdout && held_out(&threads[*id]) && !topics.is_empty())
            .count(),
        threads: threads.len(),
        ignored_user_messages: threads.values().map(|t| t.ignored_user_messages).sum(),
        samples,
        duplicates_across_threads: duplicates,
        topics: leaves.len(),
        microtopics,
        group_similarity,
        grouping_ms,
        cf_depth: depth,
        fit_ms,
        assignment_ms,
        label_ms,
        semantic_labels: None,
        ancestry: None,
        assignment_evidence,
        assigned_threads: memberships.values().filter(|t| !t.is_empty()).count(),
        overlapping_threads: memberships.values().filter(|t| t.len() > 1).count(),
        maximum_topics_per_thread: memberships.values().map(BTreeSet::len).max().unwrap_or(0),
        clusters,
        memberships,
        supporting_words,
    })
}

fn routing() -> super::birch::Routing {
    if std::env::var_os("MASH_TEST_TOPIC_EXACT_ROUTING").is_some() {
        super::birch::Routing::ExactLeaf
    } else {
        super::birch::Routing::BranchCentroid
    }
}

fn enrich(
    report: &mut Report,
    threads: &BTreeMap<String, Thread>,
    mut labeler: Option<&mut names::Labeler>,
) -> Result<(), crate::AppError> {
    if let Some(labeler) = labeler.as_mut() {
        let mut names: Vec<_> = report.clusters.iter_mut().map(|c| &mut c.name).collect();
        report.semantic_labels = Some(labeler.apply(&mut names)?);
    }
    if std::env::var_os("MASH_TEST_TOPIC_HIERARCHY").is_some() {
        let mut ancestry = ancestry::build(
            threads,
            &report.clusters,
            &report.memberships,
            &report.supporting_words,
        )?;
        report.supporting_words.clear();
        if let Some(labeler) = labeler {
            ancestry.label(labeler)?;
        }
        println!(
            "hierarchy groups={} depth={} build_ms={:.1} inherit_ms={:.1}",
            ancestry.groups, ancestry.maximum_depth, ancestry.build_ms, ancestry.inheritance_ms
        );
        report.ancestry = Some(ancestry);
    }
    Ok(())
}

#[test]
#[ignore = "manual real-corpus topic benchmark requires read-only SQLite and private report directory"]
fn benchmark_topic_discovery() {
    let database =
        PathBuf::from(std::env::var_os("MASH_TEST_SEARCH_DB").expect("explicit frozen SQLite"));
    let output = PathBuf::from(
        std::env::var_os("MASH_TEST_TOPIC_REPORT_DIR").expect("private report directory"),
    );
    let (threads, generation, mut labeler) = prepare(&database, &output);
    let evidence_selection = provenance::Selection::load().unwrap();
    if let Some(selection) = &evidence_selection {
        selection.validate(&threads).unwrap();
    }
    assert!(!threads.is_empty());
    let holdout = std::env::var_os("MASH_TEST_TOPIC_HOLDOUT").is_some();
    let modes: &[Mode] = if std::env::var_os("MASH_TEST_TOPIC_REPRESENTATIVES").is_some() {
        &[Mode::UserRepresentatives, Mode::UserAnchored]
    } else if std::env::var_os("MASH_TEST_TOPIC_ANCHORED").is_some() {
        &[Mode::UserAnchored]
    } else if std::env::var_os("MASH_TEST_TOPIC_GROUPING").is_some() {
        &[Mode::UserGroupedLoose, Mode::UserGroupedStrict]
    } else if std::env::var_os("MASH_TEST_TOPIC_BASELINES").is_some() {
        &[Mode::ThreadMean, Mode::Segments]
    } else {
        &[Mode::UserMean, Mode::UserSegments]
    };
    for &mode in modes {
        for radius in [0.4, 0.6] {
            for reversed in [false, true] {
                let path = output.join(format!("{mode:?}-{radius}-{reversed}.json"));
                let mut report = match run(
                    &threads,
                    &generation,
                    mode,
                    radius,
                    reversed,
                    holdout,
                    evidence_selection.as_ref(),
                ) {
                    Ok(report) => report,
                    Err(error) => {
                        let rejected = RejectedRun {
                            runtime_context_policy: input::runtime_context::policy(),
                            discovery_algorithm: discovery::algorithm(),
                            label_representation_policy: representatives::policy(),
                            label_text: labels::text_mode(),
                            label_discourse: labels::discourse_mode(),
                            label_shortlist: labels::shortlist::policy(),
                            mode,
                            radius,
                            routing: routing(),
                            assignment_weighting: assignments::weighting(mode),
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
                enrich(&mut report, &threads, labeler.as_mut()).unwrap();
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

fn prepare(
    database: &std::path::Path,
    output: &std::path::Path,
) -> (BTreeMap<String, Thread>, String, Option<names::Labeler>) {
    std::fs::create_dir_all(output).unwrap();
    let (mut threads, generation) = corpus(database);
    let mut labeler = names::Labeler::load(&generation).unwrap();
    if std::env::var_os("MASH_TEST_TOPIC_CLEAN_INPUT").is_some() {
        let summary = input::clean(&mut threads, labeler.as_mut()).unwrap();
        std::fs::write(
            output.join("input-summary.json"),
            serde_json::to_vec_pretty(&summary).unwrap(),
        )
        .unwrap();
    }
    if std::env::var_os("MASH_TEST_TOPIC_CONTEXT").is_some() {
        let summary = context::enrich(database, &mut threads, labeler.as_mut()).unwrap();
        std::fs::write(
            output.join("context-summary.json"),
            serde_json::to_vec_pretty(&summary).unwrap(),
        )
        .unwrap();
    }
    (threads, generation, labeler)
}
