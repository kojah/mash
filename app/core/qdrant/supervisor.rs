//! The user's own service supervisor: a systemd user unit on Linux, a
//! launchd `LaunchAgent` on macOS and a logon scheduled task on Windows. None
//! needs administrator rights. Definitions and commands are plain data, so
//! tests check them without running a supervisor.
use super::{Layout, Ports, install_error};
use crate::AppError;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Supervisor {
    SystemdUser,
    LaunchAgent,
    ScheduledTask,
}

/// What the supervisor reports about the installed service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ServiceState {
    /// The supervisor has no such service.
    NotInstalled,
    Running,
    Starting,
    Stopped,
    /// The supervisor gave up after it exited with an error.
    Failed,
    /// The supervisor could not be asked.
    Unknown,
}

impl ServiceState {
    pub(crate) fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Starting)
    }
}

/// Qdrant's own configuration file (YAML, Qdrant's format). It binds only
/// to loopback, keeps telemetry off and places data under `layout.data`.
pub(crate) fn qdrant_config(layout: &Layout, ports: Ports) -> String {
    let quoted = |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "''"));
    let mut text = format!(
        "# Written by `mash qdrant install`, which replaces it on every run.\n\
         service:\n  host: 127.0.0.1\n  http_port: {}\n  grpc_port: {}\n\
         storage:\n  storage_path: {}\n  snapshots_path: {}\n\
         telemetry_disabled: true\n\
         logger:\n  log_level: INFO\n",
        ports.http,
        ports.grpc,
        quoted(&layout.storage()),
        quoted(&layout.snapshots()),
    );
    // A scheduled task has no output stream to capture, so Qdrant writes
    // its own log; an unbuffered one survives a forced stop.
    if layout.platform.supervisor() == Supervisor::ScheduledTask {
        text.push_str("  on_disk:\n    enabled: true\n    log_file: ");
        text.push_str(&quoted(&layout.log()));
        text.push_str("\n    log_level: INFO\n    buffer_size_bytes: 1\n");
    }
    text
}

/// The supervisor's definition of the service running `binary`.
pub(crate) fn definition(layout: &Layout, binary: &Path) -> Result<String, AppError> {
    for path in [binary, &layout.root, &layout.qdrant_config(), &layout.log()] {
        let text = path.to_str().ok_or_else(|| unexpressible(path))?;
        if text.chars().any(|c| c.is_control() || c == '"') {
            return Err(unexpressible(path));
        }
    }
    let text = |path: &Path| path.display().to_string();
    Ok(match layout.platform.supervisor() {
        Supervisor::SystemdUser => {
            // `%` starts a specifier and `\` an escape in systemd values.
            let escape = |path: &Path| text(path).replace('\\', "\\\\").replace('%', "%%");
            format!(
                "# Written by `mash qdrant install`; rerun it to repair, or run `mash qdrant uninstall`.\n\
                 [Unit]\nDescription=Qdrant search service for Mash\n\n\
                 [Service]\nType=simple\nWorkingDirectory={}\n\
                 ExecStart=\"{}\" --config-path \"{}\"\n\
                 Restart=on-failure\nRestartSec=5\nTimeoutStopSec=60\n\n\
                 [Install]\nWantedBy=default.target\n",
                text(&layout.root).replace('%', "%%"),
                escape(binary),
                escape(&layout.qdrant_config()),
            )
        }
        Supervisor::LaunchAgent => {
            let string = |value: &str| format!("<string>{}</string>", xml(value));
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 <!-- Written by `mash qdrant install`; rerun it to repair, or run `mash qdrant uninstall`. -->\n\
                 <plist version=\"1.0\"><dict>\n\
                 <key>Label</key>{}\n\
                 <key>ProgramArguments</key><array>{}<string>--config-path</string>{}</array>\n\
                 <key>WorkingDirectory</key>{}\n\
                 <key>RunAtLoad</key><true/>\n\
                 <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n\
                 <key>ThrottleInterval</key><integer>10</integer>\n\
                 <key>SoftResourceLimits</key><dict><key>NumberOfFiles</key><integer>10240</integer></dict>\n\
                 <key>StandardOutPath</key>{}\n\
                 <key>StandardErrorPath</key>{}\n\
                 </dict></plist>\n",
                string(&layout.service),
                string(&text(binary)),
                string(&text(&layout.qdrant_config())),
                string(&text(&layout.root)),
                string(&text(&layout.log())),
                string(&text(&layout.log())),
            )
        }
        Supervisor::ScheduledTask => {
            let user = &layout.account.user;
            if user.is_empty() {
                return Err(install_error(
                    "USERNAME is not set, so the logon task has no user",
                ));
            }
            // A console program started at logon opens a window; the headless
            // console host runs it without one.
            let conhost = format!("{}\\System32\\conhost.exe", layout.account.system_root);
            format!(
                "<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n\
                 <RegistrationInfo><Description>Qdrant search service for Mash. Written by mash qdrant install; rerun it to repair, or run mash qdrant uninstall.</Description></RegistrationInfo>\n\
                 <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>\n\
                 <Principals><Principal id=\"Author\"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>\n\
                 <Settings>\
                 <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\
                 <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\
                 <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\
                 <AllowHardTerminate>true</AllowHardTerminate>\
                 <StartWhenAvailable>true</StartWhenAvailable>\
                 <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>\
                 <AllowStartOnDemand>true</AllowStartOnDemand>\
                 <Enabled>true</Enabled>\
                 <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\
                 <Priority>5</Priority>\
                 <RestartOnFailure><Interval>PT1M</Interval><Count>3</Count></RestartOnFailure>\
                 </Settings>\n\
                 <Actions Context=\"Author\"><Exec><Command>{}</Command><Arguments>{}</Arguments><WorkingDirectory>{}</WorkingDirectory></Exec></Actions>\n\
                 </Task>\n",
                xml(&conhost),
                xml(&format!(
                    "--headless \"{}\" --config-path \"{}\"",
                    text(binary),
                    text(&layout.qdrant_config())
                )),
                xml(&text(&layout.root)),
                user = xml(user),
            )
        }
    })
}

