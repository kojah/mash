//! Bounded, lazy fixed-shape Jina sessions; every graph node must use Core ML.
use super::{cache, coreml_sequence_bucket};
use crate::AppError;
use crate::semantic::models::{read_tokenizer_files, snapshot};
use crate::semantic::{COREML_CACHE_DIRECTORY, RERANKER_MODEL, RERANKER_REPOSITORY};
use fastembed::{RerankInitOptionsUserDefined, TextRerank, UserDefinedRerankingModel};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

const BATCH: usize = 4;

struct Session {
    model: TextRerank,
    // Keep compiled artifacts leased until the SDK session has been dropped.
    _lease: fs::File,
}

pub(crate) struct Pool {
    root: PathBuf,
    tokenizer: tokenizers::Tokenizer,
    sessions: HashMap<usize, Session>,
}

impl Pool {
    pub(crate) fn new(root: &Path, mut tokenizer: tokenizers::Tokenizer) -> Self {
        tokenizer.with_padding(None);
        Self {
            root: root.to_path_buf(),
            tokenizer,
            sessions: HashMap::new(),
        }
    }

    pub(crate) fn run(
        &mut self,
        query: &str,
        documents: &[&str],
    ) -> Result<Vec<fastembed::RerankResult>, AppError> {
        let mut scores = Vec::with_capacity(documents.len());
        for (batch, documents) in documents.chunks(BATCH).enumerate() {
            // Keep the CPU tokenizer's truncation policy, including pair special
            // tokens. Only padding changes; long query/document pairs stay equivalent.
            let pairs = documents
                .iter()
                .map(|document| (query, *document))
                .collect::<Vec<_>>();
            let encodings = self.tokenizer.encode_batch(pairs, true).map_err(|error| {
                AppError::model(format!("Core ML reranker tokenization failed: {error}"))
            })?;
            let sequence = coreml_sequence_bucket(
                encodings
                    .iter()
                    .map(tokenizers::Encoding::len)
                    .max()
                    .unwrap_or(0),
            );
            let session = match self.sessions.entry(sequence) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => entry.insert(
                    crate::progress::during(crate::progress::Phase::ModelLoading, || {
                        load(&self.root, sequence)
                    })?,
                ),
            };
            let mut padded = documents.to_vec();
            padded.resize(BATCH, "");
            let results = session
                .model
                .rerank(query, padded, false, Some(BATCH))
                .map_err(|error| {
                    AppError::model(format!("Core ML reranking inference failed: {error}"))
                })?;
            if results.len() != BATCH {
                return Err(AppError::model(
                    "Core ML reranker returned an unexpected result count",
                ));
            }
            for mut result in results
                .into_iter()
                .filter(|result| result.index < documents.len())
            {
                result.index += batch * BATCH;
                scores.push(result);
            }
        }
        scores.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.index.cmp(&b.index))
        });
        Ok(scores)
    }
}

fn load(root: &Path, sequence: usize) -> Result<Session, AppError> {
    let directory = root
        .join(COREML_CACHE_DIRECTORY)
        .join(format!("reranker-batch-{BATCH}-sequence-{sequence}"));
    let (model, lease) = cache::load(&directory, |directory| build(root, sequence, directory))?;
    Ok(Session {
        model,
        _lease: lease,
    })
}

fn build(root: &Path, sequence: usize, directory: &Path) -> Result<TextRerank, cache::BuildError> {
    let source = snapshot(root, RERANKER_REPOSITORY)?;
    let info = TextRerank::get_model_info(&RERANKER_MODEL);
    let model = UserDefinedRerankingModel::new(
        source.join(info.model_file),
        read_tokenizer_files(&source)?,
    );
    let provider = ort::ep::CoreML::default()
        .with_model_format(ort::ep::coreml::ModelFormat::MLProgram)
        .with_compute_units(ort::ep::coreml::ComputeUnits::All)
        .with_specialization_strategy(ort::ep::coreml::SpecializationStrategy::FastPrediction)
        .with_model_cache_dir(directory.to_string_lossy())
        .build()
        .error_on_failure();
    let sequence =
        i64::try_from(sequence).map_err(|_| AppError::internal("reranker shape exceeds i64"))?;
    let options = RerankInitOptionsUserDefined::default()
        .with_execution_providers(vec![provider])
        .with_disable_cpu_fallback(true)
        // Jina names these dimensions differently from MiniLM.
        .with_dimension_override("batch", 4)
        .with_dimension_override("sequence", sequence);
    let mut model = TextRerank::try_new_from_user_defined(model, options).map_err(|error| {
        let message = AppError::model(format!("failed to load Core ML reranker: {error}"));
        match error {
            fastembed::Error::Ort(_)
            | fastembed::Error::OrtBuilder(_)
            | fastembed::Error::OrtSession(_) => cache::BuildError::Compile(message),
            _ => cache::BuildError::Input(message),
        }
    })?;
    let mut padding = model.tokenizer.get_padding().cloned().ok_or_else(|| {
        AppError::model("Core ML reranker tokenizer has no padding configuration")
    })?;
    padding.strategy = tokenizers::PaddingStrategy::Fixed(
        usize::try_from(sequence).map_err(|_| AppError::internal("invalid reranker sequence"))?,
    );
    model.tokenizer.with_padding(Some(padding));
    Ok(model)
}
