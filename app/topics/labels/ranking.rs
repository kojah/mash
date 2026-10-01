//! Semantic label ranking in the same model space; no generated labels or LLM.
use super::Term;
use crate::AppError;
use crate::topics::normalize;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub(crate) struct SemanticTerm {
    pub(crate) text: String,
    pub(crate) tfidf: f64,
    pub(crate) cosine: f64,
    pub(crate) score: f64,
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

pub(crate) fn rank(
    center: &[f64],
    candidates: &[Term],
    embeddings: &BTreeMap<String, Vec<f64>>,
    limit: usize,
) -> Result<Vec<SemanticTerm>, AppError> {
    if center.is_empty()
        || center.len() > 2048
        || candidates.len() > 64
        || !(1..=32).contains(&limit)
    {
        return Err(AppError::usage("invalid semantic topic label bounds"));
    }
    let center = normalize(center, center.len())?;
    let maximum = candidates.iter().map(|t| t.score).fold(0.0, f64::max);
    let mut ranked = Vec::new();
    for term in candidates {
        if term.text.trim().is_empty() || !term.score.is_finite() || term.score < 0.0 {
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
        if cosine < 0.25 {
            continue;
        }
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
                score: 0.85 * cosine + 0.15 * relative,
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
    let mut selected: Vec<Candidate> = Vec::new();
    for candidate in ranked {
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
        },
        Term {
            text: "database connection".into(),
            score: 0.8,
        },
        Term {
            text: "database".into(),
            score: 0.7,
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
                score: f64::NAN
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
        },
        Term {
            text: "sql".into(),
            score: 0.9,
        },
        Term {
            text: "layout".into(),
            score: 0.8,
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