fn unexpressible(path: &Path) -> AppError {
    install_error(format!(
        "{} cannot be written into a service definition (it is not UTF-8, or has quotes or control characters); set a plain data directory and rerun",
        path.display()
    ))
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A PowerShell single-quoted literal.
fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// A supervisor command line and whether its failure matters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    /// Stopping or removing something already absent is not a failure.
    pub(crate) tolerate_failure: bool,
}

fn invocation(program: &str, args: &[&str]) -> Invocation {
    Invocation {
        program: program.to_owned(),
        args: args.iter().map(|&arg| arg.to_owned()).collect(),
        tolerate_failure: false,
    }
}

fn tolerant(mut invocation: Invocation) -> Invocation {
    invocation.tolerate_failure = true;
    invocation
}

fn powershell(script: &str) -> Invocation {
    invocation(
        "powershell.exe",
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ],
    )
}

/// The steps a supervisor can be asked to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Ask for the state; parsed by [`parse_state`].
    Query,
    /// Stop the running service but keep it registered.
    Stop,
    /// Register the current definition, enable it at login and (re)start it.
    Start,
    /// Stop it and remove its registration.
    Remove,
}

/// The command lines for `step`. `domain` is launchd's `gui/UID`.
pub(crate) fn commands(layout: &Layout, step: Step, domain: &str) -> Vec<Invocation> {
    let service = layout.service.as_str();
    let definition = layout.definition().display().to_string();
    match layout.platform.supervisor() {
        Supervisor::SystemdUser => {
            let systemctl = |args: &[&str]| {
                let mut all = vec!["--user"];
                all.extend_from_slice(args);
                invocation("systemctl", &all)
            };
            match step {
                Step::Query => vec![systemctl(&[
                    "show",
                    service,
                    "--property=LoadState,ActiveState",
                ])],
                Step::Stop => vec![systemctl(&["stop", service])],
                Step::Start => vec![
                    systemctl(&["daemon-reload"]),
                    systemctl(&["enable", service]),
                    systemctl(&["restart", service]),
                ],
                Step::Remove => vec![
                    tolerant(systemctl(&["disable", "--now", service])),
                    tolerant(systemctl(&["reset-failed", service])),
                ],
            }
        }
        Supervisor::LaunchAgent => {
            let target = format!("{domain}/{service}");
            match step {
                Step::Query => vec![tolerant(invocation("launchctl", &["print", &target]))],
                Step::Stop | Step::Remove => {
                    vec![tolerant(invocation("launchctl", &["bootout", &target]))]
                }
                // `RunAtLoad` and `KeepAlive` spawns are deferred, possibly
                // indefinitely, while the domain is in on-demand-only mode;
                // `kickstart` is an explicit demand that starts it regardless.
                Step::Start => vec![
                    tolerant(invocation("launchctl", &["bootout", &target])),
                    invocation("launchctl", &["enable", &target]),
                    invocation("launchctl", &["bootstrap", domain, &definition]),
                    invocation("launchctl", &["kickstart", &target]),
                ],
            }
        }
        Supervisor::ScheduledTask => {
            let name = literal(service);
            // Ending the task ends the console host; wait for Qdrant beneath
            // it to exit, then end any copy of Mash's Qdrant still running.
            let bin = literal(&format!("{}\\", layout.root.join("bin").display()));
            let halt = format!(
                "Stop-ScheduledTask -TaskName {name} -ErrorAction SilentlyContinue; \
                 $owned = @(Get-Process -Name qdrant -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -and $_.Path.StartsWith({bin}, [StringComparison]::OrdinalIgnoreCase) }}); \
                 if ($owned) {{ $owned | Wait-Process -Timeout 15 -ErrorAction SilentlyContinue; $owned | Where-Object {{ -not $_.HasExited }} | Stop-Process -Force }}"
            );
            match step {
                Step::Query => vec![powershell(&format!(
                    "$task = Get-ScheduledTask -TaskName {name} -ErrorAction SilentlyContinue; if ($task) {{ [string]$task.State }} else {{ 'Missing' }}"
                ))],
                Step::Stop => vec![powershell(&halt)],
                Step::Start => vec![
                    powershell(&halt),
                    powershell(&format!(
                        "Register-ScheduledTask -TaskName {name} -Xml (Get-Content -LiteralPath {} -Raw) -Force | Out-Null",
                        literal(&definition)
                    )),
                    powershell(&format!("Start-ScheduledTask -TaskName {name}")),
                ],
                Step::Remove => vec![
                    powershell(&halt),
                    tolerant(powershell(&format!(
                        "Unregister-ScheduledTask -TaskName {name} -Confirm:$false -ErrorAction Stop"
                    ))),
                ],
            }
        }
    }
}

