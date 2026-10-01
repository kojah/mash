//! Semantic label ranking in the same model space; no generated labels or LLM.
use super::Term;
use crate::AppError;
use crate::topics::normalize;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Serialize)]
pub(in crate::topics) struct Weights {
    semantic: f64,
    support: f64,
    tfidf: f64,
}

pub(in crate::topics) const fn weights(policy: super::shortlist::Policy) -> Weights {
    match policy {
        super::shortlist::Policy::Supported => Weights {
            semantic: 0.6,
            support: 0.3,
            tfidf: 0.1,
        },
        super::shortlist::Policy::Tfidf | super::shortlist::Policy::Balanced => Weights {
            semantic: 0.85,
            support: 0.0,
            tfidf: 0.15,
        },
    }
}

#[derive(Serialize)]
pub(crate) struct SemanticTerm {
    pub(crate) text: String,
    pub(crate) tfidf: f64,
    pub(crate) cosine: f64,
    pub(crate) score: f64,
    pub(crate) support: u64,
}

struct Candidate {
    term: SemanticTerm,
    vector: Vec<f64>,
}

fn contained(a: &str, b: &str) -> bool {
    let a: Vec<_> = a.split_whitespace().collect();
    let b: Vec<_> = b.split_whitespace().collect();
    a.len() <= b.len() && b.windows(a.len()).any(|window| window == a)
}

fn prepare(
    center: &[f64],
    candidates: &[Term],
    embeddings: &BTreeMap<String, Vec<f64>>,
    policy: super::shortlist::Policy,
) -> Result<Vec<Candidate>, AppError> {
    if center.is_empty() || center.len() > 2048 || candidates.len() > 64 {
        return Err(AppError::usage("invalid semantic topic label bounds"));
    }
    let center = normalize(center, center.len())?;
    let maximum = candidates.iter().map(|t| t.score).fold(0.0, f64::max);
    let weights = weights(policy);
    let maximum_support = candidates
        .iter()
        .map(|t| t.support)
        .max()
        .unwrap_or(1)
        .max(1);
    let mut ranked = Vec::new();
    for term in candidates {
        if term.text.trim().is_empty()
            || !term.score.is_finite()
            || term.score < 0.0
            || term.support == 0
        {
            return Err(AppError::model("invalid topic label candidate"));
        }
        let vector = embeddings
            .get(&term.text)
            .ok_or_else(|| AppError::model("missing topic label embedding"))?;
        let vector = normalize(vector, center.len())?;
        let cosine = vector
            .iter()
            .zip(&center)
            .map(|(a, b)| a * b)
            .sum::<f64>()
            .clamp(-1.0, 1.0);
        let relative = if maximum > 0.0 {
            term.score / maximum
        } else {
            0.0
        };
        ranked.push(Candidate {
            term: SemanticTerm {
                text: term.text.clone(),
                tfidf: term.score,
                cosine,
                score: {
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "bounded prototype term counters"
                    )]
                    let support = term.support as f64 / maximum_support as f64;
                    weights.semantic * cosine + weights.support * support + weights.tfidf * relative
                },
                support: term.support,
            },
            vector,
        });
    }
    ranked.sort_by(|a, b| {
        b.term
            .score
            .total_cmp(&a.term.score)
            .then(a.term.text.cmp(&b.term.text))
    });
    Ok(ranked)
}

pub(crate) fn inspect(
    center: &[f64],
    candidates: &[Term],
    embeddings: &BTreeMap<String, Vec<f64>>,
) -> Result<Vec<SemanticTerm>, AppError> {
    Ok(
        prepare(center, candidates, embeddings, super::shortlist::policy())?
            .into_iter()
            .map(|c| c.term)
            .collect(),
    )
}

pub(crate) fn rank(
    center: &[f64],
    candidates: &[Term],
    embeddings: &BTreeMap<String, Vec<f64>>,
    limit: usize,
) -> Result<Vec<SemanticTerm>, AppError> {
    if !(1..=32).contains(&limit) {
        return Err(AppError::usage("invalid semantic topic label bounds"));
    }
    let ranked = prepare(center, candidates, embeddings, super::shortlist::policy())?;
    let mut selected: Vec<Candidate> = Vec::new();
    for candidate in ranked.into_iter().filter(|c| c.term.cosine >= 0.25) {
        if selected.iter().any(|prior| {
            contained(&candidate.term.text, &prior.term.text)
                || contained(&prior.term.text, &candidate.term.text)
                || candidate
                    .vector
                    .iter()
                    .zip(&prior.vector)
                    .map(|(a, b)| a * b)
                    .sum::<f64>()
                    >= 0.92
        }) {
            continue;
        }
        selected.push(candidate);
        if selected.len() == limit {
            break;
        }
    }
    Ok(selected
        .into_iter()
        .map(|candidate| candidate.term)
        .collect())
}

