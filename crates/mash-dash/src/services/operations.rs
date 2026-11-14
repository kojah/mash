//! A durable request receipt precedes every supervisor mutation. Replaying an
//! ID only reads its receipt: an interrupted operation is never rerun.
use super::config::{Kind, Service};
use super::model::{Action, State, now};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActionRequest {
    pub machine: String,
    pub service: String,
    pub action: Action,
    pub request_id: String,
    pub control_revision: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "ServiceOutcome")]
pub(super) enum Outcome {
    Pending,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[schemars(rename = "ServiceOperation")]
pub(super) struct Operation {
    pub request: ActionRequest,
    pub requested_at: u64,
    pub completed_at: Option<u64>,
    pub outcome: Outcome,
    pub message: String,
}

pub(super) fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn read(path: &Path) -> Result<Operation, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(16_385).read_to_end(&mut bytes))
        .map_err(|_| "Could not read operation receipt")?;
    if bytes.len() > 16_384 {
        return Err("Operation receipt is too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid operation receipt".into())
}

fn write_new(path: &Path, operation: &Operation) -> Result<(), String> {
    let bytes = serde_json::to_vec(operation).map_err(|_| "Could not encode operation")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "Could not create operation receipt")?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "Could not persist operation receipt")?;
    // On Unix persist the new directory entry before executing a command.
    #[cfg(unix)]
    File::open(path.parent().ok_or("No receipt directory")?)
        .and_then(|dir| dir.sync_all())
        .map_err(|_| "Could not persist receipt directory")?;
    Ok(())
}

fn prior(directory: &Path, request: &ActionRequest) -> Result<Option<Operation>, String> {
    let pending = directory.join(format!("{}.request.json", request.request_id));
    if !pending
        .try_exists()
        .map_err(|_| "Cannot inspect operation receipt")?
    {
        return Ok(None);
    }
    let mut operation = read(&pending)?;
    if operation.request != *request {
        return Err("Request ID was already used for a different action".into());
    }
    let finished = directory.join(format!("{}.result.json", request.request_id));
    if finished
        .try_exists()
        .map_err(|_| "Cannot inspect operation result")?
    {
        let result = read(&finished)?;
        if result.request != *request {
            return Err("Operation result does not match its request".into());
        }
        return Ok(Some(result));
    }
    operation.outcome = Outcome::Unknown;
    operation.message = "This request was already admitted. It may still be running or was interrupted. Check status; it will not be submitted again.".into();
    Ok(Some(operation))
}

pub(super) async fn execute(
    directory: PathBuf,
    service: Service,
    request: ActionRequest,
) -> Result<Operation, String> {
    if !valid_id(&request.request_id) {
        return Err("Invalid operation request ID".into());
    }
    if request.control_revision != service.control_revision() {
        return Err("The configured service target changed. Check status and confirm the new target before acting.".into());
    }
    if !service.allow_control || service.kind == Kind::Dashboard {
        return Err(
            "Control is disabled for this service. The serving dashboard cannot stop itself."
                .into(),
        );
    }
    if service.manager == super::config::Manager::WindowsTask && request.action == Action::Restart {
        return Err("Scheduled tasks support Start and Stop, not Restart.".into());
    }
    let preparation = tokio::task::spawn_blocking({
        let directory = directory.clone();
        let request = request.clone();
        let id = service.id.clone();
        move || {
            if let Some(operation) = prior(&directory, &request)? {
                return Ok((None, Some(operation)));
            }
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(directory.join(format!("{id}.lock")))
                .map_err(|_| "Cannot open service control lock")?;
            lock.try_lock()
                .map_err(|_| "Another operation is running for this service")?;
            // The second read closes the race between checking and locking.
            if let Some(operation) = prior(&directory, &request)? {
                return Ok((None, Some(operation)));
            }
            Ok::<_, String>((Some(lock), None))
        }
    })
    .await
    .map_err(|_| "Operation admission worker failed")??;
    if let Some(operation) = preparation.1 {
        return Ok(operation);
    }
    let _lock = preparation.0.ok_or("Service lock missing")?;
    let (state, _) = super::manager::status(&service).await?;
    let allowed = match request.action {
        Action::Start => matches!(state, State::Stopped | State::Failed),
        Action::Stop | Action::Restart => state == State::Healthy,
    };
    if !allowed {
        return Err("Service state changed or is unknown. Check status before acting.".into());
    }
    let operation = Operation {
        request: request.clone(),
        requested_at: now(),
        completed_at: None,
        outcome: Outcome::Pending,
        message: "Supervisor action admitted.".into(),
    };
    tokio::task::spawn_blocking({
        let directory = directory.clone();
        let operation = operation.clone();
        move || {
            write_new(
                &directory.join(format!("{}.request.json", operation.request.request_id)),
                &operation,
            )
        }
    })
    .await
    .map_err(|_| "Operation receipt worker failed")??;
    // No automatic retry after this point, including timeout or process exit.
    let (outcome, message) = perform(&service, request.action).await;
    #[cfg(test)]
    if service.interrupt_after_command {
        return Err("Simulated interruption after supervisor action, before result receipt".into());
    }
    let finished = Operation {
        outcome,
        message,
        completed_at: Some(now()),
        ..operation
    };
    tokio::task::spawn_blocking({
        let directory = directory.clone();
        let finished = finished.clone();
        move || {
            write_new(
                &directory.join(format!("{}.result.json", finished.request.request_id)),
                &finished,
            )
        }
    })
    .await
    .map_err(|_| "Operation result worker failed")??;
    Ok(finished)
}

