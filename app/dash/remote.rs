//! `mash dash remote`: keep this machine's dashboard running behind a sign-in
//! and give it an HTTPS address inside the user's tailnet. It writes a token
//! file and a user service (systemd on Linux, a launch agent on macOS), asks
//! Tailscale to forward the address to loopback, and hands back what a phone
//! needs: the address, a one-time code and a QR code. Unless asked for a
//! public address, nothing here is reachable from the public internet.
use crate::AppError;
use crate::core::Paths;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const UNIT: &str = "mash-dash.service";
const LABEL: &str = "dev.mash.dash";
const TOKEN_VARIABLE: &str = "MASH_DASH_TOKEN";
/// Where Tailscale's macOS app keeps its command when it is not on PATH.
const TAILSCALE_APP: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";

/// What a user service is called under each service manager, and who wrote it.
#[derive(Clone, Copy)]
pub(crate) struct ServiceName {
    pub(crate) unit: &'static str,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    /// The command to rerun, named in the definition for whoever reads it.
    pub(crate) written_by: &'static str,
}

const DASHBOARD: ServiceName = ServiceName {
    unit: UNIT,
    label: LABEL,
    description: "Mash dashboard for this tailnet",
    written_by: "mash dash remote",
};

/// The user's own service manager on this system.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Supervisor {
    Systemd,
    Launchd,
}

impl Supervisor {
    pub(crate) fn current() -> Option<Self> {
        if cfg!(target_os = "linux") {
            Some(Self::Systemd)
        } else if cfg!(target_os = "macos") {
            Some(Self::Launchd)
        } else {
            None
        }
    }
}

/// Where the dashboard is served and what it shows.
#[derive(Clone, Copy)]
pub(crate) struct Options {
    pub(crate) port: u16,
    /// Show only the shared index: this machine has no history of its own.
    pub(crate) shared_only: bool,
    /// Reachable from the public internet through Tailscale Funnel, not only
    /// from the user's own tailnet.
    pub(crate) public: bool,
}

/// The ports Tailscale Funnel can publish.
const FUNNEL_PORTS: [u16; 3] = [443, 8443, 10000];

pub(crate) enum Action {
    /// Set up or repair the service and print how to open it.
    Setup,
    /// A fresh one-time code from the running dashboard.
    Code,
    /// The sign-in QR code again.
    Qr,
    /// Stop serving and remove the service; the token file stays.
    Remove,
}

#[derive(serde::Serialize)]
struct Report {
    dashboard: String,
    service: &'static str,
    /// Who can open the address.
    #[serde(skip_serializing_if = "Option::is_none")]
    reach: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code_minutes: Option<u64>,
    /// Whether the service was given the shared index's node token.
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_index: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<&'static str>,
}

/// A user service and the files that belong to it.
pub(crate) struct Layout {
    pub(crate) supervisor: Supervisor,
    pub(crate) name: ServiceName,
    /// The service's environment; for the dashboard, its tokens.
    pub(crate) token_file: PathBuf,
    /// The systemd unit or the launch agent property list.
    pub(crate) unit_file: PathBuf,
    /// Where a launch agent's output goes; systemd keeps its own journal.
    pub(crate) log: PathBuf,
}

impl Layout {
    /// `environment` and `log` are the service's own files.
    pub(crate) fn resolve(
        supervisor: Supervisor,
        name: ServiceName,
        environment: PathBuf,
        log: PathBuf,
    ) -> Result<Self, AppError> {
        let base = directories::BaseDirs::new()
            .ok_or_else(|| AppError::configuration("cannot find the home directory"))?;
        Ok(Self {
            supervisor,
            name,
            token_file: environment,
            unit_file: match supervisor {
                Supervisor::Systemd => base.config_dir().join("systemd/user").join(name.unit),
                Supervisor::Launchd => base
                    .home_dir()
                    .join("Library/LaunchAgents")
                    .join(format!("{}.plist", name.label)),
            },
            log,
        })
    }

    /// The name the service manager knows the service by.
    pub(crate) fn service(&self) -> &'static str {
        match self.supervisor {
            Supervisor::Systemd => self.name.unit,
            Supervisor::Launchd => self.name.label,
        }
    }

    /// Where to look when the service does not come up.
    pub(crate) fn logs(&self) -> String {
        match self.supervisor {
            Supervisor::Systemd => format!("`journalctl --user -u {}`", self.name.unit),
            Supervisor::Launchd => self.log.display().to_string(),
        }
    }
}

