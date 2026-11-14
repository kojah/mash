//! Bounded CPU contextual reranking.
use super::{AppError, Path, TextRerank, load_reranker};

pub(super) struct Reranker {
    pub(super) cpu: TextRerank,
}

#[cfg(test)]
mod tests;

impl Reranker {
    pub(super) fn load(root: &Path) -> Result<Self, AppError> {
        Ok(Self {
            cpu: load_reranker(root)?,
        })
    }

    pub(super) fn run(
        &mut self,
        query: &str,
        documents: &[&str],
    ) -> Result<Vec<fastembed::RerankResult>, AppError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        self.cpu
            .rerank(query, documents, false, Some(4))
            .map_err(|error| AppError::model(format!("reranking inference failed: {error}")))
    }
}