async fn perform(service: &Service, action: Action) -> (Outcome, String) {
    match super::manager::action(service, action).await {
        Err(message) => (Outcome::Unknown, message),
        Ok(output) if !output.success => (
            Outcome::Failed,
            "The service manager rejected the action. Check status and the service log.".into(),
        ),
        Ok(_) => {
            let expected = if action == Action::Stop {
                State::Stopped
            } else {
                State::Healthy
            };
            match super::manager::status(service).await {
                Ok((state, _)) if state == expected => (Outcome::Succeeded, "Supervisor action completed and the requested process state was observed. This does not certify application readiness or schema compatibility.".into()),
                _ => (Outcome::Unknown, "The manager accepted the action, but the requested state was not confirmed. Check status before requesting another action.".into()),
            }
        }
    }
}

pub(super) fn history(directory: &Path) -> Result<Vec<Operation>, String> {
    let mut results = Vec::new();
    for (index, entry) in std::fs::read_dir(directory)
        .map_err(|_| "Cannot read operation history")?
        .enumerate()
    {
        if index >= 20_000 {
            return Err(
                "Operation history limit reached; archive old receipts before continuing".into(),
            );
        }
        let path = entry
            .map_err(|_| "Cannot read operation history entry")?
            .path();
        if path
            .file_name()
            .and_then(|v| v.to_str())
            .is_some_and(|name| name.ends_with(".request.json"))
        {
            let record = read(&path)?;
            if !valid_id(&record.request.request_id) {
                return Err("Invalid stored operation ID".into());
            }
            if let Some(operation) = prior(directory, &record.request)? {
                results.push(operation);
            }
        }
    }
    results.sort_by_key(|record| std::cmp::Reverse(record.requested_at));
    results.truncate(100);
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn services_changed_target_is_rejected_before_a_receipt_or_command() {
        let dir = tempfile::tempdir().unwrap();
        let service: Service = toml::from_str("id='fixture'\nname='Fixture'\nkind='watcher'\nmanager='systemd_user'\nunit='mash-no-such-unit.service'\nallow_control=true\n").unwrap();
        let request = ActionRequest {
            machine: "fixture".into(),
            service: "managed-fixture".into(),
            action: Action::Stop,
            request_id: "12345678-1234-1234-1234-123456789abc".into(),
            control_revision: "old-target".into(),
        };
        let error = execute(dir.path().into(), service, request)
            .await
            .unwrap_err();
        assert!(error.contains("configured service target changed"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn services_interrupted_receipt_never_authorizes_reexecution() {
        let dir = tempfile::tempdir().unwrap();
        let request = ActionRequest {
            control_revision: "fixture".into(),
            machine: "test".into(),
            service: "managed-watch".into(),
            action: Action::Restart,
            request_id: "12345678-1234-1234-1234-123456789abc".into(),
        };
        let operation = Operation {
            request: request.clone(),
            requested_at: 1,
            completed_at: None,
            outcome: Outcome::Pending,
            message: "admitted".into(),
        };
        write_new(
            &dir.path()
                .join(format!("{}.request.json", request.request_id)),
            &operation,
        )
        .unwrap();
        assert_eq!(
            prior(dir.path(), &request).unwrap().unwrap().outcome,
            Outcome::Unknown
        );
        let mut other = request.clone();
        other.action = Action::Stop;
        assert!(prior(dir.path(), &other).is_err());
        assert!(!valid_id("../../etc/passwd"));
        assert_eq!(history(dir.path()).unwrap().len(), 1);
    }
}
