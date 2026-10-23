//! Explicit model installation and smoke validation before marker publication.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::EmbeddingPool;
use super::assets::{self, InstallMarker, inventory};
use super::downloads;
use super::generations::StagedAssets;
use super::{
    AppError, Backend, EMBEDDING_MODEL, InstallSummary, Models, RERANKER_MODEL,
    coreml_embedding_enabled, embedding_model_description, load_reranker,
};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::semantic::embeddings::EmbeddingGroup;
use std::fs;
use std::path::Path;

fn installation_lock(root: &Path) -> Result<fs::File, AppError> {
    fs::create_dir_all(root).map_err(AppError::io)?;
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options
        .open(root.join(assets::INSTALL_LOCK_NAME))
        .map_err(AppError::io)?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(fs::TryLockError::WouldBlock) => Err(AppError::model_busy()),
        Err(fs::TryLockError::Error(error)) => Err(AppError::io(error)),
    }
}

impl Models {
    pub(crate) fn install(&self) -> Result<InstallSummary, AppError> {
        if let Some(config) = &self.remote {
            config.validate()?;
        }
        // Keep the lock through downloads, smoke checks, inventory and marker
        // publication. Never unlink it: another process may hold the same inode.
        let _lock = installation_lock(&self.root)?;
        let staged = StagedAssets::new(&self.root)?;
        let asset_root = staged.root();
        if let Some(config) = &self.remote {
            downloads::reranker(asset_root)?;
            let mut reranker = load_reranker(asset_root)?;
            reranker
                .rerank("installation check", ["installation check"], false, None)
                .map_err(|_| AppError::model("reranking model installation check failed"))?;
            let files = inventory(asset_root)?;
            let summary = InstallSummary {
                embedding_model: "remote",
                reranker_model: "jinaai/jina-reranker-v1-turbo-en",
                embedding_dimensions: config.dimensions,
                files: files.len(),
            };
            let marker = assets::remote_install_marker(&self.root, config.dimensions, files)?;
            drop(reranker);
            staged.publish(&self.root, marker)?;
            return Ok(summary);
        }
        let mut backend = Backend::download_and_load(asset_root)?;
        let embeddings = backend.embed(&["semantic model installation check"])?;
        let embedding_dimensions = embeddings
            .first()
            .map(Vec::len)
            .ok_or_else(|| AppError::model("embedding model returned no smoke result"))?;
        let reranked = backend.rerank(
            "semantic model installation check",
            &["unrelated", "semantic model installation check"],
        )?;
        if reranked.len() != 2 {
            return Err(AppError::model(
                "reranker returned an unexpected smoke result count",
            ));
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            let mut indexer = EmbeddingPool::load_local(asset_root, false)?;
            let smoke = [EmbeddingGroup {
                content: "semantic Core ML installation check".to_owned(),
                message_ids: vec!["smoke".to_owned()],
            }];
            if indexer.embed_wave(&smoke)?.len() != 1 {
                return Err(AppError::model(
                    "Core ML embedding model returned an unexpected smoke result count",
                ));
            }
        }

        let files = inventory(asset_root)?;
        if files.is_empty() {
            return Err(AppError::model("model installation produced no assets"));
        }
        let marker = InstallMarker {
            schema: 1,
            asset_directory: None,
            embedding_model: EMBEDDING_MODEL.to_string(),
            reranker_model: RERANKER_MODEL.to_string(),
            embedding_dimensions,
            coreml_embedding: coreml_embedding_enabled(),
            embedding_available: true,
            files,
        };
        let summary = InstallSummary {
            embedding_model: embedding_model_description(),
            reranker_model: "jinaai/jina-reranker-v1-turbo-en",
            embedding_dimensions,
            files: marker.files.len(),
        };
        drop(backend);
        staged.publish(&self.root, marker)?;
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installers_are_exclusive_and_release_the_same_lock_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("asset"), b"asset").unwrap();
        let first = installation_lock(directory.path()).unwrap();
        let error = installation_lock(directory.path()).unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::ModelBusy);
        assert_eq!(error.code, 6);
        assert!(error.error.retryable);
        assert_eq!(
            error.error.recommended_action,
            Some(crate::Action::ModelsInstall)
        );
        assert_eq!(assets::inventory(directory.path()).unwrap().len(), 1);
        drop(first);
        let next = installation_lock(directory.path()).unwrap();
        assert!(directory.path().join(assets::INSTALL_LOCK_NAME).is_file());
        assert!(installation_lock(directory.path()).is_err());
        drop(next);
    }

    #[test]
    fn unwritable_model_location_fails_before_any_sdk_work() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let result = Models::new(file.path().to_path_buf()).install();
        assert!(matches!(
            result.unwrap_err().error.kind,
            crate::ErrorKind::Io
        ));
    }
}
