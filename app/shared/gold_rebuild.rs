//! Validate gold-only uploads against retained, certified canonical history.
use super::{
    protocol,
    snapshot::{Loaded, Plan},
    validation::conflict,
};
use crate::{
    AppError,
    silver::certificate::{Completeness, INTERPRETATION_VERSION},
};
use std::{collections::HashSet, io::BufRead};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Event {
    Start {
        publication_revision: i64,
        silver: super::silver_revision::State,
        generation: String,
        #[serde(default)]
        build: crate::gold::config::Build,
    },
    Embeddings {
        records: Vec<Embedding>,
    },
    Finish,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Embedding {
    pub(super) message_id: String,
    pub(super) vector: Vec<f64>,
}

pub(super) fn certified(loaded: &Loaded) -> bool {
    let Some(head) = &loaded.head else {
        return false;
    };
    if let Some(source) = &head.source_silver {
        return source.is_complete() && source.interpretation_version == INTERPRETATION_VERSION;
    }
    let covered: HashSet<_> = head
        .silver_coverage
        .iter()
        .filter(|c| {
            c.completeness == Completeness::Complete
                && c.scan.interpretation_version == INTERPRETATION_VERSION
        })
        .map(|c| c.scan.provider)
        .collect();
    !covered.is_empty()
        && loaded
            .data
            .conversations
            .values()
            .all(|c| covered.contains(&c.provider))
}

pub(super) fn plan(
    file: &tempfile::NamedTempFile,
    mut loaded: Loaded,
    generation: &str,
    dimensions: usize,
    build: &crate::gold::config::Build,
) -> Result<Plan, AppError> {
    let mut lines = std::io::BufReader::new(file.reopen().map_err(AppError::io)?).lines();
    let first = lines
        .next()
        .ok_or_else(|| AppError::usage("empty gold stream"))?
        .map_err(AppError::io)?;
    let Event::Start {
        publication_revision,
        silver,
        generation: requested,
        build: requested_build,
    } = serde_json::from_str(&first).map_err(|_| AppError::usage("invalid gold start"))?
    else {
        return Err(AppError::usage("gold stream must start with metadata"));
    };
    silver
        .validate()
        .map_err(|_| AppError::usage("invalid gold silver identity"))?;
    let head = loaded
        .head
        .as_ref()
        .ok_or_else(|| conflict("shared silver has not been indexed"))?;
    if head.revision != publication_revision || head.silver_revision.as_ref() != Some(&silver) {
        return Err(conflict(
            "shared silver or publication changed; restart gold rebuild",
        ));
    }
    if requested != generation {
        return Err(AppError::usage(
            "gold model does not match shared embedding space",
        ));
    }
    if &requested_build != build {
        return Err(conflict("shared gold recipe changed; restart gold rebuild"));
    }
    if !certified(&loaded) {
        return Err(conflict(
            "gold rebuild requires complete, current shared silver",
        ));
    }
    // This is the source-upload counter. Advancing it here would collide with
    // the next local SQLite snapshot revision and discard new canonical data.
    // Qdrant's immutable version and head CAS serialize gold publication instead.
    let revision = head.revision;
    loaded
        .head
        .as_mut()
        .ok_or_else(|| conflict("missing shared head"))?
        .gold_manifest = Some(super::gold_manifest::Manifest::new(
        requested_build,
        generation.to_owned(),
        silver,
    ));
    let mut seen = HashSet::new();
    let mut finished = false;
    for line in lines {
        if finished {
            return Err(AppError::usage("data after gold finish"));
        }
        let event: Event = serde_json::from_str(&line.map_err(AppError::io)?)
            .map_err(|_| AppError::usage("invalid gold event"))?;
        match event {
            Event::Embeddings { records } => {
                if records.len() > 128 {
                    return Err(AppError::usage("gold batch exceeds 128 records"));
                }
                for record in records {
                    let message = loaded
                        .data
                        .messages
                        .get_mut(&record.message_id)
                        .ok_or_else(|| AppError::usage("gold embedding has no silver message"))?;
                    if message.projection.is_empty()
                        || !protocol::valid_vector(&record.vector, dimensions)
                        || !seen.insert(record.message_id)
                    {
                        return Err(AppError::usage("invalid or duplicate gold embedding"));
                    }
                    message.generation = Some(generation.to_owned());
                    message.vector = Some(record.vector);
                }
            }
            Event::Finish => finished = true,
            Event::Start { .. } => return Err(AppError::usage("duplicate gold start")),
        }
    }
    let searchable = loaded
        .data
        .messages
        .values()
        .filter(|m| !m.projection.is_empty())
        .count();
    if !finished || seen.len() != searchable {
        return Err(AppError::usage("incomplete gold replacement"));
    }
    Ok(Plan {
        source_build: None,
        loaded,
        revision,
        changed: false,
        removed: 0,
        purged: 0,
    })
}