pub(crate) async fn run(paths: Paths, options: Options, action: Action) -> Result<(), AppError> {
    let Options { port, public, .. } = options;
    let Some(supervisor) = Supervisor::current() else {
        return Err(AppError::usage(
            "`mash dash remote` manages a user service and is available on Linux and macOS; on other systems follow https://getmash.io/dashboard/#set-it-up-by-hand",
        ));
    };
    if public && !FUNNEL_PORTS.contains(&port) {
        return Err(AppError::usage(
            "a public address needs --port 443, 8443 or 10000, the ports Tailscale Funnel can publish",
        ));
    }
    let home = crate::platform::mash_home()
        .ok_or_else(|| AppError::configuration("cannot find the home directory"))?;
    let layout = Layout::resolve(
        supervisor,
        DASHBOARD,
        home.join("dashboard.env"),
        home.join("dashboard.log"),
    )?;
    if matches!(action, Action::Remove) {
        return remove(&layout, port).await;
    }
    let name = tailnet_name().await?;
    let host = format!("{name}:{port}");
    let dashboard = format!("https://{host}/");
    match action {
        Action::Setup => {
            ensure_port_free(&layout, &host, port).await?;
            setup(&paths, &layout, options, &host, dashboard).await
        }
        Action::Code => {
            let token = read_token(&layout.token_file)?;
            report(&Report {
                dashboard,
                service: layout.service(),
                reach: None,
                warning: None,
                code: Some(new_code(port, &token, &layout).await?),
                code_minutes: Some(10),
                shared_index: None,
                next: None,
            })
        }
        Action::Qr => {
            show_qr(&dashboard, &read_token(&layout.token_file)?);
            report(&Report {
                dashboard,
                service: layout.service(),
                reach: None,
                warning: None,
                code: None,
                code_minutes: None,
                shared_index: None,
                next: None,
            })
        }
        Action::Remove => unreachable!("handled above"),
    }
}

/// Write the token and the service, publish the address, and say how to open it.
async fn setup(
    paths: &Paths,
    layout: &Layout,
    options: Options,
    host: &str,
    dashboard: String,
) -> Result<(), AppError> {
    let Options {
        port,
        shared_only,
        public,
    } = options;
    let configuration = crate::config::load(paths.config.as_deref())?;
    let shared = configuration
        .shared
        .as_ref()
        .map(|shared| shared.token_env.clone());
    let token = ensure_environment(&layout.token_file, shared.as_deref())?;
    let exe = std::env::current_exe().map_err(AppError::io)?;
    let arguments = arguments(&exe, paths, (port, host), shared_only)?;
    write_definition(layout, &arguments, &token.environment)?;
    start(layout).await?;
    let forward = format!("http://127.0.0.1:{port}");
    let https = format!("--https={port}");
    let publish = if public { "funnel" } else { "serve" };
    command(tailscale(), &[publish, "--bg", &https, &forward]).await?;
    wait_ready(port, layout).await?;
    let code = new_code(port, &token.dashboard, layout).await?;
    show_qr(&dashboard, &token.dashboard);
    report(&Report {
        dashboard,
        service: layout.service(),
        reach: Some(if public {
            "the public internet"
        } else {
            "devices signed in to your tailnet"
        }),
        warning: public.then_some(
            "anyone on the internet can reach the sign-in page; the token and one-time codes are all that protect your history",
        ),
        code: Some(code),
        code_minutes: Some(10),
        shared_index: shared.map(|_| {
            if token.shared {
                "included"
            } else {
                "token variable not set here; the dashboard shows only this machine"
            }
        }),
        next: Some(if public {
            "On any device, scan the QR code, or open the address and type the code."
        } else {
            "On a device signed in to your tailnet, scan the QR code, or open the address and type the code."
        }),
    })
}

fn report(report: &Report) -> Result<(), AppError> {
    crate::write_json(&mut std::io::stdout(), report)
}

/// Write the service's definition for its manager. launchd reads no
/// environment file, so its property list carries `environment` and is as
/// private as that file.
pub(crate) fn write_definition(
    layout: &Layout,
    arguments: &[String],
    environment: &[(String, String)],
) -> Result<(), AppError> {
    match layout.supervisor {
        Supervisor::Systemd => write_private(&layout.unit_file, &unit(arguments, layout)?, 0o644),
        Supervisor::Launchd => write_private(
            &layout.unit_file,
            &property_list(arguments, layout, environment),
            0o600,
        ),
    }
}

