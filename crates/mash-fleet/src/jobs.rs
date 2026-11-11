//! Durable fleet requests: a recorded ID is observed, never submitted again.
use crate::inventory::{Action, Inventory};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub action: Action,
    pub targets: Vec<String>,
    pub revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Running,
    Succeeded,
    Partial,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetState {
    Waiting,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Target {
    pub id: String,
    pub state: TargetState,
    pub progress: Option<Progress>,
    /// One line on what the operation found or why it failed. These are the
    /// fixed sentences the probe and playbooks write, never command output.
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Progress {
    pub progress: f64,
    pub total: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Job {
    pub request: Request,
    pub requested_at: u64,
    pub completed_at: Option<u64>,
    pub outcome: Outcome,
    pub targets: Vec<Target>,
}

pub enum Admission {
    Existing(Job),
    Start(Box<Pending>),
}

pub struct Pending {
    inventory: Inventory,
    directory: PathBuf,
    lock: File,
    job: Job,
}

#[must_use]
pub fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn decode<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(1_048_577).read_to_end(&mut bytes))
        .map_err(|_| "cannot read fleet job record")?;
    if bytes.len() > 1_048_576 {
        return Err("fleet job record exceeds size limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid fleet job record".into())
}

fn create(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|_| "cannot create private fleet job file".into())
}

fn persist(path: &Path, job: &Job) -> Result<(), String> {
    let bytes = serde_json::to_vec(job).map_err(|_| "cannot encode fleet job")?;
    let parent = path.parent().ok_or("missing job directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "cannot create private fleet job record")?;
    file.write_all(&bytes)
        .map_err(|_| "cannot write fleet job")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "cannot sync fleet job")?;
    file.persist_noclobber(path)
        .map_err(|_| "cannot publish fleet job record")?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|_| "cannot sync fleet job directory")?;
    Ok(())
}

fn directory(inventory: &Inventory, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("invalid fleet request ID".into());
    }
    Ok(inventory.controller.jobs.join("dashboard").join(id))
}

#[derive(Deserialize)]
struct TargetReport {
    state: TargetState,
    progress: Option<Progress>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    /// A watcher operation's supervisor state, such as `active`.
    #[serde(default)]
    process: Option<String>,
}

impl TargetReport {
    fn detail(&self) -> Option<String> {
        let detail = match (&self.error, &self.summary, &self.process) {
            (Some(error), ..) => error.clone(),
            (None, Some(summary), _) => summary.clone(),
            (None, None, Some(process)) => format!("Watcher process is {process}"),
            (None, None, None) => return None,
        };
        Some(detail.chars().take(300).collect())
    }
}

fn targets(job: &Job, directory: &Path) -> Vec<Target> {
    job.request
        .targets
        .iter()
        .map(|id| {
            let final_path = directory.join("run").join(format!("{id}.json"));
            let progress_path = directory.join("run").join(format!("{id}.progress.json"));
            let report = if final_path.exists() {
                decode::<TargetReport>(&final_path)
            } else {
                decode::<TargetReport>(&progress_path)
            };
            match report {
                Ok(report) => Target {
                    id: id.clone(),
                    detail: report.detail(),
                    state: report.state,
                    progress: report.progress,
                },
                Err(_) => Target {
                    id: id.clone(),
                    state: if job.outcome == Outcome::Running {
                        TargetState::Waiting
                    } else {
                        TargetState::Unknown
                    },
                    progress: None,
                    detail: None,
                },
            }
        })
        .collect()
}

