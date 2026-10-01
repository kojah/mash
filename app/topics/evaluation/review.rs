//! Explicit, bounded private diagnostics for extraction/shortlist/ranking loss.
use super::{BTreeMap, BTreeSet, Mode, PathBuf, Report, TopicId, labels, names, provenance};
use crate::AppError;
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Probes {
    terms: Vec<String>,
}

impl Probes {
    fn parse(text: &str) -> Result<Self, AppError> {
        let probes: Self =
            toml::from_str(text).map_err(|_| AppError::usage("invalid topic label probes"))?;
        if !(1..=32).contains(&probes.terms.len())
            || probes.terms.iter().collect::<BTreeSet<_>>().len() != probes.terms.len()
            || probes
                .terms
                .iter()
                .any(|t| t.trim() != t || t.is_empty() || t.len() > 192)
        {
            return Err(AppError::usage("invalid topic label probe bounds"));
        }
        Ok(probes)
    }

    fn load() -> Self {
        let path = std::env::var_os("MASH_TEST_TOPIC_LABEL_PROBES")
            .expect("explicit private label probes");
        let mut text = String::new();
        std::fs::File::open(path)
            .unwrap()
            .take(16_385)
            .read_to_string(&mut text)
            .unwrap();
        assert!(text.len() <= 16_384, "label probe input cap");
        Self::parse(&text).unwrap()
    }
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum State {
    AbsentFromSupport,
    ExcludedFromShortlist,
    Shortlisted { rank: usize, selected: bool },
}

#[derive(Serialize)]
struct Probe {
    text: String,
    supporting_threads: u64,
    state: State,
}

#[derive(Serialize)]
struct Topic {
    id: TopicId,
    supporting_terms: usize,
    probes: Vec<Probe>,
    candidates: Vec<labels::ranking::SemanticTerm>,
    selected: Vec<String>,
}

#[derive(Serialize)]
struct Capture {
    schema_version: u32,
    generation: String,
    label_text: labels::TextMode,
    label_shortlist: labels::shortlist::Policy,
    mode: Mode,
    radius: f64,
    reversed: bool,
    held_out: bool,
    threads: BTreeMap<String, BTreeSet<TopicId>>,
    topics: Vec<Topic>,
}

fn capture(
    report: &Report,
    selection: &provenance::Selection,
    probes: &Probes,
    labeler: &names::Labeler,
) -> Result<Capture, AppError> {
    if report.supporting_words.len() != report.memberships.len() {
        return Err(AppError::usage(
            "label review requires complete supporting words",
        ));
    }
    let collector = provenance::Collector::new(selection, report.mode);
    let threads: BTreeMap<_, _> = report
        .memberships
        .iter()
        .filter(|(id, _)| collector.selected(id))
        .map(|(id, topics)| (id.clone(), topics.clone()))
        .collect();
    let ids: BTreeSet<_> = threads.values().flatten().copied().collect();
    if ids.len() > 64 {
        return Err(AppError::usage("label review exceeds 64 topic cap"));
    }
    let mut topics = Vec::new();
    for cluster in report.clusters.iter().filter(|c| ids.contains(&c.id)) {
        let candidates = labeler.inspect(&cluster.name)?;
        let selected: Vec<_> = cluster
            .name
            .semantic_terms
            .as_ref()
            .into_iter()
            .flatten()
            .map(|t| t.text.clone())
            .collect();
        let mut words = labels::Terms::new();
        for supporting in report
            .supporting_words
            .values()
            .filter_map(|m| m.get(&cluster.id))
        {
            for (word, count) in supporting {
                *words.entry(word.clone()).or_default() += count;
                if words.len() > 8192 {
                    return Err(AppError::usage(
                        "label review support vocabulary exceeds cap",
                    ));
                }
            }
        }
        let probes = probes
            .terms
            .iter()
            .map(|text| {
                let supporting_threads = words.get(text).copied().unwrap_or(0);
                let state = if supporting_threads == 0 {
                    State::AbsentFromSupport
                } else if let Some(rank) = candidates.iter().position(|t| &t.text == text) {
                    State::Shortlisted {
                        rank,
                        selected: selected.contains(text),
                    }
                } else {
                    State::ExcludedFromShortlist
                };
                Probe {
                    text: text.clone(),
                    supporting_threads,
                    state,
                }
            })
            .collect();
        topics.push(Topic {
            id: cluster.id,
            supporting_terms: words.len(),
            probes,
            candidates,
            selected,
        });
    }
    Ok(Capture {
        schema_version: 1,
        generation: report.generation.clone(),
        label_text: labels::text_mode(),
        label_shortlist: labels::shortlist::policy(),
        mode: report.mode,
        radius: report.radius,
        reversed: report.reversed,
        held_out: report.held_out,
        threads,
        topics,
    })
}

#[test]
#[ignore = "explicit frozen corpus, installed models, private selected threads and label probes required"]
fn benchmark_label_candidates() {
    let database =
        PathBuf::from(std::env::var_os("MASH_TEST_SEARCH_DB").expect("explicit frozen SQLite"));
    let output = PathBuf::from(
        std::env::var_os("MASH_TEST_TOPIC_REPORT_DIR").expect("private report directory"),
    );
    let probes = Probes::load();
    let selection = provenance::Selection::load()
        .unwrap()
        .expect("explicit private review threads");
    let (threads, generation, labeler) = super::prepare(&database, &output);
    selection.validate(&threads).unwrap();
    let mut labeler = labeler.expect("explicit compatible installed label model");
    let held_out = std::env::var_os("MASH_TEST_TOPIC_HOLDOUT").is_some();
    for reversed in [false, true] {
        let mut report = super::run(
            &threads,
            &generation,
            Mode::UserRepresentatives,
            0.4,
            reversed,
            held_out,
            None,
        )
        .unwrap();
        let mut names: Vec<_> = report.clusters.iter_mut().map(|c| &mut c.name).collect();
        labeler.apply(&mut names).unwrap();
        let capture = capture(&report, &selection, &probes, &labeler).unwrap();
        std::fs::write(
            output.join(format!("label-review-{reversed}.json")),
            serde_json::to_vec_pretty(&capture).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn probe_inputs_fail_closed_on_unknown_duplicate_or_oversized_requests() {
    assert!(Probes::parse("terms=['deadlock','runtime confirmation']").is_ok());
    for text in [
        "terms=[]",
        "terms=['a','a']",
        "terms=['']",
        "terms=[' padded']",
        "terms=['a']\nunknown=true",
    ] {
        assert!(Probes::parse(text).is_err());
    }
    assert!(Probes::parse(&format!("terms=['{}']", "x".repeat(193))).is_err());
}