/// The service state from the query's exit status and standard output.
pub(crate) fn parse_state(supervisor: Supervisor, succeeded: bool, output: &str) -> ServiceState {
    match supervisor {
        Supervisor::SystemdUser => {
            if !succeeded {
                return ServiceState::Unknown;
            }
            let value = |key: &str| {
                output
                    .lines()
                    .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
                    .map(str::trim)
            };
            if value("LoadState") == Some("not-found") {
                return ServiceState::NotInstalled;
            }
            match value("ActiveState") {
                Some("active" | "reloading" | "refreshing") => ServiceState::Running,
                Some("activating") => ServiceState::Starting,
                Some("inactive" | "deactivating") => ServiceState::Stopped,
                Some("failed") => ServiceState::Failed,
                _ => ServiceState::Unknown,
            }
        }
        Supervisor::LaunchAgent => {
            if !succeeded {
                return ServiceState::NotInstalled;
            }
            match output
                .lines()
                .find_map(|line| line.trim().strip_prefix("state = "))
                .map(str::trim)
            {
                Some("running") => ServiceState::Running,
                Some("spawn scheduled" | "xpcproxy") => ServiceState::Starting,
                Some(_) => ServiceState::Stopped,
                None => ServiceState::Unknown,
            }
        }
        Supervisor::ScheduledTask if succeeded => match output.trim() {
            "Missing" => ServiceState::NotInstalled,
            "Running" => ServiceState::Running,
            "Queued" => ServiceState::Starting,
            "Ready" | "Disabled" => ServiceState::Stopped,
            _ => ServiceState::Unknown,
        },
        Supervisor::ScheduledTask => ServiceState::Unknown,
    }
}