/// Read current progress. A lost worker lock without a result means unknown, not retryable.
///
/// # Errors
/// Rejects invalid IDs and unreadable/corrupt durable receipts.
pub fn read(inventory: &Inventory, id: &str) -> Result<Job, String> {
    let directory = directory(inventory, id)?;
    let mut job: Job = decode(&directory.join("request.json"))?;
    if job.request.id != id {
        return Err("fleet receipt ID mismatch".into());
    }
    if job.request.targets.is_empty()
        || job.request.targets.len() > 32
        || job
            .request
            .targets
            .iter()
            .any(|target| !crate::inventory::identifier(target))
    {
        return Err("invalid fleet receipt targets".into());
    }
    let result = directory.join("result.json");
    if result.exists() {
        let completed: Job = decode(&result)?;
        if completed.request != job.request {
            return Err("fleet result does not match request".into());
        }
        return Ok(completed);
    }
    let lock =
        File::open(directory.join("worker.lock")).map_err(|_| "cannot inspect fleet worker")?;
    match lock.try_lock() {
        Ok(()) => job.outcome = Outcome::Unknown,
        Err(std::fs::TryLockError::WouldBlock) => {}
        Err(_) => return Err("cannot inspect fleet worker lock".into()),
    }
    job.targets = targets(&job, &directory);
    Ok(job)
}

/// Admit an explicit request before any external process is started.
///
/// # Errors
/// Rejects changed inventories, disabled actions, invalid targets and reused IDs with new payloads.
pub fn admit(inventory: Inventory, request: Request) -> Result<Admission, String> {
    let directory = directory(&inventory, &request.id)?;
    if directory.exists() {
        let prior = read(&inventory, &request.id)?;
        if prior.request != request {
            return Err("fleet request ID was already used for another action".into());
        }
        return Ok(Admission::Existing(prior));
    }
    if request.revision != inventory.revision()? || request.targets.is_empty() {
        return Err(
            "fleet inventory changed or no machines were selected; refresh before confirming"
                .into(),
        );
    }
    inventory.targets(request.action, &request.targets)?;
    let parent = directory.parent().ok_or("missing fleet job parent")?;
    std::fs::create_dir_all(parent).map_err(|_| "cannot create fleet job storage")?;
    // Only Unix has a mode to restrict; elsewhere the builder is used as made.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&directory)
        .map_err(|_| "fleet request was already admitted or cannot be recorded")?;
    #[cfg(unix)]
    {
        File::open(parent)
            .and_then(|file| file.sync_all())
            .map_err(|_| "cannot sync fleet job parent")?;
    }
    let lock = create(&directory.join("worker.lock"))?;
    lock.try_lock()
        .map_err(|_| "cannot acquire fleet worker lock")?;
    let job = Job {
        request,
        requested_at: now(),
        completed_at: None,
        outcome: Outcome::Running,
        targets: Vec::new(),
    };
    persist(&directory.join("request.json"), &job)?;
    Ok(Admission::Start(Box::new(Pending {
        inventory,
        directory,
        lock,
        job,
    })))
}

impl Pending {
    #[must_use]
    pub fn job(&self) -> Job {
        let mut job = self.job.clone();
        job.targets = job
            .request
            .targets
            .iter()
            .map(|id| Target {
                id: id.clone(),
                state: TargetState::Waiting,
                progress: None,
                detail: None,
            })
            .collect();
        job
    }

    /// Execute the admitted snapshot once. Errors leave a durable, nonretryable receipt.
    ///
    /// # Errors
    /// Reports durable result-write failures; the caller must retain an unknown outcome.
    pub async fn run(self) -> Result<Job, String> {
        let (pending, prepared) = tokio::task::spawn_blocking(move || {
            let prepared = (|| {
                let mut command = crate::runner::command(
                    &self.inventory,
                    self.job.request.action,
                    &self.job.request.targets,
                    &self.directory,
                )?;
                command.stdout(create(&self.directory.join("stdout.log"))?);
                command.stderr(create(&self.directory.join("stderr.log"))?);
                Ok::<_, String>(command)
            })();
            (self, prepared)
        })
        .await
        .map_err(|_| "fleet preparation worker stopped")?;
        let execution = async {
            let mut command = prepared?;
            command
                .status()
                .await
                .map_err(|_| "cannot start fleet runner".to_owned())
        };
        let status = tokio::time::timeout(std::time::Duration::from_mins(125), execution).await;
        tokio::task::spawn_blocking(move || pending.finish(&status))
            .await
            .map_err(|_| "fleet result worker stopped")?
    }

