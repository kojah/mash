//! Fleet orchestration delegates to the repository's single Ansible workflow.
mod probe;
mod readiness;
mod report;

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(version, about = "Manage Mash across configured machines")]
struct Cli {
    /// Shared TOML inventory used by the dashboard and fleet CLI.
    #[arg(long, global = true)]
    fleet_config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Control opt-in supervised history watchers.
    Watch {
        #[command(subcommand)]
        action: Watch,
    },
    /// Show recent watcher logs for selected machines.
    Logs(Logs),
    /// Check installed builds, schemas, and search readiness.
    Status(Targets),
    /// Validate deployment prerequisites without installing anything.
    Preflight(Targets),
    /// Build and deploy through the configured Ansible inventory.
    Deploy(Targets),
    /// Index and publish configured history.
    Sync(Targets),
    /// Rebuild local search views from stored history.
    LocalViews(Targets),
    /// Replay shared search views from stored history.
    ReplayViews(Targets),
    /// Internal typed MCP check used by Ansible.
    #[command(hide = true)]
    Probe(probe::Arguments),
}

#[derive(Subcommand)]
enum Watch {
    Start(Targets),
    Stop(Targets),
    Status(Targets),
}

#[derive(Args)]
struct Logs {
    #[arg(value_parser = target_name)]
    host: String,
    #[arg(value_enum, default_value = "watcher")]
    service: Service,
    /// Mash source checkout containing deployment playbooks.
    #[arg(long, default_value = ".")]
    repository: PathBuf,
    /// Private Ansible inventory.
    #[arg(long)]
    inventory: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Service {
    Watcher,
}

#[derive(Args)]
struct Targets {
    /// Mash source checkout containing deployment playbooks.
    #[arg(long, default_value = ".")]
    repository: PathBuf,
    /// Private Ansible inventory; defaults to `MASH_DEPLOY_INVENTORY`.
    #[arg(long)]
    inventory: Option<PathBuf>,
    /// Explicit inventory host names.
    #[arg(required_unless_present = "all", num_args = 1.., value_parser = target_name)]
    hosts: Vec<String>,
    /// Select every eligible machine from the private inventory.
    #[arg(long, conflicts_with = "hosts")]
    all: bool,
}

fn target_name(value: &str) -> Result<String, String> {
    if value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
    {
        Ok(value.to_owned())
    } else {
        Err("expected an inventory host name, not a host pattern or command".into())
    }
}

async fn execute(operation: &str, targets: Targets) -> Result<ExitCode, String> {
    let repository = targets
        .repository
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let runner = repository.join("tools/fleet.sh");
    if !runner.is_file() {
        return Err("--repository must point to a Mash checkout containing tools/fleet.sh".into());
    }
    let mut command = tokio::process::Command::new("bash");
    command.arg(runner).arg(operation).args(targets.hosts);
    command.env(
        "MASH_FLEET_BINARY",
        std::env::current_exe().map_err(|error| error.to_string())?,
    );
    if targets.all {
        command.arg("--all");
    }
    if let Some(inventory) = targets.inventory {
        command.env("MASH_DEPLOY_INVENTORY", inventory);
    }
    let status = command.status().await.map_err(|error| error.to_string())?;
    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn execute_configured(
    operation: &str,
    targets: Targets,
    path: PathBuf,
) -> Result<ExitCode, String> {
    if targets.inventory.is_some() || targets.repository != std::path::Path::new(".") {
        return Err(
            "--fleet-config supplies the repository and inventory; do not combine overrides".into(),
        );
    }
    let inventory = mash_fleet::inventory::Inventory::load(&path)?;
    let action = mash_fleet::inventory::Action::from_operation(operation)
        .ok_or("unsupported fleet operation")?;
    inventory.targets(action, &targets.hosts)?;
    std::fs::create_dir_all(&inventory.controller.jobs)
        .map_err(|_| "cannot create private fleet job directory")?;
    let directory = tempfile::Builder::new()
        .prefix("request-")
        .tempdir_in(&inventory.controller.jobs)
        .map_err(|_| "cannot create fleet request directory")?
        .keep();
    let status = mash_fleet::runner::command(&inventory, action, &targets.hosts, &directory)?
        .status()
        .await
        .map_err(|_| "cannot start fleet runner")?;
    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let (operation, targets) = match cli.command {
        Command::Watch { action } => match action {
            Watch::Start(targets) => ("watch-start", targets),
            Watch::Stop(targets) => ("watch-stop", targets),
            Watch::Status(targets) => ("watch-status", targets),
        },
        Command::Logs(logs) => (
            match logs.service {
                Service::Watcher => "watch-logs",
            },
            Targets {
                repository: logs.repository,
                inventory: logs.inventory,
                hosts: vec![logs.host],
                all: false,
            },
        ),
        Command::Status(targets) => ("check", targets),
        Command::Preflight(targets) => ("preflight", targets),
        Command::Deploy(targets) => ("deploy", targets),
        Command::Sync(targets) => ("sync", targets),
        Command::LocalViews(targets) => ("local-views", targets),
        Command::ReplayViews(targets) => ("replay-views", targets),
        Command::Probe(arguments) => {
            probe::run_probe(arguments).await;
            return ExitCode::SUCCESS;
        }
    };
    let result = if let Some(path) = cli.fleet_config {
        execute_configured(operation, targets, path).await
    } else {
        execute(operation, targets).await
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn fleet_forwards_inventory_and_all_without_shell_interpolation() {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("checkout with spaces");
        std::fs::create_dir_all(repository.join("tools")).unwrap();
        std::fs::write(
            repository.join("tools/fleet.sh"),
            "set -eu\n[[ $# == 2 && $1 == check && $2 == --all ]]\n[[ $MASH_DEPLOY_INVENTORY == 'private inventory.yml' ]]\n[[ -x $MASH_FLEET_BINARY ]]\nexit 0\n",
        )
        .unwrap();
        let result = execute(
            "check",
            Targets {
                repository: repository.clone(),
                inventory: Some("private inventory.yml".into()),
                hosts: vec![],
                all: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(result, ExitCode::SUCCESS);
        std::fs::write(repository.join("tools/fleet.sh"), "exit 75\n").unwrap();
        let result = execute(
            "deploy",
            Targets {
                repository,
                inventory: None,
                hosts: vec!["offline".into()],
                all: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(result, ExitCode::FAILURE);
    }

    #[test]
    fn fleet_rejects_patterns_and_requires_explicit_targets() {
        for target in ["all:!server", "*", "host;true", "-host", ""] {
            assert!(target_name(target).is_err());
        }
        assert!(Cli::try_parse_from(["mash-fleet", "deploy"]).is_err());
        assert!(Cli::try_parse_from(["mash-fleet", "status", "dev-macbook"]).is_ok());
        assert!(Cli::try_parse_from(["mash-fleet", "logs", "dev-macbook", "watcher"]).is_ok());
        assert!(Cli::try_parse_from(["mash-fleet", "logs", "dev-macbook", "arbitrary"]).is_err());
        assert!(Cli::try_parse_from(["mash-fleet", "watch", "start", "--all"]).is_ok());
        assert!(Cli::try_parse_from(["mash-fleet", "watch", "stop", "--all", "xenia"]).is_err());
    }
}