/// How supervisor steps are carried out.
pub(crate) enum Control {
    /// Run the real supervisor's commands.
    System,
    /// Simulate a supervisor in tests.
    #[cfg(test)]
    Fake(std::sync::Arc<super::tests::FakeSupervisor>),
}

/// Bounds each supervisor command; none should take this long.
const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

impl Control {
    pub(crate) async fn state(&self, layout: &Layout) -> Result<ServiceState, AppError> {
        match self {
            Self::System => {
                let domain = launchd_domain(layout).await?;
                let [query] = commands(layout, Step::Query, &domain)
                    .try_into()
                    .map_err(|_| AppError::internal("one query command"))?;
                Ok(match run(&query).await {
                    Ok((succeeded, output)) => {
                        parse_state(layout.platform.supervisor(), succeeded, &output)
                    }
                    Err(_) => ServiceState::Unknown,
                })
            }
            #[cfg(test)]
            Self::Fake(fake) => Ok(fake.state()),
        }
    }

    pub(crate) async fn stop(&self, layout: &Layout) -> Result<(), AppError> {
        self.perform(layout, Step::Stop).await
    }

    pub(crate) async fn start(&self, layout: &Layout) -> Result<(), AppError> {
        self.perform(layout, Step::Start).await
    }

    pub(crate) async fn remove(&self, layout: &Layout) -> Result<(), AppError> {
        self.perform(layout, Step::Remove).await
    }

    async fn perform(&self, layout: &Layout, step: Step) -> Result<(), AppError> {
        match self {
            Self::System => {
                let domain = launchd_domain(layout).await?;
                for command in commands(layout, step, &domain) {
                    let (succeeded, output) = run(&command).await?;
                    if !succeeded && !command.tolerate_failure {
                        return Err(supervisor_failure(layout, &command, &output));
                    }
                    if step == Step::Stop || command.args.first().is_some_and(|a| a == "bootout") {
                        wait_unloaded(layout, &domain).await;
                    }
                }
                Ok(())
            }
            #[cfg(test)]
            Self::Fake(fake) => fake.perform(layout, step).await,
        }
    }
}

/// launchd finishes a `bootout` after the command returns; bootstrapping
/// before then fails, so wait (bounded) until the label is gone.
async fn wait_unloaded(layout: &Layout, domain: &str) {
    if layout.platform.supervisor() != Supervisor::LaunchAgent {
        return;
    }
    let Ok([query]) = <[Invocation; 1]>::try_from(commands(layout, Step::Query, domain)) else {
        return;
    };
    for _ in 0..40 {
        if matches!(run(&query).await, Ok((false, _))) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

async fn launchd_domain(layout: &Layout) -> Result<String, AppError> {
    if layout.platform.supervisor() != Supervisor::LaunchAgent {
        return Ok(String::new());
    }
    let (succeeded, output) = run(&invocation("id", &["-u"])).await?;
    let uid = output.trim();
    if !succeeded || uid.is_empty() || !uid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(install_error(
            "could not determine this user's ID for launchd",
        ));
    }
    Ok(format!("gui/{uid}"))
}

/// Run one command; `Err` only when it could not be started or timed out.
async fn run(command: &Invocation) -> Result<(bool, String), AppError> {
    let child = tokio::process::Command::new(&command.program)
        .args(&command.args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(COMMAND_TIMEOUT, child)
        .await
        .map_err(|_| {
            install_error(format!(
                "`{}` did not finish within {} s",
                command.program,
                COMMAND_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|error| {
            install_error(format!(
                "could not run `{}` ({error}); `mash qdrant install` needs the user's service manager on this machine. Run Qdrant yourself, for example with Docker, as the installation guide describes",
                command.program
            ))
        })?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((output.status.success(), text))
}

fn supervisor_failure(layout: &Layout, command: &Invocation, output: &str) -> AppError {
    let detail: String = output.trim().chars().take(600).collect();
    let hint = match layout.platform.supervisor() {
        Supervisor::SystemdUser => {
            " If there is no systemd user session here (some containers and WSL setups), run Qdrant yourself, for example with Docker."
        }
        Supervisor::LaunchAgent | Supervisor::ScheduledTask => "",
    };
    install_error(format!(
        "`{} {}` failed: {detail}.{hint}",
        command.program,
        command.args.join(" ")
    ))
}
