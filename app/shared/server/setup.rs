//! `mash-server --setup`: turn this machine into the home of a shared index
//! with one command. It writes a first configuration under Mash's own
//! directory, adds the first node, keeps the server running as a user service
//! and gives it an HTTPS address inside the user's tailnet. It uses the Qdrant
//! that is already running here and downloads nothing.
use super::configuration;
use crate::AppError;
use crate::remote::{self, Layout, ServiceName, Supervisor};
use std::path::{Path, PathBuf};

const SERVICE: ServiceName = ServiceName {
    unit: "mash-server.service",
    label: "dev.mash.server",
    description: "Mash shared index for this tailnet",
    written_by: "mash-server --setup",
};
const QDRANT_URL: &str = "MASH_QDRANT_URL";
const QDRANT_GRPC_URL: &str = "MASH_QDRANT_GRPC_URL";

/// The files of a server that `--setup` manages.
struct Home {
    configuration: PathBuf,
    environment: PathBuf,
    log: PathBuf,
    silver: PathBuf,
}

impl Home {
    fn resolve() -> Result<Self, AppError> {
        let home = crate::platform::mash_home()
            .ok_or_else(|| AppError::configuration("cannot find the home directory"))?
            .join("server");
        Ok(Self {
            configuration: home.join("server.toml"),
            environment: home.join("server.env"),
            log: home.join("server.log"),
            silver: home.join("silver"),
        })
    }
}

/// A first configuration: word search only, so the server needs no models.
fn first_configuration(port: u16, silver: &Path) -> Result<String, AppError> {
    let silver = silver
        .to_str()
        .filter(|text| {
            !text
                .chars()
                .any(|c| c.is_control() || matches!(c, '"' | '\\'))
        })
        .ok_or_else(|| {
            AppError::configuration("cannot write a configuration for this home directory")
        })?;
    Ok(format!(
        "# Written by `mash-server --setup`. Nodes added with --add-node are kept in\n\
         # server.toml.nodes beside this file.\n\
         version = 1\n\
         nodes = []\n\
         listen = \"127.0.0.1:{port}\"\n\
         qdrant_url_env = \"{QDRANT_URL}\"\n\
         qdrant_grpc_url_env = \"{QDRANT_GRPC_URL}\"\n\
         silver_dir = \"{silver}\"\n\
         \n\
         [views.build]\n\
         representations = \"lexical-only\"\n"
    ))
}

