//! Terminal-only setup presentation. All mutations remain in the typed core.
mod prompts;

use super::{AppError, Cli, Operation, Response, core};
use crate::config::{self, QdrantConfig};
use crate::core::InitRequest;
use std::time::Duration;

pub(super) const fn interactive(yes: bool, input_terminal: bool, output_terminal: bool) -> bool {
    !yes && input_terminal && output_terminal
}

pub(super) async fn run(cli: Cli) -> Result<Response, AppError> {
    let paths = cli.paths();
    let request = cli.into_request()?;
    let Operation::Init(init) = request.operation else {
        return Err(AppError::internal("wizard requires init"));
    };
    let draft = crate::blocking(move || prompts::collect(&paths, init)).await?;
    let Some(mut draft) = draft else {
        return Ok(Response::InitCancelled { cancelled: true });
    };
    eprintln!("Checking local Qdrant (no changes yet)...");
    let mut ready = qdrant_ready(&draft.qdrant).await;
    if ready {
        eprintln!(
            "Local Qdrant HTTP endpoint is reachable. Indexing will also require its gRPC endpoint."
        );
    } else {
        eprintln!(
            "Local Qdrant is unavailable. Start it using https://getmash.io/installation/#start-the-local-search-service\nYou can save configuration now and run `mash init` again afterward."
        );
    }
    if let Some(shared) = &draft.shared {
        eprintln!("Checking shared-server authentication (no history uploaded)...");
        let checked =
            tokio::time::timeout(Duration::from_secs(8), crate::shared::freshness(shared)).await;
        if matches!(checked, Ok(Ok(_))) {
            eprintln!(
                "Shared-server authentication succeeded. Clients and server must use matching Mash releases."
            );
        } else {
            ready = false;
            eprintln!(
                "Could not verify the shared connection. Check the server URL, group, machine ID and token environment variable. You can save configuration without indexing."
            );
        }
    }
    let confirmed = crate::blocking(move || {
        if prompts::confirm(&mut draft, ready)? {
            Ok(Some(draft))
        } else {
            Ok(None)
        }
    })
    .await?;
    let Some(draft) = confirmed else {
        return Ok(Response::InitCancelled { cancelled: true });
    };
    core::execute(core::Request {
        paths: request.paths,
        operation: Operation::Init(draft.init),
    })
    .await
    .map(Box::new)
    .map(Response::Operation)
}

struct Draft {
    init: InitRequest,
    qdrant: QdrantConfig,
    shared: Option<config::SharedConfig>,
    summary: String,
}

async fn qdrant_ready(config: &QdrantConfig) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
    else {
        return false;
    };
    let mut request = client.get(format!("{}/readyz", config.url.trim_end_matches('/')));
    if let Some(name) = &config.api_key_env {
        let Ok(key) = std::env::var(name) else {
            return false;
        };
        request = request.header("api-key", key);
    }
    request
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_wizard_only_prompts_with_input_and_stderr_terminals() {
        for yes in [true, false] {
            for input in [true, false] {
                for output in [true, false] {
                    assert_eq!(interactive(yes, input, output), !yes && input && output);
                }
            }
        }
    }
}
