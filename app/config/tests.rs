use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::document::parse_document;
use super::loading::{MAX_CONFIG_BYTES, load_from_paths};
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
        ("both", IndexDestination::Both),
    ] {
        let document = toml::toml! {
            version = 1
            providers = {
            claude-code = {roots = ["/claude"]},codex = {roots = ["/codex"]}
            }
            shared = (shared.clone())
            index = {destination = (mode),local = {providers = ["claude-code", "codex"]},shared = {providers = ["codex"]}}
        };
        let parsed = parse_document(&toml::to_string(&document).unwrap().into_bytes()).unwrap();
        assert_eq!(parsed.destination, expected);
        let options = crate::ingestion::IndexOptions {
            providers: parsed.providers,
            since_days: None,
            roots_are_authoritative: true,
        };
        let mut local = options.clone();
        IndexFilter::apply(parsed.local_filter.as_ref(), &mut local);
        assert!(local.providers.contains_key(&Provider::ClaudeCode));
        assert!(local.providers.contains_key(&Provider::Codex));
        let mut remote = options;
        IndexFilter::apply(parsed.shared_filter.as_ref(), &mut remote);
        assert!(!remote.providers.contains_key(&Provider::ClaudeCode));
        assert!(remote.providers.contains_key(&Provider::Codex));
        IndexFilter::apply(Some(&IndexFilter { providers: vec![] }), &mut remote);
        assert!(remote.providers.is_empty());
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
    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
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
        parsed.providers[&Provider::Codex],
        vec![PathBuf::from("/missing/codex")]
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
    assert!(absent.providers.contains_key(&Provider::Codex));
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
        &load_from_paths(Ok(too_large.clone()), Some(too_large)).expect_err("1 MiB plus one byte"),
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
    let loaded = load_from_paths(Ok(link.clone()), Some(link)).expect("symlink to regular file");
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