/// The service's environment: where Qdrant is. An existing file is kept, so
/// an operator can point the server at another Qdrant and rerun.
fn environment(path: &Path) -> Result<Vec<(String, String)>, AppError> {
    let defaults = [
        (QDRANT_URL, "http://127.0.0.1:6333"),
        (QDRANT_GRPC_URL, "http://127.0.0.1:6334"),
    ];
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let value = |name: &str| {
        existing.lines().find_map(|line| {
            line.strip_prefix(name)?
                .strip_prefix('=')
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
    };
    let variables: Vec<(String, String)> = defaults
        .iter()
        .map(|(name, default)| {
            (
                (*name).to_owned(),
                value(name).unwrap_or_else(|| (*default).to_owned()),
            )
        })
        .collect();
    let text: String = variables
        .iter()
        .fold(String::new(), |mut text, (name, value)| {
            text.push_str(name);
            text.push('=');
            text.push_str(value);
            text.push('\n');
            text
        });
    remote::write_private(path, &text, 0o600)?;
    Ok(variables)
}

async fn status(url: &str) -> Option<reqwest::StatusCode> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    Some(client.get(url).send().await.ok()?.status())
}

/// Stop the server and take its address down. The configuration, the added
/// nodes and the history under `~/.mash/server` are kept, so `--setup` brings
/// the same server back.
pub async fn remove(port: u16) -> Result<(), AppError> {
    let Some(supervisor) = Supervisor::current() else {
        return Err(AppError::usage(
            "`mash-server --setup` manages a user service and is available on Linux and macOS",
        ));
    };
    let home = Home::resolve()?;
    let layout = Layout::resolve(
        supervisor,
        SERVICE,
        home.environment.clone(),
        home.log.clone(),
    )?;
    // The address to take down is the one the server was set up on.
    let port = if home.configuration.exists() {
        configuration::load(&home.configuration)
            .await?
            .listen
            .port()
    } else {
        port
    };
    remote::uninstall(&layout, port).await?;
    crate::write_json(
        &mut std::io::stdout(),
        &serde_json::json!({
            "removed": layout.service(),
            "kept": home.configuration.parent(),
        }),
    )
}

/// Set up or repair the server, and add `node` (`GROUP/NODE`) if it is new.
pub async fn run(node: &str, port: u16) -> Result<(), AppError> {
    let Some(supervisor) = Supervisor::current() else {
        return Err(AppError::usage(
            "`mash-server --setup` manages a user service and is available on Linux and macOS; on other systems follow https://getmash.io/shared-index/",
        ));
    };
    let home = Home::resolve()?;
    let layout = Layout::resolve(
        supervisor,
        SERVICE,
        home.environment.clone(),
        home.log.clone(),
    )?;
    let variables = environment(&home.environment)?;
    let qdrant = &variables[0].1;
    if status(&format!("{qdrant}/readyz")).await.is_none() {
        return Err(AppError::configuration(format!(
            "Qdrant is not answering at {qdrant}; start it first, for example with `mash qdrant install`"
        )));
    }
    let name = remote::tailnet_name().await?;
    let api_url = format!("https://{name}:{port}");
    // A server set up earlier keeps its port: the address is in every join
    // string already handed out.
    if home.configuration.exists() {
        let listening = configuration::load(&home.configuration)
            .await?
            .listen
            .port();
        if listening != port {
            return Err(AppError::usage(format!(
                "this server is already set up on port {listening}; rerun with --port {listening}"
            )));
        }
    }
    remote::ensure_port_free(&layout, &format!("{name}:{port}"), port).await?;
    if !home.configuration.exists() {
        remote::write_private(
            &home.configuration,
            &first_configuration(port, &home.silver)?,
            0o600,
        )?;
    }
    // A rerun repairs the service; it never mints a second token for a node.
    let existing = configuration::load(&home.configuration).await?;
    let (group_id, node_id) = super::node_name(node)?;
    let join = if configuration::has_node(&existing, group_id, node_id) {
        None
    } else {
        Some(super::mint(&home.configuration, node, &api_url).await?)
    };
    let exe = std::env::current_exe().map_err(AppError::io)?;
    let arguments = [
        exe.as_path(),
        Path::new("--server-config"),
        home.configuration.as_path(),
    ]
    .iter()
    .map(|part| {
        part.to_str().map(str::to_owned).ok_or_else(|| {
            AppError::configuration("cannot write a service for a path that is not UTF-8")
        })
    })
    .collect::<Result<Vec<_>, _>>()?;
    remote::write_definition(&layout, &arguments, &variables)?;
    remote::start(&layout).await?;
    let forward = format!("http://127.0.0.1:{port}");
    let https = format!("--https={port}");
    remote::command(remote::tailscale(), &["serve", "--bg", &https, &forward]).await?;
    // The server refuses an unauthenticated request once it is listening;
    // any other answer comes from something else on the port.
    let mut listening = false;
    for _ in 0..80 {
        if status(&format!("{forward}/v1/freshness")).await
            == Some(reqwest::StatusCode::UNAUTHORIZED)
        {
            listening = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    if !listening {
        return Err(AppError::configuration(format!(
            "the server did not start; see {}",
            layout.logs()
        )));
    }
    crate::write_json(
        &mut std::io::stdout(),
        &serde_json::json!({
            "server": api_url,
            "service": layout.service(),
            "configuration": home.configuration,
            "node": node,
            "join": join,
            "next": if join.is_some() {
                "On that machine, run `mash init --join` with the join value; it contains the node's token, so pass it privately. Add more machines with `mash-server --setup --add-node GROUP/NODE`."
            } else {
                "That node was already added, so no new join string was made. The service was repaired and restarted."
            },
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_configuration_is_one_the_server_accepts() {
        let text = first_configuration(8787, Path::new("/home/me/.mash/server/silver")).unwrap();
        let config: configuration::Configuration = toml::from_str(&text).unwrap();
        assert_eq!(config.listen.to_string(), "127.0.0.1:8787");
        assert_eq!(config.qdrant_url_env.as_deref(), Some(QDRANT_URL));
        assert_eq!(config.qdrant_grpc_url_env.as_deref(), Some(QDRANT_GRPC_URL));
        assert!(first_configuration(8787, Path::new("/home/a\"b")).is_err());
    }

    #[test]
    fn an_existing_environment_keeps_its_qdrant_addresses() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server/server.env");
        let first = environment(&path).unwrap();
        assert_eq!(
            first[0],
            (QDRANT_URL.to_owned(), "http://127.0.0.1:6333".to_owned())
        );
        std::fs::write(&path, format!("{QDRANT_URL}=http://127.0.0.1:7000\n")).unwrap();
        let kept = environment(&path).unwrap();
        assert_eq!(kept[0].1, "http://127.0.0.1:7000");
        assert_eq!(
            kept[1].1, "http://127.0.0.1:6334",
            "a missing one is filled in"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
