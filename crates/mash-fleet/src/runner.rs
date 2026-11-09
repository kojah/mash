//! Prepare the existing Ansible runner from a validated inventory snapshot.
use crate::inventory::{Action, Inventory};
use std::path::Path;

/// Prepare one invocation; the caller owns its durable job directory and receipt.
/// This does not launch or retry work.
///
/// # Errors
/// Rejects disabled operations, invalid targets or unwritable snapshot paths.
pub fn command(
    inventory: &Inventory,
    action: Action,
    requested: &[String],
    directory: &Path,
) -> Result<tokio::process::Command, String> {
    let targets = inventory.targets(action, requested)?;
    let snapshot = directory.join("inventory.json");
    crate::ansible::write(inventory, &snapshot)?;
    let mut command = tokio::process::Command::new("bash");
    command
        .arg(inventory.controller.repository.join("tools/fleet.sh"))
        .arg(action.operation())
        .args(targets)
        .current_dir(&inventory.controller.repository)
        .env("MASH_FLEET_BINARY", &inventory.controller.binary)
        .env("MASH_DEPLOY_INVENTORY", snapshot)
        .env("MASH_DEPLOY_LOGS", &inventory.controller.jobs)
        .env("MASH_FLEET_JOB_DIRECTORY", directory.join("run"));
    Ok(command)
}