#[test]
fn semantic_labels_prefer_relevant_phrases_and_reject_invalid_model_boundaries() {
    let candidates = vec![
        Term {
            text: "served".into(),
            score: 1.0,
            support: 1,
        },
        Term {
            text: "database connection".into(),
            score: 0.8,
            support: 1,
        },
        Term {
            text: "database".into(),
            score: 0.7,
            support: 1,
        },
    ];
    let embeddings = BTreeMap::from([
        ("served".into(), vec![0.0, 1.0]),
        ("database connection".into(), vec![1.0, 0.0]),
        ("database".into(), vec![1.0, 0.0]),
    ]);
    let result = rank(&[1.0, 0.0], &candidates, &embeddings, 3).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].text, "database connection");
    assert_eq!(result[0].cosine, 1.0);
    assert!(rank(&[1.0], &candidates, &embeddings, 3).is_err());
    assert!(rank(&[1.0, 0.0], &candidates, &BTreeMap::new(), 3).is_err());
    assert!(rank(&[0.0, 0.0], &candidates, &embeddings, 3).is_err());
    assert!(
        rank(
            &[1.0, 0.0],
            &[Term {
                text: "bad".into(),
                score: f64::NAN,
                support: 1
            }],
            &embeddings,
            3
        )
        .is_err()
    );
}

#[test]
fn label_diversity_omits_semantic_duplicates_without_lexical_overlap() {
    let candidates = vec![
        Term {
            text: "database".into(),
            score: 1.0,
            support: 1,
        },
        Term {
            text: "sql".into(),
            score: 0.9,
            support: 1,
        },
        Term {
            text: "layout".into(),
            score: 0.8,
            support: 1,
        },
    ];
    let vectors = BTreeMap::from([
        ("database".into(), vec![1.0, 0.0]),
        ("sql".into(), vec![0.99, 0.01]),
        ("layout".into(), vec![0.6, 0.8]),
    ]);
    let labels = rank(&[1.0, 0.0], &candidates, &vectors, 3).unwrap();
    assert_eq!(
        labels
            .iter()
            .map(|term| term.text.as_str())
            .collect::<Vec<_>>(),
        ["database", "layout"]
    );
}

#[test]
fn inspection_retains_low_similarity_candidates_and_matches_selected_scores() {
    let terms = vec![
        Term {
            text: "deadlock".into(),
            score: 1.0,
            support: 1,
        },
        Term {
            text: "boilerplate".into(),
            score: 0.9,
            support: 1,
        },
    ];
    let embeddings = BTreeMap::from([
        ("deadlock".into(), vec![1.0, 0.0]),
        ("boilerplate".into(), vec![0.0, 1.0]),
    ]);
    let review = inspect(&[1.0, 0.0], &terms, &embeddings).unwrap();
    let selected = rank(&[1.0, 0.0], &terms, &embeddings, 8).unwrap();
    assert_eq!(review.len(), 2);
    assert_eq!(selected.len(), 1);
    assert_eq!(review[0].text, selected[0].text);
    assert_eq!(review[0].score, selected[0].score);
    assert_eq!(review[1].cosine, 0.0);
}

#[test]
fn support_aware_scoring_balances_relevance_and_distinctiveness_without_hiding_scores() {
    use super::shortlist::Policy;
    let terms = vec![
        Term {
            text: "one off detail".into(),
            score: 1.0,
            support: 1,
        },
        Term {
            text: "database".into(),
            score: 0.1,
            support: 10,
        },
    ];
    let embeddings = BTreeMap::from([
        (
            "one off detail".into(),
            vec![0.9, (1.0_f64 - 0.9 * 0.9).sqrt()],
        ),
        ("database".into(), vec![0.8, 0.6]),
    ]);
    let center = [1.0, 0.0];
    let baseline = prepare(&center, &terms, &embeddings, Policy::Tfidf).unwrap();
    let supported = prepare(&center, &terms, &embeddings, Policy::Supported).unwrap();
    assert_eq!(baseline[0].term.text, "one off detail");
    assert_eq!(supported[0].term.text, "database");
    assert_eq!(supported[0].term.support, 10);
    assert!((supported[0].term.score - (0.6 * 0.8 + 0.3 + 0.1 * 0.1)).abs() < 1e-12);
    let mut invalid = terms;
    invalid[0].support = 0;
    assert!(prepare(&center, &invalid, &embeddings, Policy::Supported).is_err());
}
