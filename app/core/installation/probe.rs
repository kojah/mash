//! Explicit subprocess check. Never invoke an index/search tool or execute hooks.
use super::{Client, Component, State, expected_stamp};
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
use serde::Deserialize;
use std::{process::Stdio, time::Duration};

#[derive(Deserialize)]
struct Help {
    runtime: Runtime,
}

#[derive(Deserialize)]
struct Runtime {
    contract_fingerprint: String,
}

pub(super) async fn check(client: &mut Client) {
    let Some(launch) = &client.launch else {
        client.add(
            Component::Probe,
            State::Unverified,
            None,
            "No unambiguous enabled user-level stdio registration is available to probe.",
            Some("Resolve the configuration findings, then rerun with --probe."),
        );
        return;
    };
    let source = launch.source.clone();
    if launch
        .server
        .args
        .iter()
        .any(|arg| arg == "--watch" || arg.starts_with("--watch="))
    {
        client.add(
            Component::Probe,
            State::Unverified,
            Some(&source),
            "Probe skipped: configured --watch would ingest history on startup.",
            Some("Use a separate MCP registration without --watch for a read-only probe."),
        );
        return;
    }
    if launch
        .server
        .env_vars
        .iter()
        .any(|key| !launch.server.env.contains_key(key) && std::env::var_os(key).is_none())
    {
        client.add(
            Component::Probe,
            State::Failed,
            Some(&source),
            "A forwarded environment variable is missing from this CLI process.",
            Some("Run the probe with the same environment as the agent client."),
        );
        return;
    }
    let mut command = tokio::process::Command::new(&launch.executable);
    command
        .args(&launch.server.args)
        .envs(&launch.server.env)
        .current_dir(&launch.cwd)
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let result = tokio::time::timeout(Duration::from_secs(10), handshake(command)).await;
    let (state, message) = match result {
        Ok(Ok(())) => (
            State::Passed,
            "New process completed the MCP handshake and Mash help contract check. Existing agent connections remain unverified.",
        ),
        Ok(Err(message)) => (State::Failed, message),
        Err(_) => (
            State::Failed,
            "MCP initialization/help/shutdown exceeded the 10-second probe deadline.",
        ),
    };
    client.add(Component::Probe, state, Some(&source), message, (state == State::Failed).then_some("Repair the MCP command, arguments and environment; reconnect the agent, then rerun --probe."));
}

async fn handshake(command: tokio::process::Command) -> Result<(), &'static str> {
    let (transport, _) = TokioChildProcess::builder(command)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Cannot start configured MCP executable.")?;
    let service = ()
        .serve(transport)
        .await
        .map_err(|_| "Configured process failed its MCP initialization handshake.")?;
    let result = async {
        if service.peer_info().is_none_or(|info| {
            info.server_info
                .as_ref()
                .is_none_or(|server| server.name != "mash")
        }) {
            return Err("Configured server does not identify itself as Mash.");
        }
        let response = service
            .call_tool(CallToolRequestParams::new("help"))
            .await
            .map_err(|_| "Mash help request failed.")?;
        if response.is_error == Some(true) {
            return Err("Mash help returned an error.");
        }
        let help: Help = serde_json::from_value(
            response
                .structured_content
                .ok_or("Mash help omitted structured runtime identity.")?,
        )
        .map_err(|_| "Mash help returned an unsupported runtime identity.")?;
        if help.runtime.contract_fingerprint != expected_stamp().contract_fingerprint {
            return Err(
                "MCP contract differs from this CLI; update the binary/launcher and reconnect.",
            );
        }
        Ok(())
    }
    .await;
    // A bounded cancellation prevents a server ignoring shutdown from hanging status.
    let _ = tokio::time::timeout(Duration::from_secs(1), service.cancel()).await;
    result
}