/// Load the written definition and start the service, replacing a running one.
pub(crate) async fn start(layout: &Layout) -> Result<(), AppError> {
    match layout.supervisor {
        Supervisor::Systemd => {
            let unit = layout.name.unit;
            for arguments in [
                vec!["--user", "daemon-reload"],
                vec!["--user", "enable", "--quiet", unit],
                vec!["--user", "restart", unit],
            ] {
                command("systemctl", &arguments).await?;
            }
        }
        Supervisor::Launchd => {
            let (domain, target) = launchd_target(layout.name.label).await?;
            let definition = layout.unit_file.display().to_string();
            unload(&target).await;
            command("launchctl", &["enable", &target]).await?;
            command("launchctl", &["bootstrap", &domain, &definition]).await?;
            // A domain in on-demand mode defers RunAtLoad; this starts it now.
            command("launchctl", &["kickstart", &target]).await?;
        }
    }
    Ok(())
}

/// This user's launchd domain and the service's name within it.
async fn launchd_target(label: &str) -> Result<(String, String), AppError> {
    let uid = command("id", &["-u"]).await?;
    let uid = uid.trim();
    if uid.is_empty() || !uid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AppError::configuration(
            "could not determine this user's ID for launchd",
        ));
    }
    Ok((format!("gui/{uid}"), format!("gui/{uid}/{label}")))
}

/// Unload the launch agent and wait until launchd has let go of it; loading a
/// definition while the old one is still unloading fails.
async fn unload(target: &str) {
    // A service that is not loaded is the wanted outcome.
    let _ = command("launchctl", &["bootout", target]).await;
    for _ in 0..40 {
        if command("launchctl", &["print", target]).await.is_err() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// Stop the service, remove its definition and take down its address. A
/// service that is already gone is the wanted outcome.
pub(crate) async fn uninstall(layout: &Layout, port: u16) -> Result<(), AppError> {
    match layout.supervisor {
        Supervisor::Systemd => {
            let _ = command(
                "systemctl",
                &["--user", "disable", "--now", layout.name.unit],
            )
            .await;
        }
        Supervisor::Launchd => {
            if let Ok((_, target)) = launchd_target(layout.name.label).await {
                unload(&target).await;
            }
        }
    }
    match std::fs::remove_file(&layout.unit_file) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(AppError::io(error)),
    }
    if layout.supervisor == Supervisor::Systemd {
        let _ = command("systemctl", &["--user", "daemon-reload"]).await;
    }
    // Removing the handler takes a public address down with it.
    let https = format!("--https={port}");
    let _ = command(tailscale(), &["serve", &https, "off"]).await;
    Ok(())
}

async fn remove(layout: &Layout, port: u16) -> Result<(), AppError> {
    uninstall(layout, port).await?;
    crate::write_json(
        &mut std::io::stdout(),
        &serde_json::json!({
            "removed": layout.service(),
            "token_file_kept": layout.token_file,
        }),
    )
}

/// Run a helper to completion; its output is kept out of ours.
pub(crate) async fn command(program: &str, arguments: &[&str]) -> Result<String, AppError> {
    // A helper that stops to ask something would otherwise hang this command.
    let running = tokio::process::Command::new(program)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(std::time::Duration::from_secs(30), running)
        .await
        .map_err(|_| {
            AppError::configuration(format!(
                "`{program} {}` did not finish within 30 s",
                arguments.join(" ")
            ))
        })?
        .map_err(|_| {
            AppError::configuration(format!("`{program}` is required and was not found"))
        })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::configuration(format!(
            "`{program} {}` failed: {}",
            arguments.join(" "),
            detail.trim().lines().next().unwrap_or("no output")
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Tailscale's command: the one on PATH, or on macOS the app's own copy.
pub(crate) fn tailscale() -> &'static str {
    let on_path = std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| directory.join("tailscale").is_file())
    });
    if !on_path && cfg!(target_os = "macos") && Path::new(TAILSCALE_APP).is_file() {
        TAILSCALE_APP
    } else {
        "tailscale"
    }
}

