use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};

use crate::AppError;

const MAX_CONFIG_BYTES: usize = 1024 * 1024;
const DEFAULT_SINCE_DAYS: u32 = 90;
const MAX_SINCE_DAYS: u32 = 36_500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedConfig {
    pub(crate) path: PathBuf,
    pub(crate) loaded: bool,
    pub(crate) shared: Option<SharedConfig>,
    pub(crate) embeddings: Option<crate::semantic::remote::Configuration>,
    pub(crate) providers: ResolvedProviders,
    pub(crate) since_days: Option<u32>,
    pub(crate) destination: IndexDestination,
    pub(crate) local_filter: Option<IndexFilter>,
    pub(crate) shared_filter: Option<IndexFilter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IndexFilter {
    pub(crate) providers: Vec<String>,
}

impl IndexFilter {
    pub(crate) fn apply(filter: Option<&Self>, options: &mut crate::ingestion::IndexOptions) {
        if let Some(filter) = filter {
            if !filter.providers.iter().any(|p| p == "claude-code") {
                options.claude_code = None;
            }
            if !filter.providers.iter().any(|p| p == "codex") {
                options.codex = None;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum IndexDestination {
    Local,
    Shared,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SharedConfig {
    pub(crate) group_id: String,
    pub(crate) node_id: String,
    pub(crate) api_url: String,
    pub(crate) token_env: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ResolvedProviders {
    pub(crate) claude_code: Option<Vec<PathBuf>>,
    pub(crate) codex: Option<Vec<PathBuf>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ConfigurationStatus {
    path: PathBuf,
    loaded: bool,
    shared: Option<SharedConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    embeddings: Option<Box<crate::semantic::remote::Configuration>>,
    providers: ProviderStatusMap,
    index: IndexStatus,
}

#[derive(Debug, Serialize)]
struct ProviderStatusMap {
    #[serde(rename = "claude-code", skip_serializing_if = "Option::is_none")]
    claude_code: Option<ProviderStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    codex: Option<ProviderStatus>,
}

#[derive(Debug, Serialize)]
struct ProviderStatus {
    roots: Vec<PathBuf>,
}

#[derive(Debug, Serialize)]
struct IndexStatus {
    since_days: Option<u32>,
    destination: IndexDestination,
    #[serde(skip_serializing_if = "Option::is_none")]
    local: Option<IndexFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared: Option<IndexFilter>,
}

impl ResolvedConfig {
    pub(crate) fn status(&self) -> ConfigurationStatus {
        ConfigurationStatus {
            path: self.path.clone(),
            loaded: self.loaded,
            shared: self.shared.clone(),
            embeddings: self.embeddings.clone().map(Box::new),
            providers: ProviderStatusMap {
                claude_code: self
                    .providers
                    .claude_code
                    .clone()
                    .map(|roots| ProviderStatus { roots }),
                codex: self
                    .providers
                    .codex
                    .clone()
                    .map(|roots| ProviderStatus { roots }),
            },
            index: IndexStatus {
                since_days: self.since_days,
                destination: self.destination,
                local: self.local_filter.clone(),
                shared: self.shared_filter.clone(),
            },
        }
    }
}

#[derive(Debug)]
struct ParsedDocument {
    providers: ResolvedProviders,
    since_days: Option<u32>,
    shared: Option<SharedConfig>,
    embeddings: Option<crate::semantic::remote::Configuration>,
    destination: IndexDestination,
    local_filter: Option<IndexFilter>,
    shared_filter: Option<IndexFilter>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    version: u32,
    providers: RawProviders,
    #[serde(default)]
    index: RawIndex,
    shared: Option<SharedConfig>,
    embeddings: Option<crate::semantic::remote::Configuration>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProviders {
    #[serde(rename = "claude-code")]
    claude_code: Option<RawProvider>,
    codex: Option<RawProvider>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    roots: Vec<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIndex {
    since_days: Option<u32>,
    #[serde(default)]
    all_history: bool,
    destination: Option<IndexDestination>,
    local: Option<IndexFilter>,
    shared: Option<IndexFilter>,
}

pub(crate) fn load(explicit_path: Option<&Path>) -> Result<ResolvedConfig, AppError> {
    load_from_paths(default_config_path(), explicit_path.map(Path::to_path_buf))
}

fn default_config_path() -> Result<PathBuf, AppError> {
    let path = ProjectDirs::from("dev", "jamesqo", "mash")
        .ok_or_else(|| AppError::configuration("platform configuration directory is unavailable"))?
        .config_dir()
        .join("config.toml");
    if !path.is_absolute() {
        return Err(AppError::configuration(
            "platform configuration path is not absolute",
        ));
    }
    Ok(path)
}

fn load_from_paths(
    default_path: Result<PathBuf, AppError>,
    explicit_path: Option<PathBuf>,
) -> Result<ResolvedConfig, AppError> {
    let explicit = explicit_path.is_some();
    let path = match explicit_path {
        Some(path) => path,
        None => default_path?,
    };

    if !explicit && !path.is_absolute() {
        return Err(AppError::configuration(
            "platform configuration path is not absolute",
        ));
    }

    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound && !explicit => {
            let providers = built_in_providers()?;
            return Ok(ResolvedConfig {
                path,
                loaded: false,
                shared: None,
                embeddings: None,
                providers,
                since_days: Some(DEFAULT_SINCE_DAYS),
                destination: IndexDestination::Local,
                local_filter: None,
                shared_filter: None,
            });
        }
        Err(error) => return Err(path_error("inspect", &path, &error)),
    }

    let canonical =
        fs::canonicalize(&path).map_err(|error| path_error("resolve", &path, &error))?;
    let target_metadata =
        fs::metadata(&canonical).map_err(|error| path_error("inspect", &canonical, &error))?;
    if !target_metadata.is_file() {
        return Err(not_regular_file(&canonical));
    }
    let mut file =
        File::open(&canonical).map_err(|error| path_error("open", &canonical, &error))?;
    let metadata = file
        .metadata()
        .map_err(|error| path_error("inspect", &canonical, &error))?;
    if !metadata.is_file() {
        return Err(not_regular_file(&canonical));
    }

    let metadata_size = usize::try_from(metadata.len()).unwrap_or(MAX_CONFIG_BYTES);
    let mut bytes = Vec::with_capacity(MAX_CONFIG_BYTES.min(metadata_size));
    (&mut file)
        .take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| path_error("read", &canonical, &error))?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(AppError::configuration(format!(
            "configuration exceeds {MAX_CONFIG_BYTES} bytes: {}",
            canonical.display()
        )));
    }

    let parsed = parse_document(&bytes)?;
    Ok(ResolvedConfig {
        path: canonical,
        loaded: true,
        providers: parsed.providers,
        since_days: parsed.since_days,
        shared: parsed.shared,
        embeddings: parsed.embeddings,
        destination: parsed.destination,
        local_filter: parsed.local_filter,
        shared_filter: parsed.shared_filter,
    })
}

fn built_in_providers() -> Result<ResolvedProviders, AppError> {
    let directories = BaseDirs::new()
        .ok_or_else(|| AppError::configuration("platform home directory is unavailable"))?;
    let home = directories.home_dir();
    if !home.is_absolute() {
        return Err(AppError::configuration(
            "platform home directory is not absolute",
        ));
    }
    Ok(ResolvedProviders {
        claude_code: Some(vec![
            home.join(".claude/projects"),
            home.join(".config/claude/projects"),
        ]),
        codex: Some(vec![
            home.join(".codex/sessions"),
            home.join(".local/share/codex/sessions"),
        ]),
    })
}

fn not_regular_file(path: &Path) -> AppError {
    AppError::configuration(format!(
        "configuration path is not a regular file: {}",
        path.display()
    ))
}

fn path_error(operation: &str, path: &Path, error: &io::Error) -> AppError {
    AppError::configuration(format!(
        "failed to {operation} configuration {}: {error}",
        path.display()
    ))
}

fn parse_document(bytes: &[u8]) -> Result<ParsedDocument, AppError> {
    let contents = std::str::from_utf8(bytes)
        .map_err(|_| AppError::configuration("configuration must be UTF-8 TOML"))?;
    let raw: RawDocument = toml::from_str(contents).map_err(|error: toml::de::Error| {
        AppError::configuration(format!("invalid configuration TOML: {}", error.message()))
    })?;
    if raw.version != 1 {
        return Err(AppError::configuration(format!(
            "unsupported configuration version: {}",
            raw.version
        )));
    }
    if let Some(embeddings) = &raw.embeddings {
        embeddings.validate()?;
    }
    if raw.index.all_history && raw.index.since_days.is_some() {
        return Err(AppError::configuration(
            "index all_history and since_days cannot both be specified",
        ));
    }
    let since_days = if raw.index.all_history {
        None
    } else {
        Some(raw.index.since_days.unwrap_or(DEFAULT_SINCE_DAYS))
    };
    if since_days.is_some_and(|days| !(1..=MAX_SINCE_DAYS).contains(&days)) {
        return Err(AppError::configuration(
            "index since_days must be between 1 and 36500",
        ));
    }
    if let Some(shared) = &raw.shared {
        validate_identifier(&shared.group_id, "group ID")?;
        validate_identifier(&shared.node_id, "node ID")?;
        let name = &shared.token_env;
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || name.as_bytes()[0].is_ascii_digit()
        {
            return Err(AppError::configuration("invalid token_env"));
        }
        let url = reqwest::Url::parse(&shared.api_url)
            .map_err(|_| AppError::configuration("invalid shared api_url"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(AppError::configuration(
                "shared api_url must be HTTP(S), without credentials, query, or fragment",
            ));
        }
    }
    let destination = raw.index.destination.unwrap_or(if raw.shared.is_some() {
        IndexDestination::Both
    } else {
        IndexDestination::Local
    });
    if destination != IndexDestination::Local && raw.shared.is_none() {
        return Err(AppError::configuration(
            "shared and both indexing destinations require shared configuration",
        ));
    }
    for filter in [&raw.index.local, &raw.index.shared].into_iter().flatten() {
        let mut unique = HashSet::new();
        for provider in &filter.providers {
            if !matches!(provider.as_str(), "claude-code" | "codex") || !unique.insert(provider) {
                return Err(AppError::configuration(
                    "index provider filters must contain unique supported provider names",
                ));
            }
            if (provider == "claude-code" && raw.providers.claude_code.is_none())
                || (provider == "codex" && raw.providers.codex.is_none())
            {
                return Err(AppError::configuration(
                    "index provider filter selects a disabled provider",
                ));
            }
        }
    }
    Ok(ParsedDocument {
        providers: ResolvedProviders {
            claude_code: validate_roots(raw.providers.claude_code, "claude-code")?,
            codex: validate_roots(raw.providers.codex, "codex")?,
        },
        since_days,
        shared: raw.shared,
        embeddings: raw.embeddings,
        destination,
        local_filter: raw.index.local,
        shared_filter: raw.index.shared,
    })
}

pub(crate) fn validate_identifier(value: &str, label: &str) -> Result<(), AppError> {
    let bytes = value.as_bytes();
    let valid = (1..=255).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(AppError::configuration(format!("invalid {label}: {value}")))
    }
}

fn validate_roots(
    provider: Option<RawProvider>,
    provider_name: &str,
) -> Result<Option<Vec<PathBuf>>, AppError> {
    let Some(provider) = provider else {
        return Ok(None);
    };
    if provider.roots.is_empty() {
        return Err(AppError::configuration(format!(
            "provider {provider_name} roots must not be empty"
        )));
    }
    let mut unique = HashSet::with_capacity(provider.roots.len());
    for root in &provider.roots {
        if !root.is_absolute() {
            return Err(AppError::configuration(format!(
                "provider {provider_name} root is not absolute: {}",
                root.display()
            )));
        }
        if !unique.insert(root) {
            return Err(AppError::configuration(format!(
                "provider {provider_name} has duplicate root: {}",
                root.display()
            )));
        }
    }
    Ok(Some(provider.roots))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;

    const VALID: &str = r#""version" = 1
"providers" = {"codex" = {"roots" = ["/missing/codex"]}}
"#;

    #[test]
    fn toml_tables_comments_and_remote_models_are_supported_without_json_fallback() {
        let document = r"
version = 1 # node configuration
[providers.codex]
roots = ['/missing/codex']
[index]
all_history = true
destination = 'local'
[embeddings]
api = 'voyage'
url = 'https://api.voyageai.com/v1/embeddings'
model = 'voyage-code-4'
dimensions = 1024
token_env = 'VOYAGE_API_KEY'
";
        let parsed = parse_document(document.as_bytes()).expect("TOML tables");
        assert_eq!(parsed.since_days, None);
        assert_eq!(parsed.embeddings.unwrap().model, "voyage-code-4");
        for invalid in [
            r#"{"version":1,"providers":{}}"#,
            "version = 1\nversion = 1\n[providers]\n",
            "version = 1\n[providers]\n[index]\nall_history = true\nsince_days = 90\n",
            "version = 1\n[providers]\n[index]\nall_history = 'true'\n",
        ] {
            assert_configuration_error(&parse_document(invalid.as_bytes()).unwrap_err());
        }
        assert_configuration_error(&parse_document(b"\xff").unwrap_err());
    }

    #[test]
    fn destinations_and_independent_provider_filters_are_validated() {
        let shared = toml::toml! {
            group_id = "test"
            node_id = "node"
            api_url = "http://127.0.0.1:8787"
            token_env = "URL"
        };
        for (mode, expected) in [
            ("local", IndexDestination::Local),
            ("shared", IndexDestination::Shared),
            ("both", IndexDestination::Both),
        ] {
            let document = toml::toml! {
                version = 1
                providers = {
                claude-code = {roots = ["/claude"]},codex = {roots = ["/codex"]}
                }
                shared = (shared.clone())
                index = {destination = (mode),local = {providers = ["claude-code"]},shared = {providers = ["codex"]}}
            };
            let parsed = parse_document(&toml::to_string(&document).unwrap().into_bytes()).unwrap();
            assert_eq!(parsed.destination, expected);
            let options = crate::ingestion::IndexOptions {
                claude_code: parsed.providers.claude_code,
                codex: parsed.providers.codex,
                since_days: None,
                roots_are_authoritative: true,
            };
            let mut local = options.clone();
            IndexFilter::apply(parsed.local_filter.as_ref(), &mut local);
            assert!(local.claude_code.is_some());
            assert!(local.codex.is_none());
            let mut remote = options;
            IndexFilter::apply(parsed.shared_filter.as_ref(), &mut remote);
            assert!(remote.claude_code.is_none());
            assert!(remote.codex.is_some());
            IndexFilter::apply(Some(&IndexFilter { providers: vec![] }), &mut remote);
            assert!(remote.codex.is_none());
        }
        for index in [
            toml::toml! {
                destination = "unsupported"
            },
            toml::toml! {
                destination = false
            },
            toml::toml! {
                local = false
            },
            toml::toml! {
                shared = { providers = ["other"] }
            },
            toml::toml! {
                local = { providers = ["codex", "codex"] }
            },
            toml::toml! {
                local = { providers = ["claude-code"] }
            },
        ] {
            let document = toml::toml! {
                version = 1
                providers = {codex = {roots = ["/codex"]}}
                index = (index.clone())
                shared = (shared.clone())
            };
            assert!(parse_document(&toml::to_string(&document).unwrap().into_bytes()).is_err());
        }
        for mode in ["shared", "both"] {
            let document = toml::toml! {
                version = 1
                providers = {}
                index = {destination = (mode)}
            };
            assert!(parse_document(&toml::to_string(&document).unwrap().into_bytes()).is_err());
        }
    }

    fn write_config(directory: &TempDir, contents: &str) -> PathBuf {
        let path = directory.path().join("config.toml");
        fs::write(&path, contents).expect("temporary configuration is writable");
        path
    }

    fn assert_configuration_error(error: &crate::AppError) {
        assert_eq!(error.code, 9);
        assert_eq!(error.error.kind, "configuration");
        assert!(!error.error.retryable);
        assert_eq!(error.error.recommended_action, None);
    }

    fn assert_configuration_error_message(error: &crate::AppError, fragment: &str) {
        assert_configuration_error(error);
        assert!(
            error.error.message.contains(fragment),
            "expected configuration error message {:?} to contain {fragment:?}",
            error.error.message
        );
    }

    #[test]
    fn local_and_shared_configuration_is_strict() {
        let parsed = parse_document(VALID.as_bytes()).expect("local config");
        assert_eq!(parsed.since_days, Some(90));
        assert!(parsed.shared.is_none());
        assert_eq!(
            parsed.providers.codex,
            Some(vec![PathBuf::from("/missing/codex")])
        );
        let valid = r#""version" = 1
"providers" = {}
"index" = {"all_history" = true}
"shared" = {"group_id" = "test", "node_id" = "laptop", "api_url" = "http://127.0.0.1:8787", "token_env" = "MASH_DATABASE_URL"}
"#;
        let parsed = parse_document(valid.as_bytes()).expect("shared config");
        assert_eq!(parsed.since_days, None);
        assert_eq!(parsed.shared.expect("shared").node_id, "laptop");
        for invalid in [
            r#""version" = 1
"local_node" = "old"
"nodes" = []
"#,
            r#""version" = 2
"providers" = {}
"#,
            r#""version" = 1
"providers" = {"other" = {"roots" = ["/tmp"]}}
"#,
            r#""version" = 1
"providers" = {"codex" = {"roots" = ["relative"]}}
"#,
            r#""version" = 1
"providers" = {"codex" = {"roots" = ["/tmp", "/tmp"]}}
"#,
            r#""version" = 1
"providers" = {"codex" = {"roots" = []}}
"#,
            r#""version" = 1
"providers" = {}
"index" = {"since_days" = 0}
"#,
            r#""version" = 1
"providers" = {}
"index" = {"since_days" = 36501}
"#,
            r#""version" = 1
"index" = false
"providers" = {}
"#,
            r#""version" = 1
"providers" = {}
"shared" = {"group_id" = "test", "node_id" = "bad name", "api_url" = "http://127.0.0.1:8787", "token_env" = "URL"}
"#,
            r#""version" = 1
"providers" = {}
"shared" = {"group_id" = "test", "node_id" = "ok", "api_url" = "http://127.0.0.1:8787", "token_env" = "1URL"}
"#,
            r#""version" = 1
"providers" = {}
"shared" = {"group_id" = "test", "node_id" = "ok", "api_url" = "http://127.0.0.1:8787", "token_env" = "URL", "password" = "secret"}
"#,
        ] {
            assert_configuration_error(
                &parse_document(invalid.as_bytes()).expect_err("invalid config"),
            );
        }
    }

    #[test]
    fn shared_http_configuration_rejects_credentials_and_old_database_access() {
        let shared = toml::toml! {
            group_id = "test"
            node_id = "laptop"
            api_url = "https://mash.example.net/prefix/"
            token_env = "MASH_NODE_TOKEN"
        };
        let mut document = toml::toml! {
            version = 1
            providers = {}
            shared = (shared.clone())
        };
        assert!(parse_document(&toml::to_string(&document).unwrap().into_bytes()).is_ok());
        for url in [
            "postgresql://host/db",
            "https://user:secret@host",
            "https://host?token=secret",
            "https://host/#fragment",
            "invalid",
        ] {
            document["shared"]["api_url"] = toml::Value::String(url.into());
            assert_configuration_error(
                &parse_document(&toml::to_string(&document).unwrap().into_bytes()).unwrap_err(),
            );
        }
        document["shared"] = toml::Value::Table(toml::toml! {
            group_id = "test"
            node_id = "laptop"
            database_url_env = "MASH_DATABASE_URL"
        });
        assert_configuration_error(
            &parse_document(&toml::to_string(&document).unwrap().into_bytes()).unwrap_err(),
        );
    }

    #[test]
    fn loading_distinguishes_optional_default_and_required_explicit_paths() {
        let dir = TempDir::new().expect("temp");
        let missing = dir.path().join("missing");
        let absent = load_from_paths(Ok(missing.clone()), None).expect("optional default");
        assert!(!absent.loaded);
        assert!(absent.shared.is_none());
        assert!(absent.providers.codex.is_some());
        assert_configuration_error(
            &load_from_paths(Ok(missing.clone()), Some(missing)).expect_err("explicit missing"),
        );
        let path = write_config(&dir, VALID);
        assert!(
            load_from_paths(Ok(path.clone()), Some(path))
                .expect("loaded")
                .loaded
        );
    }

    #[test]
    fn file_input_is_bounded_before_toml_parsing() {
        let directory = TempDir::new().expect("temporary directory");
        let mut exactly = VALID.as_bytes().to_vec();
        exactly.resize(MAX_CONFIG_BYTES, b' ');
        let path = directory.path().join("exact.toml");
        fs::write(&path, exactly).expect("bounded file is writable");
        load_from_paths(Ok(path.clone()), Some(path)).expect("exactly 1 MiB");

        let too_large = directory.path().join("large.toml");
        let bytes = vec![b'{'; MAX_CONFIG_BYTES + 1];
        fs::write(&too_large, bytes).expect("oversized file is writable");
        assert_configuration_error_message(
            &load_from_paths(Ok(too_large.clone()), Some(too_large))
                .expect_err("1 MiB plus one byte"),
            "exceeds 1048576 bytes",
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_must_resolve_to_a_regular_file() {
        use std::os::unix::fs::symlink;

        let directory = TempDir::new().expect("temporary directory");
        let target = write_config(&directory, VALID);
        let link = directory.path().join("link.toml");
        symlink(&target, &link).expect("file symlink");
        let loaded =
            load_from_paths(Ok(link.clone()), Some(link)).expect("symlink to regular file");
        assert_eq!(
            loaded.path,
            fs::canonicalize(target).expect("canonical target")
        );

        let directory_link = directory.path().join("directory.toml");
        symlink(directory.path(), &directory_link).expect("directory symlink");
        assert_configuration_error(
            &load_from_paths(Ok(directory_link.clone()), Some(directory_link))
                .expect_err("symlink to directory"),
        );

        let broken = directory.path().join("broken.toml");
        symlink(directory.path().join("absent"), &broken).expect("broken symlink");
        assert_configuration_error(
            &load_from_paths(Ok(broken.clone()), Some(broken)).expect_err("broken symlink"),
        );
    }

    #[test]
    fn explicit_directory_and_malformed_file_are_typed_errors() {
        let directory = TempDir::new().expect("temporary directory");
        let directory_path = directory.path().to_path_buf();
        assert_configuration_error(
            &load_from_paths(Ok(directory_path.clone()), Some(directory_path))
                .expect_err("directory is not a configuration file"),
        );

        let malformed = write_config(&directory, "{");
        assert_configuration_error_message(
            &load_from_paths(Ok(malformed.clone()), Some(malformed)).expect_err("malformed TOML"),
            "invalid configuration TOML",
        );
    }

    #[cfg(unix)]
    #[test]
    fn fifo_is_rejected_without_blocking_on_open() {
        let directory = TempDir::new().expect("temporary directory");
        let fifo = directory.path().join("config.fifo");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo is available on supported Unix platforms");
        assert!(status.success(), "mkfifo creates the test FIFO");
        assert_configuration_error_message(
            &load_from_paths(Ok(fifo.clone()), Some(fifo)).expect_err("FIFO is not a regular file"),
            "not a regular file",
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_file_failure_is_typed_when_permissions_are_enforced() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TempDir::new().expect("temporary directory");
        let path = write_config(&directory, VALID);
        let mut permissions = fs::metadata(&path)
            .expect("configuration metadata")
            .permissions();
        permissions.set_mode(0o000);
        fs::set_permissions(&path, permissions).expect("permissions are mutable");

        let result = load_from_paths(Ok(path.clone()), Some(path.clone()));

        let mut restored = fs::metadata(&path)
            .expect("configuration metadata")
            .permissions();
        restored.set_mode(0o600);
        fs::set_permissions(&path, restored).expect("permissions are restorable");

        let error = result.expect_err("permission bits make the configuration unreadable");
        assert_configuration_error_message(&error, "failed to open");
    }

    #[test]
    fn default_path_failure_is_typed() {
        let error = load_from_paths(
            Err(crate::AppError::configuration(
                "no absolute config directory",
            )),
            None,
        )
        .expect_err("missing platform path");
        assert_configuration_error(&error);
    }
}
