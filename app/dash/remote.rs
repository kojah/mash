//! `mash dash remote`: keep this machine's dashboard running behind a sign-in
//! and give it an HTTPS address inside the user's tailnet. It writes a token
//! file and a user service, asks Tailscale to forward the address to
//! loopback, and hands back what a phone needs: the address, a one-time code
//! and a QR code. Nothing here is reachable from the public internet.
use crate::AppError;
use crate::core::Paths;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const UNIT: &str = "mash-dash.service";
const TOKEN_VARIABLE: &str = "MASH_DASH_TOKEN";

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

struct Layout {
    token_file: PathBuf,
    unit_file: PathBuf,
}

impl Layout {
    fn resolve() -> Result<Self, AppError> {
        let home = crate::platform::mash_home()
            .ok_or_else(|| AppError::configuration("cannot find the home directory"))?;
        let base = directories::BaseDirs::new()
            .ok_or_else(|| AppError::configuration("cannot find the home directory"))?;
        Ok(Self {
            token_file: home.join("dashboard.env"),
            unit_file: base.config_dir().join("systemd/user").join(UNIT),
        })
    }
}

pub(crate) async fn run(paths: Paths, options: Options, action: Action) -> Result<(), AppError> {
    let Options { port, public, .. } = options;
    if !cfg!(target_os = "linux") {
        return Err(AppError::usage(
            "`mash dash remote` manages a systemd user service and is available on Linux; on other systems follow https://getmash.io/dashboard/#open-it-from-your-phone-or-another-computer",
        ));
    }
    if public && !FUNNEL_PORTS.contains(&port) {
        return Err(AppError::usage(
            "a public address needs --port 443, 8443 or 10000, the ports Tailscale Funnel can publish",
        ));
    }
    let layout = Layout::resolve()?;
    if matches!(action, Action::Remove) {
        return remove(&layout, port).await;
    }
    let name = tailnet_name().await?;
    let host = format!("{name}:{port}");
    let dashboard = format!("https://{host}/");
    match action {
        Action::Setup => setup(&paths, &layout, options, &host, dashboard).await,
        Action::Code => {
            let token = read_token(&layout.token_file)?;
            report(&Report {
                dashboard,
                service: UNIT,
                reach: None,
                warning: None,
                code: Some(new_code(port, &token).await?),
                code_minutes: Some(10),
                shared_index: None,
                next: None,
            })
        }
        Action::Qr => {
            show_qr(&dashboard, &read_token(&layout.token_file)?);
            report(&Report {
                dashboard,
                service: UNIT,
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
    write_private(
        &layout.unit_file,
        &unit(&exe, paths, layout, (port, host), shared_only)?,
        0o644,
    )?;
    for arguments in [
        vec!["--user", "daemon-reload"],
        vec!["--user", "enable", "--quiet", UNIT],
        vec!["--user", "restart", UNIT],
    ] {
        command("systemctl", &arguments).await?;
    }
    let forward = format!("http://127.0.0.1:{port}");
    let https = format!("--https={port}");
    let publish = if public { "funnel" } else { "serve" };
    command("tailscale", &[publish, "--bg", &https, &forward]).await?;
    wait_ready(port).await?;
    let code = new_code(port, &token.dashboard).await?;
    show_qr(&dashboard, &token.dashboard);
    report(&Report {
        dashboard,
        service: UNIT,
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

async fn remove(layout: &Layout, port: u16) -> Result<(), AppError> {
    // A service that is already gone is the wanted outcome.
    let _ = command("systemctl", &["--user", "disable", "--now", UNIT]).await;
    match std::fs::remove_file(&layout.unit_file) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(AppError::io(error)),
    }
    let _ = command("systemctl", &["--user", "daemon-reload"]).await;
    let https = format!("--https={port}");
    for publish in ["serve", "funnel"] {
        let _ = command("tailscale", &[publish, &https, "off"]).await;
    }
    crate::write_json(
        &mut std::io::stdout(),
        &serde_json::json!({
            "removed": UNIT,
            "token_file_kept": layout.token_file,
        }),
    )
}

/// Run a helper to completion; its output is kept out of ours.
async fn command(program: &str, arguments: &[&str]) -> Result<String, AppError> {
    let output = tokio::process::Command::new(program)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .output()
        .await
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

/// The name other devices in the tailnet use for this machine.
async fn tailnet_name() -> Result<String, AppError> {
    parse_tailnet_name(&command("tailscale", &["status", "--json"]).await?)
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
    Ok(Tokens {
        dashboard,
        shared: shared_value.is_some(),
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

fn write_private(path: &Path, text: &str, mode: u32) -> Result<(), AppError> {
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

/// The user service that keeps the dashboard running on loopback.
fn unit(
    exe: &Path,
    paths: &Paths,
    layout: &Layout,
    (port, host): (u16, &str),
    shared_only: bool,
) -> Result<String, AppError> {
    let quote = |path: &Path| {
        let text = path.to_str().filter(|text| {
            !text
                .chars()
                .any(|c| c.is_control() || matches!(c, '"' | '\\' | '$' | '%'))
        });
        text.map(|text| format!("\"{text}\"")).ok_or_else(|| {
            AppError::configuration(format!(
                "cannot write a service for the path {}",
                path.display()
            ))
        })
    };
    let mut start = quote(exe)?;
    for (flag, path) in [
        ("--config", &paths.config),
        ("--db", &paths.db),
        ("--models-dir", &paths.models_dir),
    ] {
        if let Some(path) = path {
            let absolute = std::path::absolute(path).map_err(AppError::io)?;
            let _ = write!(start, " {flag} {}", quote(&absolute)?);
        }
    }
    let shared = if shared_only { " --shared-only" } else { "" };
    Ok(format!(
        "[Unit]\n\
         Description=Mash dashboard for this tailnet\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         EnvironmentFile={}\n\
         ExecStart={start} dash --listen 127.0.0.1:{port} --allow-host {host} --access-token-env {TOKEN_VARIABLE}{shared}\n\
         Restart=on-failure\n\
         RestartSec=10\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        layout.token_file.display()
    ))
}

fn client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|_| AppError::internal("build dashboard client"))
}

async fn wait_ready(port: u16) -> Result<(), AppError> {
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
        "the dashboard did not start; see `journalctl --user -u {UNIT}`"
    )))
}

/// Ask the running dashboard for a one-time code, shown as `ABC-DEF`.
async fn new_code(port: u16, token: &str) -> Result<String, AppError> {
    #[derive(serde::Deserialize)]
    struct Minted {
        code: String,
    }
    let unavailable = || {
        AppError::configuration(format!(
            "the dashboard is not answering; see `journalctl --user -u {UNIT}`"
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

    // A systemd unit is written from Unix paths.
    #[cfg(unix)]
    #[test]
    fn the_service_serves_loopback_under_the_tailnet_name_with_the_token() {
        let layout = Layout {
            token_file: "/home/me/.mash/dashboard.env".into(),
            unit_file: "/unused".into(),
        };
        let paths = Paths {
            config: Some("/home/me/.mash/config.toml".into()),
            ..Paths::default()
        };
        let host = (8790, "laptop.tail1234.ts.net:8790");
        let text = unit(
            Path::new("/usr/local/bin/mash"),
            &paths,
            &layout,
            host,
            false,
        )
        .unwrap();
        assert!(text.contains("EnvironmentFile=/home/me/.mash/dashboard.env\n"));
        assert!(text.contains(
            "ExecStart=\"/usr/local/bin/mash\" --config \"/home/me/.mash/config.toml\" dash --listen 127.0.0.1:8790 --allow-host laptop.tail1234.ts.net:8790 --access-token-env MASH_DASH_TOKEN\n"
        ));
        // A path systemd would reinterpret is refused, not written.
        assert!(unit(Path::new("/opt/ma$h"), &paths, &layout, host, false).is_err());
        // A machine without history of its own serves the shared index alone.
        let server = unit(
            Path::new("/usr/local/bin/mash"),
            &paths,
            &layout,
            host,
            true,
        )
        .unwrap();
        assert!(server.contains("--access-token-env MASH_DASH_TOKEN --shared-only\n"));
    }

    #[tokio::test]
    async fn a_public_address_is_refused_on_a_port_funnel_cannot_publish() {
        let options = Options {
            port: 8790,
            shared_only: false,
            public: true,
        };
        if cfg!(target_os = "linux") {
            let error = run(Paths::default(), options, Action::Setup)
                .await
                .unwrap_err();
            assert_eq!(error.error.kind, crate::ErrorKind::Usage);
            assert!(error.error.message.contains("8443"));
        }
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