/// The name other devices in the tailnet use for this machine.
pub(crate) async fn tailnet_name() -> Result<String, AppError> {
    parse_tailnet_name(&command(tailscale(), &["status", "--json"]).await?)
}

/// This machine's own addresses inside the tailnet.
fn parse_tailnet_addresses(status: &str) -> Vec<std::net::IpAddr> {
    #[derive(serde::Deserialize)]
    struct Status {
        #[serde(rename = "Self")]
        this: Node,
    }
    #[derive(serde::Deserialize)]
    struct Node {
        #[serde(rename = "TailscaleIPs", default)]
        addresses: Vec<std::net::IpAddr>,
    }
    serde_json::from_str::<Status>(status).map_or_default(|status| status.this.addresses)
}

/// Whether Tailscale already forwards `host` (`name:port`) to this port on
/// loopback, as an earlier run of the same command left it.
fn already_published(serve: &str, host: &str, port: u16) -> bool {
    serde_json::from_str::<serde_json::Value>(serve).is_ok_and(|status| {
        status["Web"][host]["Handlers"]["/"]["Proxy"]
            .as_str()
            .is_some_and(|proxy| proxy == format!("http://127.0.0.1:{port}"))
    })
}

/// Refuse a port something else on this machine already answers on. Tailscale
/// takes the port on this machine's tailnet address, so a program listening
/// there, such as another dashboard, would silently stop being reachable.
pub(crate) async fn ensure_port_free(
    layout: &Layout,
    host: &str,
    port: u16,
) -> Result<(), AppError> {
    // A rerun finds its own service and address in place. Without the
    // service, a forward to this port belongs to something else.
    let installed = layout.unit_file.exists();
    let serve = command(tailscale(), &["serve", "status", "--json"])
        .await
        .unwrap_or_default();
    if installed && already_published(&serve, host, port) {
        return Ok(());
    }
    let status = command(tailscale(), &["status", "--json"]).await?;
    let mut addresses = parse_tailnet_addresses(&status);
    // Before the service exists, nothing of ours listens on loopback either.
    if !installed {
        addresses.push(std::net::Ipv4Addr::LOCALHOST.into());
    }
    for address in addresses {
        let connect = tokio::net::TcpStream::connect((address, port));
        if tokio::time::timeout(std::time::Duration::from_secs(1), connect)
            .await
            .is_ok_and(|connected| connected.is_ok())
        {
            return Err(AppError::usage(format!(
                "port {port} is already in use on this machine ({address}); choose another with --port"
            )));
        }
    }
    Ok(())
}

fn parse_tailnet_name(status: &str) -> Result<String, AppError> {
    #[derive(serde::Deserialize)]
    struct Status {
        #[serde(rename = "Self")]
        this: Node,
    }
    #[derive(serde::Deserialize)]
    struct Node {
        #[serde(rename = "DNSName", default)]
        name: String,
    }
    let name = serde_json::from_str::<Status>(status)
        .map_or_default(|status| status.this.name.trim_end_matches('.').to_owned());
    if name.is_empty() {
        return Err(AppError::configuration(
            "Tailscale is not connected; run `tailscale up` first",
        ));
    }
    Ok(name)
}

struct Tokens {
    dashboard: String,
    /// The shared index's node token was available to save for the service.
    shared: bool,
    /// Every variable the service needs, as written to the token file.
    environment: Vec<(String, String)>,
}

/// The service's environment file: the dashboard token, created once and then
/// kept, and the shared index's node token when this process was given it.
fn ensure_environment(path: &Path, shared_variable: Option<&str>) -> Result<Tokens, AppError> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let dashboard = if let Some(token) = value(&existing, TOKEN_VARIABLE) {
        token
    } else {
        let mut random = [0_u8; 24];
        getrandom::fill(&mut random).map_err(|_| AppError::internal("create dashboard token"))?;
        random.iter().fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
    };
    let mut text = format!("{TOKEN_VARIABLE}={dashboard}\n");
    let shared_value = shared_variable.and_then(|name| {
        std::env::var(name)
            .ok()
            .filter(|token| token.bytes().all(|byte| (33..=126).contains(&byte)))
            .or_else(|| value(&existing, name))
            .map(|token| (name, token))
    });
    if let Some((name, token)) = &shared_value {
        let _ = writeln!(text, "{name}={token}");
    }
    write_private(path, &text, 0o600)?;
    let mut environment = vec![(TOKEN_VARIABLE.to_owned(), dashboard.clone())];
    environment.extend(
        shared_value
            .iter()
            .map(|(name, token)| ((*name).to_owned(), token.clone())),
    );
    Ok(Tokens {
        dashboard,
        shared: shared_value.is_some(),
        environment,
    })
}

