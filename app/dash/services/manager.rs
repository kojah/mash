//! Bounded calls to existing OS supervisors. No shell, elevation or installation.
use super::config::{Manager, Service};
use super::model::{Action, State};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

pub(super) struct Output {
    pub success: bool,
    pub text: String,
}

pub(super) fn supported(manager: Manager) -> bool {
    match manager {
        Manager::SystemdUser | Manager::SystemdSystem => cfg!(target_os = "linux"),
        Manager::Launchd => cfg!(target_os = "macos"),
        Manager::WindowsService | Manager::WindowsTask => cfg!(windows),
    }
}

async fn run(program: &str, args: &[String]) -> Result<Output, String> {
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "The configured service manager could not be started.".to_owned())?;
    let stdout = child.stdout.take().ok_or("Missing manager stdout")?;
    let stderr = child.stderr.take().ok_or("Missing manager stderr")?;
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut stdout = stdout.take(65_537);
        let mut stderr = stderr.take(65_537);
        let (read_out, read_err, status) = tokio::join!(
            stdout.read_to_end(&mut out),
            stderr.read_to_end(&mut err),
            child.wait()
        );
        read_out.map_err(|_| "Could not read service manager output")?;
        read_err.map_err(|_| "Could not read service manager errors")?;
        let status = status.map_err(|_| "Could not wait for service manager")?;
        if out.len() > 65_536 || err.len() > 65_536 {
            return Err("Service manager output exceeded its limit".to_owned());
        }
        Ok(Output {
            success: status.success(),
            text: crate::redaction::text(&format!(
                "{}{}",
                String::from_utf8_lossy(&out),
                String::from_utf8_lossy(&err)
            )),
        })
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => Err("Service manager timed out. The operation outcome is unknown; check status before trying again.".into()),
    }
}

fn systemd_args(service: &Service, verb: &str) -> Vec<String> {
    let mut args = vec!["--no-pager".into(), "--no-ask-password".into()];
    if service.manager == Manager::SystemdUser {
        args.push("--user".into());
    }
    args.push(verb.into());
    args.push(service.unit.clone());
    args
}

async fn launchd_target(service: &Service) -> Result<String, String> {
    let uid = run("id", &["-u".into()]).await?;
    let uid = uid.text.trim();
    if uid.is_empty() || !uid.bytes().all(|c| c.is_ascii_digit()) {
        return Err("Could not determine the launchd user domain".into());
    }
    Ok(format!("gui/{uid}/{}", service.unit))
}

pub(super) async fn status(service: &Service) -> Result<(State, String), String> {
    tokio::time::timeout(Duration::from_secs(5), status_inner(service))
        .await
        .map_err(|_| "Service status check timed out".to_owned())?
}

async fn status_inner(service: &Service) -> Result<(State, String), String> {
    if !supported(service.manager) {
        return Err("This service manager is not supported on this machine.".into());
    }
    match service.manager {
        Manager::SystemdUser | Manager::SystemdSystem => {
            let mut args = systemd_args(service, "show");
            args.extend(["--property=LoadState,ActiveState,SubState,MainPID".into()]);
            let out = run("systemctl", &args).await?;
            if !out.success || !out.text.lines().any(|line| line == "LoadState=loaded") {
                return Err("The configured systemd unit could not be inspected.".into());
            }
            let state = if out.text.lines().any(|line| line == "ActiveState=active") {
                State::Healthy
            } else if out.text.lines().any(|line| line == "ActiveState=inactive") {
                State::Stopped
            } else if out.text.lines().any(|line| line == "ActiveState=failed") {
                State::Failed
            } else {
                State::Unknown
            };
            Ok((state, out.text))
        }
        Manager::Launchd => launchd_status(service).await,
        Manager::WindowsService => {
            let out = run("sc.exe", &["query".into(), service.unit.clone()]).await?;
            if !out.success {
                return Err("Windows did not report this service.".into());
            }
            // sc's symbolic state names are stable even when surrounding text is localized.
            let state = if out.text.split_whitespace().any(|word| word == "RUNNING") {
                State::Healthy
            } else if out.text.split_whitespace().any(|word| word == "STOPPED") {
                State::Stopped
            } else {
                State::Unknown
            };
            Ok((state, out.text))
        }
        Manager::WindowsTask => {
            let script = format!(
                "$ErrorActionPreference='Stop'; (Get-ScheduledTask -TaskName '{}' -TaskPath '\\').State.ToString()",
                service.unit
            );
            let out = run(
                "powershell.exe",
                &[
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-Command".into(),
                    script,
                ],
            )
            .await?;
            if !out.success {
                return Err("Windows did not report this scheduled task.".into());
            }
            let state = match out.text.trim() {
                "Running" => State::Healthy,
                "Ready" | "Disabled" => State::Stopped,
                _ => State::Unknown,
            };
            Ok((state, out.text))
        }
    }
}

