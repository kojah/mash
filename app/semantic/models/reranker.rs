//! Keep a ready CPU session when Apple acceleration cannot compile or execute.
use super::{AppError, Path, TextRerank, load_reranker};
use crate::observability::retrieval::RerankerEngine;

pub(super) struct Reranker {
    pub(super) cpu: TextRerank,
    engine: RerankerEngine,
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    accelerated: Option<crate::semantic::coreml::reranker::Pool>,
}

#[cfg(test)]
mod tests;

impl Reranker {
    pub(super) fn load(root: &Path) -> Result<Self, AppError> {
        let cpu = load_reranker(root)?;
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        let accelerated = Some(crate::semantic::coreml::reranker::Pool::new(
            root,
            cpu.tokenizer.clone(),
        ));
        Ok(Self {
            cpu,
            engine: RerankerEngine::Cpu,
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            accelerated,
        })
    }

    pub(super) const fn engine(&self) -> RerankerEngine {
        self.engine
    }

    pub(super) fn run(
        &mut self,
        query: &str,
        documents: &[&str],
    ) -> Result<Vec<fastembed::RerankResult>, AppError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if let Some(pool) = &mut self.accelerated {
            match pool.run(query, documents) {
                Ok(scores) => {
                    self.engine = RerankerEngine::CoreMl;
                    return Ok(scores);
                }
                Err(error) => {
                    tracing::warn!(reason = %error.error.message, "Core ML reranking failed; using CPU for this backend");
                    // Do not retry compilation or a failing device on every batch.
                    self.accelerated = None;
                    self.engine = RerankerEngine::CpuFallback;
                }
            }
        }
        self.cpu
            .rerank(query, documents, false, Some(4))
            .map_err(|error| AppError::model(format!("reranking inference failed: {error}")))
    }
}