fn value(environment: &str, name: &str) -> Option<String> {
    environment.lines().find_map(|line| {
        line.strip_prefix(name)?
            .strip_prefix('=')
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn read_token(path: &Path) -> Result<String, AppError> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| value(&text, TOKEN_VARIABLE))
        .ok_or_else(|| AppError::usage("no dashboard token yet; run `mash dash remote` first"))
}

pub(crate) fn write_private(path: &Path, text: &str, mode: u32) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::io)?;
    }
    std::fs::write(path, text).map_err(AppError::io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(AppError::io)?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

/// The dashboard's command line: this executable, the paths this command was
/// given, and the loopback address behind the tailnet name.
fn arguments(
    exe: &Path,
    paths: &Paths,
    (port, host): (u16, &str),
    shared_only: bool,
) -> Result<Vec<String>, AppError> {
    let plain = |path: &Path| {
        path.to_str()
            .filter(|text| {
                !text
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '"' | '\\' | '$' | '%'))
            })
            .map(str::to_owned)
            .ok_or_else(|| {
                AppError::configuration(format!(
                    "cannot write a service for the path {}",
                    path.display()
                ))
            })
    };
    let mut arguments = vec![plain(exe)?];
    for (flag, path) in [
        ("--config", &paths.config),
        ("--db", &paths.db),
        ("--models-dir", &paths.models_dir),
    ] {
        if let Some(path) = path {
            arguments.push(flag.to_owned());
            arguments.push(plain(&std::path::absolute(path).map_err(AppError::io)?)?);
        }
    }
    arguments.extend(
        [
            "dash",
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--allow-host",
            host,
            "--access-token-env",
            TOKEN_VARIABLE,
        ]
        .map(str::to_owned),
    );
    if shared_only {
        arguments.push("--shared-only".to_owned());
    }
    Ok(arguments)
}

