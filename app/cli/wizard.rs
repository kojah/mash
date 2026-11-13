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
    let observed = crate::views::qdrant::reachability(&draft.qdrant).await;
    let mut ready = observed.state == crate::views::qdrant::Reachability::Reachable;
    if ready {
        eprintln!(
            "Local Qdrant HTTP endpoint is reachable. Indexing will also require its gRPC endpoint."
        );
    } else {
        eprintln!(
            "Local Qdrant is unavailable. {}\nYou can save configuration now and run `mash init` again afterward.",
            observed.message.unwrap_or_default()
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