async fn launchd_status(service: &Service) -> Result<(State, String), String> {
    let target = launchd_target(service).await?;
    let out = run("launchctl", &["print".into(), target.clone()]).await?;
    if !out.success {
        if out.text.contains("Could not find service")
            && service.plist.as_ref().is_some_and(|path| path.is_file())
        {
            let domain = target.rsplit_once('/').ok_or("Invalid launchd target")?.0;
            if run("launchctl", &["print".into(), domain.into()])
                .await?
                .success
            {
                return Ok((
                    State::Stopped,
                    "Job is unloaded; the configured plist is available for Start.".into(),
                ));
            }
        }
        return Err("launchd did not report this job. It may be unloaded or inaccessible.".into());
    }
    let state = if out
        .text
        .lines()
        .any(|line| line.trim() == "state = running")
    {
        State::Healthy
    } else {
        State::Stopped
    };
    // `launchctl print` can include environment variables. Return only
    // process-state lines, never its complete diagnostic document.
    let detail = out
        .text
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.starts_with("state = ")
                || line.starts_with("pid = ")
                || line.starts_with("last exit code = ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok((state, detail))
}

pub(super) async fn action(service: &Service, action: Action) -> Result<Output, String> {
    if !supported(service.manager) || !service.allow_control {
        return Err("Control is not enabled for this service.".into());
    }
    let verb = match action {
        Action::Start => "start",
        Action::Stop => "stop",
        Action::Restart => "restart",
    };
    match service.manager {
        Manager::SystemdUser | Manager::SystemdSystem => {
            run("systemctl", &systemd_args(service, verb)).await
        }
        Manager::Launchd => {
            let target = launchd_target(service).await?;
            let args = match action {
                Action::Start => {
                    let plist = service
                        .plist
                        .as_ref()
                        .ok_or("No launchd plist configured")?;
                    let domain = target.rsplit_once('/').ok_or("Invalid launchd target")?.0;
                    vec![
                        "bootstrap".into(),
                        domain.into(),
                        plist.to_string_lossy().into_owned(),
                    ]
                }
                Action::Stop => vec!["bootout".into(), target],
                Action::Restart => vec!["kickstart".into(), "-k".into(), target],
            };
            run("launchctl", &args).await
        }
        Manager::WindowsService | Manager::WindowsTask => {
            // Unit names were restricted to identifiers at configuration load.
            let script = if service.manager == Manager::WindowsService {
                format!(
                    "$ErrorActionPreference='Stop'; {verb}-Service -Name '{}'",
                    service.unit
                )
            } else {
                match action {
                    Action::Start => format!(
                        "$ErrorActionPreference='Stop'; Start-ScheduledTask -TaskName '{}' -TaskPath '\\'",
                        service.unit
                    ),
                    Action::Stop => format!(
                        "$ErrorActionPreference='Stop'; Stop-ScheduledTask -TaskName '{}' -TaskPath '\\'",
                        service.unit
                    ),
                    Action::Restart => {
                        return Err("Scheduled tasks support Start and Stop, not Restart.".into());
                    }
                }
            };
            run(
                "powershell.exe",
                &[
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-Command".into(),
                    script,
                ],
            )
            .await
        }
    }
}

pub(super) async fn logs(service: &Service) -> Result<String, String> {
    if let Some(path) = service.log.clone() {
        return tokio::task::spawn_blocking(move || {
            use std::io::{Read, Seek};
            let mut file =
                std::fs::File::open(path).map_err(|_| "Could not open configured log")?;
            let meta = file
                .metadata()
                .map_err(|_| "Could not inspect configured log")?;
            if !meta.is_file() {
                return Err("Configured log is not a regular file".to_owned());
            }
            file.seek(std::io::SeekFrom::Start(meta.len().saturating_sub(65_536)))
                .map_err(|_| "Could not seek log")?;
            let mut bytes = Vec::new();
            file.take(65_536)
                .read_to_end(&mut bytes)
                .map_err(|_| "Could not read log")?;
            Ok(crate::redaction::text(&String::from_utf8_lossy(&bytes)))
        })
        .await
        .map_err(|_| "Log worker failed")?;
    }
    if matches!(
        service.manager,
        Manager::SystemdUser | Manager::SystemdSystem
    ) && supported(service.manager)
    {
        let mut args = vec![
            "--no-pager".into(),
            "-n".into(),
            "100".into(),
            "-o".into(),
            "short-iso".into(),
        ];
        if service.manager == Manager::SystemdUser {
            args.push("--user".into());
        }
        args.extend(["-u".into(), service.unit.clone()]);
        let out = run("journalctl", &args).await?;
        return if out.success {
            Ok(out.text)
        } else {
            Err("Could not read service journal".into())
        };
    }
    Err("No log source is configured for this service.".into())
}
