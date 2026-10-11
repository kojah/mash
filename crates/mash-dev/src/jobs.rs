//! Thin job adapter: Ansible owns fleet policy; Runner owns process/event artifacts.
use crate::{
    RefreshMode,
    config::Config,
    report::{Report, State},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};
use tokio::sync::Mutex;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case", tag = "operation", content = "mode")]
pub enum Operation {
    Deploy,
    Refresh(RefreshMode),
    Check,
}

#[derive(Clone, Serialize)]
pub struct Outcome {
    #[serde(flatten)]
    pub report: Report,
    pub task: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct Job {
    pub id: u64,
    pub state: State,
    operation: Operation,
    logs_directory: std::path::PathBuf,
    pub nodes: BTreeMap<String, Outcome>,
}

#[derive(Default)]
struct Registry {
    next: u64,
    jobs: BTreeMap<u64, Job>,
}

#[derive(Clone)]
pub struct Runner {
    config: Arc<Config>,
    registry: Arc<Mutex<Registry>>,
}

#[derive(Serialize)]
struct Variables<'a> {
    #[serde(rename = "mash_selected_nodes")]
    selected_nodes: &'a [String],
    #[serde(rename = "mash_repository")]
    repository: &'a Path,
    #[serde(rename = "mash_job_directory")]
    job_directory: &'a Path,
    #[serde(rename = "mash_operation")]
    operation: Mode,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Mode {
    Deploy,
    Check,
    Sync,
    LocalViews,
}

#[derive(Serialize)]
struct Settings {
    job_timeout: u64,
    idle_timeout: u64,
}

impl Runner {
    pub fn new(mut config: Config) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        config.logs = config.logs.join(format!("{now}-{}", std::process::id()));
        Self {
            config: Arc::new(config),
            registry: Arc::default(),
        }
    }

    pub async fn get(&self, id: u64) -> Result<Job, String> {
        self.update(id).await;
        self.registry
            .lock()
            .await
            .jobs
            .get(&id)
            .cloned()
            .ok_or_else(|| "unknown or expired job ID".into())
    }

    pub async fn start(&self, nodes: Vec<String>, operation: Operation) -> Result<Job, String> {
        if nodes.is_empty()
            || nodes.len() > 32
            || nodes.iter().collect::<BTreeSet<_>>().len() != nodes.len()
            || nodes.iter().any(|node| !self.config.nodes.contains(node))
        {
            return Err("select 1..32 distinct configured nodes".into());
        }
        let mut registry = self.registry.lock().await;
        if registry
            .jobs
            .values()
            .any(|job| job.state == State::Running)
        {
            return Err("a dev job is already running; poll it before starting another".into());
        }
        while registry.jobs.len() >= 32 {
            registry.jobs.pop_first();
        }
        registry.next += 1;
        let id = registry.next;
        let job = Job {
            id,
            state: State::Running,
            operation,
            logs_directory: self.config.logs.join(id.to_string()),
            nodes: nodes
                .iter()
                .map(|node| {
                    (
                        node.clone(),
                        Outcome {
                            report: Report::default(),
                            task: None,
                        },
                    )
                })
                .collect(),
        };
        registry.jobs.insert(id, job.clone());
        let runner = self.clone();
        tokio::spawn(async move {
            runner.perform(id, nodes, operation).await;
        });
        Ok(job)
    }

    async fn perform(&self, id: u64, nodes: Vec<String>, operation: Operation) {
        let result = self.run(id, &nodes, operation).await;
        self.update(id).await;
        let mut registry = self.registry.lock().await;
        if let Some(job) = registry.jobs.get_mut(&id) {
            for outcome in job.nodes.values_mut() {
                if outcome.report.state == State::Running {
                    outcome.report.state = State::Failed;
                    outcome.report.error = Some(
                        "playbook did not verify this target; inspect private Runner artifacts"
                            .into(),
                    );
                }
            }
            job.state = if result.is_ok()
                && job
                    .nodes
                    .values()
                    .all(|node| matches!(node.report.state, State::Succeeded | State::Skipped))
            {
                State::Succeeded
            } else {
                State::Failed
            };
            if let Err(error) = result {
                for outcome in job
                    .nodes
                    .values_mut()
                    .filter(|outcome| outcome.report.state == State::Failed)
                {
                    outcome.report.error = Some(error.clone());
                }
            }
        }
    }