    fn finish(
        mut self,
        status: &Result<Result<std::process::ExitStatus, String>, tokio::time::error::Elapsed>,
    ) -> Result<Job, String> {
        self.job.outcome = Outcome::Unknown;
        self.job.targets = targets(&self.job, &self.directory);
        self.job.outcome = match status {
            Err(_) => Outcome::Unknown,
            Ok(Err(_)) => Outcome::Failed,
            Ok(Ok(status)) => {
                if self.job.targets.iter().any(|target| {
                    matches!(
                        target.state,
                        TargetState::Unknown | TargetState::Running | TargetState::Waiting
                    )
                }) {
                    Outcome::Unknown
                } else if !status.success()
                    || self
                        .job
                        .targets
                        .iter()
                        .any(|target| target.state == TargetState::Failed)
                {
                    Outcome::Failed
                } else if self
                    .job
                    .targets
                    .iter()
                    .any(|target| target.state == TargetState::Skipped)
                {
                    Outcome::Partial
                } else {
                    Outcome::Succeeded
                }
            }
        };
        self.job.completed_at = Some(now());
        persist(&self.directory.join("result.json"), &self.job)?;
        drop(self.lock);
        Ok(self.job)
    }
}

/// Read bounded recent job history without exposing private runner output.
///
/// # Errors
/// Fails closed on corrupt records or an inventory exceeding the history scan limit.
pub fn history(inventory: &Inventory) -> Result<Vec<Job>, String> {
    let root = inventory.controller.jobs.join("dashboard");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(root).map_err(|_| "cannot list fleet history")?;
    let mut jobs = Vec::new();
    for entry in entries.take(1001) {
        let entry = entry.map_err(|_| "cannot read fleet history entry")?;
        let id = entry
            .file_name()
            .into_string()
            .map_err(|_| "invalid fleet history entry")?;
        jobs.push(read(inventory, &id)?);
    }
    if jobs.len() > 1000 {
        return Err("fleet history scan limit exceeded; archive old jobs".into());
    }
    jobs.sort_by_key(|job| std::cmp::Reverse(job.requested_at));
    jobs.truncate(100);
    Ok(jobs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_detail_prefers_the_failure_then_the_finding_then_the_watcher_state() {
        let detail = |json: &str| serde_json::from_str::<TargetReport>(json).unwrap().detail();
        assert_eq!(
            detail(r#"{"state":"failed","error":"stale","summary":"ready"}"#).as_deref(),
            Some("stale")
        );
        assert_eq!(
            detail(r#"{"state":"succeeded","error":null,"summary":"ready"}"#).as_deref(),
            Some("ready")
        );
        assert_eq!(
            detail(r#"{"state":"succeeded","process":"active","results":[]}"#).as_deref(),
            Some("Watcher process is active")
        );
        assert_eq!(detail(r#"{"state":"succeeded","results":[]}"#), None);
        let long = format!(r#"{{"state":"failed","error":"{}"}}"#, "x".repeat(900));
        assert_eq!(detail(&long).unwrap().len(), 300);
    }

    fn setup() -> (tempfile::TempDir, Inventory, Request) {
        let root = tempfile::tempdir().unwrap();
        let mut inventory = crate::inventory::tests::fixture();
        inventory.controller.jobs = root.path().join("jobs");
        inventory.controller.repository = root.path().join("repository");
        inventory.controller.binary = std::env::current_exe().unwrap();
        std::fs::create_dir_all(inventory.controller.repository.join("tools")).unwrap();
        let request = Request {
            id: "00000000-0000-0000-0000-000000000001".into(),
            action: Action::Status,
            targets: vec!["xenia".into()],
            revision: inventory.revision().unwrap(),
        };
        (root, inventory, request)
    }

    #[test]
    fn recorded_fleet_request_is_not_repeated_after_worker_loss() {
        let (_root, inventory, request) = setup();
        let Admission::Start(pending) = admit(inventory.clone(), request.clone()).unwrap() else {
            panic!("new request must be admitted");
        };
        assert_eq!(
            read(&inventory, &request.id).unwrap().outcome,
            Outcome::Running
        );
        drop(pending);
        let Admission::Existing(prior) = admit(inventory.clone(), request.clone()).unwrap() else {
            panic!("recorded request must not be resubmitted");
        };
        assert_eq!(prior.outcome, Outcome::Unknown);
        let mut changed = request;
        changed.action = Action::WatchStart;
        assert!(admit(inventory, changed).is_err());
    }

    #[test]
    fn fleet_admission_rejects_changed_inventory_before_recording() {
        let (_root, mut inventory, request) = setup();
        inventory.controller.allow_actions.clear();
        assert!(admit(inventory.clone(), request).is_err());
        assert!(!inventory.controller.jobs.join("dashboard").exists());
    }

    #[test]
    fn concurrent_fleet_admission_has_only_one_owner() {
        let (_root, inventory, request) = setup();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let inventory = inventory.clone();
                let request = request.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    admit(inventory, request)
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Ok(Admission::Start(_))))
                .count(),
            1
        );
        assert_eq!(
            read(&inventory, &request.id).unwrap().outcome,
            Outcome::Running
        );
        drop(results);
        assert_eq!(
            read(&inventory, &request.id).unwrap().outcome,
            Outcome::Unknown
        );
    }

    #[test]
    fn incomplete_fleet_receipt_never_authorizes_reexecution() {
        let (_root, inventory, request) = setup();
        let path = directory(&inventory, &request.id).unwrap();
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("request.json"), "{").unwrap();
        assert!(admit(inventory, request).is_err());
        assert!(!path.join("inventory.json").exists());
    }

    #[test]
    fn corrupt_fleet_receipt_cannot_read_target_paths_outside_job() {
        let (_root, inventory, request) = setup();
        let Admission::Start(pending) = admit(inventory.clone(), request.clone()).unwrap() else {
            panic!("new request must be admitted");
        };
        let mut corrupt = pending.job();
        corrupt.request.targets = vec!["../../outside".into()];
        std::fs::write(
            pending.directory.join("request.json"),
            serde_json::to_vec(&corrupt).unwrap(),
        )
        .unwrap();
        drop(pending);
        assert!(read(&inventory, &request.id).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn completed_fleet_job_executes_once_and_reports_target_results() {
        let (_root, inventory, request) = setup();
        std::fs::write(inventory.controller.repository.join("tools/fleet.sh"),
            "set -eu\nmkdir \"$MASH_FLEET_JOB_DIRECTORY\"\nprintf 'run\\n' >> \"$MASH_DEPLOY_LOGS/executions\"\nprintf '%s' '{\"state\":\"succeeded\"}' > \"$MASH_FLEET_JOB_DIRECTORY/xenia.json\"\n").unwrap();
        let Admission::Start(pending) = admit(inventory.clone(), request.clone()).unwrap() else {
            panic!("new request must be admitted");
        };
        let job = pending.run().await.unwrap();
        assert_eq!(job.outcome, Outcome::Succeeded);
        assert_eq!(job.targets[0].state, TargetState::Succeeded);
        assert!(matches!(
            admit(inventory.clone(), request).unwrap(),
            Admission::Existing(_)
        ));
        assert_eq!(
            std::fs::read_to_string(inventory.controller.jobs.join("executions")).unwrap(),
            "run\n"
        );
        assert_eq!(history(&inventory).unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn successful_runner_without_target_evidence_is_unknown() {
        let (_root, inventory, request) = setup();
        std::fs::write(
            inventory.controller.repository.join("tools/fleet.sh"),
            "exit 0\n",
        )
        .unwrap();
        let Admission::Start(pending) = admit(inventory, request).unwrap() else {
            panic!("new request must be admitted");
        };
        assert_eq!(pending.run().await.unwrap().outcome, Outcome::Unknown);
    }
}