/// The systemd user service that keeps the dashboard running on loopback.
fn unit(arguments: &[String], layout: &Layout) -> Result<String, AppError> {
    let token_file = layout.token_file.to_str().ok_or_else(|| {
        AppError::configuration("cannot write a service for a home directory that is not UTF-8")
    })?;
    // Paths are quoted; nothing else in the command line has a space.
    let start = arguments
        .iter()
        .map(|argument| {
            if argument.starts_with('/') {
                format!("\"{argument}\"")
            } else {
                argument.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    Ok(format!(
        "# Written by `{}`; rerun it to repair.\n\
         [Unit]\n\
         Description={}\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         EnvironmentFile={token_file}\n\
         ExecStart={start}\n\
         Restart=on-failure\n\
         RestartSec=10\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        layout.name.written_by, layout.name.description,
    ))
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The launch agent that keeps the dashboard running on loopback.
fn property_list(
    arguments: &[String],
    layout: &Layout,
    environment: &[(String, String)],
) -> String {
    let string = |value: &str| format!("<string>{}</string>", xml(value));
    let program: String = arguments.iter().map(|argument| string(argument)).collect();
    let variables = environment
        .iter()
        .fold(String::new(), |mut text, (name, value)| {
            let _ = write!(text, "<key>{}</key>{}", xml(name), string(value));
            text
        });
    let log = string(&layout.log.display().to_string());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <!-- Written by `{}`; rerun it to repair. -->\n\
         <plist version=\"1.0\"><dict>\n\
         <key>Label</key>{}\n\
         <key>ProgramArguments</key><array>{program}</array>\n\
         <key>EnvironmentVariables</key><dict>{variables}</dict>\n\
         <key>RunAtLoad</key><true/>\n\
         <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n\
         <key>ThrottleInterval</key><integer>10</integer>\n\
         <key>StandardOutPath</key>{log}\n\
         <key>StandardErrorPath</key>{log}\n\
         </dict></plist>\n",
        layout.name.written_by,
        string(layout.name.label),
    )
}

fn client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|_| AppError::internal("build dashboard client"))
}

async fn wait_ready(port: u16, layout: &Layout) -> Result<(), AppError> {
    let client = client()?;
    let url = format!("http://127.0.0.1:{port}/api/health");
    for _ in 0..40 {
        if client
            .get(&url)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    Err(AppError::configuration(format!(
        "the dashboard did not start; see {}",
        layout.logs()
    )))
}

/// Ask the running dashboard for a one-time code, shown as `ABC-DEF`.
async fn new_code(port: u16, token: &str, layout: &Layout) -> Result<String, AppError> {
    #[derive(serde::Deserialize)]
    struct Minted {
        code: String,
    }
    let unavailable = || {
        AppError::configuration(format!(
            "the dashboard is not answering; see {}",
            layout.logs()
        ))
    };
    let minted: Minted = client()?
        .post(format!("http://127.0.0.1:{port}/api/session/code"))
        .bearer_auth(token)
        .send()
        .await
        .ok()
        .filter(|response| response.status().is_success())
        .ok_or_else(unavailable)?
        .json()
        .await
        .map_err(|_| unavailable())?;
    Ok(spaced(&minted.code))
}

fn spaced(code: &str) -> String {
    match code.split_at_checked(code.len() / 2) {
        Some((left, right)) if code.len() >= 4 => format!("{left}-{right}"),
        _ => code.to_owned(),
    }
}

/// The address with the token after `#`, which browsers never send to a
/// server; the dashboard signs in from it and removes it at once.
fn sign_in_link(dashboard: &str, token: &str) -> String {
    format!("{dashboard}#signin={token}")
}

/// Draw the sign-in link for a phone camera, when a person is watching.
fn show_qr(dashboard: &str, token: &str) {
    use std::io::IsTerminal;
    if !std::io::stderr().is_terminal() {
        return;
    }
    if let Ok(code) = qrcode::QrCode::new(sign_in_link(dashboard, token)) {
        let drawing = code
            .render::<qrcode::render::unicode::Dense1x2>()
            .quiet_zone(true)
            .build();
        eprintln!("{drawing}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailnet_name_comes_from_this_machine_without_the_trailing_dot() {
        let status = r#"{"Self":{"DNSName":"laptop.tail1234.ts.net.","Online":true},"Peer":{}}"#;
        assert_eq!(
            parse_tailnet_name(status).unwrap(),
            "laptop.tail1234.ts.net"
        );
        for stopped in [r#"{"Self":{"DNSName":""}}"#, r#"{"Self":{}}"#, "not json"] {
            assert!(parse_tailnet_name(stopped).is_err(), "{stopped}");
        }
    }

    #[test]
    fn the_environment_file_keeps_its_token_and_is_private() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/dashboard.env");
        let first = ensure_environment(&path, None).unwrap();
        assert_eq!(first.dashboard.len(), 48);
        assert!(!first.shared);
        let again = ensure_environment(&path, Some("MASH_TEST_ABSENT_NODE_TOKEN")).unwrap();
        assert_eq!(again.dashboard, first.dashboard, "a rerun keeps the token");
        assert!(!again.shared);
        assert_eq!(read_token(&path).unwrap(), first.dashboard);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // A node token saved by an earlier run survives a run without it.
        std::fs::write(
            &path,
            format!("{TOKEN_VARIABLE}={}\nNODE=abc\n", first.dashboard),
        )
        .unwrap();
        assert!(ensure_environment(&path, Some("NODE")).unwrap().shared);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("NODE=abc\n")
        );
        assert!(read_token(&directory.path().join("missing")).is_err());
    }

    // Service definitions are written from Unix paths.
    #[cfg(unix)]
    #[test]
    fn the_service_serves_loopback_under_the_tailnet_name_with_the_token() {
        let layout = |supervisor| Layout {
            supervisor,
            name: DASHBOARD,
            token_file: "/home/me/.mash/dashboard.env".into(),
            unit_file: "/unused".into(),
            log: "/home/me/.mash/dashboard.log".into(),
        };
        let paths = Paths {
            config: Some("/home/me/.mash/config.toml".into()),
            ..Paths::default()
        };
        let host = (8790, "laptop.tail1234.ts.net:8790");
        let exe = Path::new("/usr/local/bin/mash");
        let command = arguments(exe, &paths, host, false).unwrap();
        let text = unit(&command, &layout(Supervisor::Systemd)).unwrap();
        assert!(text.contains("EnvironmentFile=/home/me/.mash/dashboard.env\n"));
        assert!(text.contains(
            "ExecStart=\"/usr/local/bin/mash\" --config \"/home/me/.mash/config.toml\" dash --listen 127.0.0.1:8790 --allow-host laptop.tail1234.ts.net:8790 --access-token-env MASH_DASH_TOKEN\n"
        ));
        // A path a service manager would reinterpret is refused, not written.
        assert!(arguments(Path::new("/opt/ma$h"), &paths, host, false).is_err());
        // A machine without history of its own serves the shared index alone.
        let server = arguments(exe, &paths, host, true).unwrap();
        assert!(
            unit(&server, &layout(Supervisor::Systemd))
                .unwrap()
                .contains("--access-token-env MASH_DASH_TOKEN --shared-only\n")
        );

        // launchd takes the same command as separate arguments and, having no
        // environment file, the tokens themselves.
        let environment = [
            ("MASH_DASH_TOKEN".to_owned(), "abc".to_owned()),
            ("NODE".to_owned(), "a<b&\"c".to_owned()),
        ];
        let agent = property_list(&command, &layout(Supervisor::Launchd), &environment);
        assert!(agent.contains("<key>Label</key><string>dev.mash.dash</string>"));
        assert!(agent.contains(
            "<array><string>/usr/local/bin/mash</string><string>--config</string><string>/home/me/.mash/config.toml</string><string>dash</string><string>--listen</string><string>127.0.0.1:8790</string><string>--allow-host</string><string>laptop.tail1234.ts.net:8790</string><string>--access-token-env</string><string>MASH_DASH_TOKEN</string></array>"
        ));
        assert!(agent.contains(
            "<dict><key>MASH_DASH_TOKEN</key><string>abc</string><key>NODE</key><string>a&lt;b&amp;&quot;c</string></dict>"
        ));
        assert!(
            agent.contains(
                "<key>StandardErrorPath</key><string>/home/me/.mash/dashboard.log</string>"
            )
        );
        assert_eq!(layout(Supervisor::Launchd).service(), "dev.mash.dash");
        assert_eq!(layout(Supervisor::Systemd).service(), "mash-dash.service");
        assert!(text.contains("Description=Mash dashboard for this tailnet\n"));
        assert!(agent.contains("Written by `mash dash remote`"));
        assert!(
            layout(Supervisor::Launchd)
                .logs()
                .ends_with("dashboard.log")
        );
        assert!(layout(Supervisor::Systemd).logs().contains("journalctl"));
    }

    #[tokio::test]
    async fn a_public_address_is_refused_on_a_port_funnel_cannot_publish() {
        let options = Options {
            port: 8790,
            shared_only: false,
            public: true,
        };
        if Supervisor::current().is_some() {
            let error = run(Paths::default(), options, Action::Setup)
                .await
                .unwrap_err();
            assert_eq!(error.error.kind, crate::ErrorKind::Usage);
            assert!(error.error.message.contains("8443"));
        }
    }

    #[test]
    fn a_port_is_ours_only_when_tailscale_already_forwards_it_here() {
        let serve = r#"{"TCP":{"8790":{"HTTPS":true}},"Web":{"laptop.ts.net:8790":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:8790"}}}}}"#;
        assert!(already_published(serve, "laptop.ts.net:8790", 8790));
        assert!(!already_published(serve, "laptop.ts.net:8791", 8791));
        assert!(!already_published(serve, "laptop.ts.net:8790", 9000));
        assert!(!already_published("{}", "laptop.ts.net:8790", 8790));
        assert!(!already_published("not json", "laptop.ts.net:8790", 8790));
        let status = r#"{"Self":{"DNSName":"laptop.ts.net.","TailscaleIPs":["100.90.2.48","fd7a:115c:a1e0::1"]}}"#;
        assert_eq!(parse_tailnet_addresses(status).len(), 2);
        assert_eq!(parse_tailnet_addresses("{}"), [] as [std::net::IpAddr; 0]);
    }

    #[test]
    fn codes_and_links_are_shaped_for_people_and_cameras() {
        assert_eq!(spaced("K7M2QX"), "K7M-2QX");
        assert_eq!(spaced("AB"), "AB");
        assert_eq!(
            sign_in_link("https://laptop.ts.net:8790/", "token"),
            "https://laptop.ts.net:8790/#signin=token"
        );
    }
}