    async fn run(&self, id: u64, nodes: &[String], operation: Operation) -> Result<(), String> {
        let directory = self.config.logs.join(id.to_string());
        private_directory(&directory.join("env"))?;
        let (playbook, mode) = match operation {
            Operation::Deploy => (&self.config.playbooks.deploy, Mode::Deploy),
            Operation::Check => (&self.config.playbooks.check, Mode::Check),
            Operation::Refresh(RefreshMode::Sync) => (&self.config.playbooks.refresh, Mode::Sync),
            Operation::Refresh(RefreshMode::LocalViews) => {
                (&self.config.playbooks.refresh, Mode::LocalViews)
            }
        };
        private_json(
            &directory.join("env/extravars"),
            &Variables {
                selected_nodes: nodes,
                repository: &self.config.repository,
                job_directory: &directory,
                operation: mode,
            },
        )?;
        private_json(
            &directory.join("env/settings"),
            &Settings {
                job_timeout: self.config.timeout_seconds,
                idle_timeout: 0,
            },
        )?;
        let log = private_file(&directory.join("runner.log"))?;
        let stderr = log.try_clone().map_err(|_| "cannot open Runner log")?;
        let mut command = self.config.runner.process(&self.config.repository);
        command
            .arg("run")
            .arg(&directory)
            .arg("--project-dir")
            .arg(&self.config.repository)
            .arg("--inventory")
            .arg(&self.config.inventory)
            .arg("--playbook")
            .arg(playbook)
            .args(["--ident", "job", "--json"])
            .stdin(std::process::Stdio::null())
            .stdout(log)
            .stderr(stderr);
        let status = command
            .status()
            .await
            .map_err(|_| "cannot start trusted Ansible Runner; inspect private logs")?;
        if status.success() {
            Ok(())
        } else {
            Err("Ansible rollout failed or timed out; inspect private artifacts and target readiness before retrying".into())
        }
    }

    async fn update(&self, id: u64) {
        let directory = self.config.logs.join(id.to_string());
        let reports = read_reports(&directory, &self.config.nodes);
        let events = read_events(&directory.join("artifacts/job/job_events"));
        if let Some(job) = self.registry.lock().await.jobs.get_mut(&id) {
            for event in events {
                if let Some(host) = event.event_data.host
                    && let Some(node) = job.nodes.get_mut(&host)
                {
                    node.task = event.event_data.task;
                }
            }
            for (name, report) in reports {
                if let Some(node) = job.nodes.get_mut(&name) {
                    node.report = report;
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct Event {
    counter: u64,
    #[serde(default)]
    event_data: EventData,
}
#[derive(Default, Deserialize)]
struct EventData {
    host: Option<String>,
    task: Option<String>,
}

fn read_events(directory: &Path) -> Vec<Event> {
    let mut events = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<Event>(&bytes).ok())
        .collect::<Vec<_>>();
    events.sort_by_key(|event| event.counter);
    events
}

fn read_reports(directory: &Path, nodes: &[String]) -> Vec<(String, Report)> {
    nodes
        .iter()
        .filter_map(|node| {
            let final_path = directory.join(format!("{node}.json"));
            let progress = directory.join(format!("{node}.progress.json"));
            let bytes = std::fs::read(final_path)
                .or_else(|_| std::fs::read(progress))
                .ok()?;
            let report = serde_json::from_slice(&bytes).ok()?;
            Some((node.clone(), report))
        })
        .collect()
}

fn private_directory(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|_| "cannot create private Runner directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for directory in [path, path.parent().ok_or("missing parent")?] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "cannot secure Runner directory")?;
        }
    }
    Ok(())
}

fn private_file(path: &Path) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|_| "cannot open private Runner artifact".into())
}

fn private_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(private_file(path)?, value)
        .map_err(|_| "cannot encode Runner input".into())
}

#[cfg(test)]
mod tests;
