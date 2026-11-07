use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::loading::{MAX_CONFIG_BYTES, load_from_paths};
use super::*;

#[test]
fn local_qdrant_configuration_is_strict_and_does_not_resolve_credentials() {
    let prefix = "version = 1\n[providers]\n";
    let defaults = parse_document(prefix.as_bytes()).unwrap();
    assert_eq!(defaults.qdrant, QdrantConfig::default());
    let parsed = parse_document(
        format!(
            "{prefix}[qdrant]\nurl = 'https://localhost:6333'\n\
             api_key_env = 'MASH_TEST_UNRESOLVED_QDRANT_KEY'\nmemory = 'cold'\n\
             [qdrant.vector_search]\nalgorithm = 'exact'\n"
        )
        .as_bytes(),
    )
    .unwrap();
    assert_eq!(parsed.qdrant.memory, crate::views::qdrant::MemoryTier::Cold);
    assert_eq!(
        parsed.qdrant.vector_search,
        crate::views::vector::Search::Exact {}
    );
    for settings in [
        "url = 'file:///db'",
        "url = 'http://user:secret@localhost:6333'",
        "url = 'http://localhost:6333/path'",
        "url = 'http://localhost:6333/?key=secret'",
        "url = 'http://localhost:6333/#fragment'",
        "api_key_env = ''",
        "api_key_env = '1KEY'",
        "api_key_env = 'KEY-NAME'",
        "api_key = 'secret'",
        "memory = 'unknown'",
        "[qdrant.vector_search]\nalgorithm = 'hnsw'\nef_runtime = 0",
    ] {
        assert!(
            parse_document(format!("{prefix}[qdrant]\n{settings}\n").as_bytes()).is_err(),
            "accepted {settings}"
        );
    }
}

/// Fixtures name Unix-style absolute roots; on Windows a root needs a drive
/// to be absolute, as Mash requires, so quoted absolute paths gain one there.
fn portable(text: &str) -> String {
    if cfg!(windows) {
        text.replace("\"/", "\"C:/").replace("'/", "'C:/")
    } else {
        text.to_owned()
    }
}

fn parse_document(bytes: &[u8]) -> Result<super::document::ParsedDocument, crate::AppError> {
    match std::str::from_utf8(bytes) {
        Ok(text) => super::document::parse_document(portable(text).as_bytes()),
        Err(_) => super::document::parse_document(bytes),
    }
}

#[test]
fn removed_providers_are_rejected_in_roots_and_index_allowlists() {
    for provider in [
        "gemini-cli",
        "windsurf",
        "cline",
        "aider",
        "hermes",
        "openclaw",
        "pi",
    ] {
        for config in [
            format!("version = 1\n[providers.{provider}]\nroots = ['/history']\n"),
            format!("version = 1\n[providers]\n[index.local]\nproviders = ['{provider}']\n"),
            format!("version = 1\n[providers]\n[index.shared]\nproviders = ['{provider}']\n"),
        ] {
            assert!(super::document::parse_document(config.as_bytes()).is_err());
        }
    }
}

const VALID: &str = r#""version" = 1
"providers" = {"codex" = {"roots" = ["/missing/codex"]}}
"#;

#[test]
fn built_in_views_configuration_rejects_retired_gold_keys() {
    for section in ["gold", "gold.build", "gold.query"] {
        assert!(parse_document(format!("{VALID}\n[{section}]\n").as_bytes()).is_err());
    }
    assert!(parse_document(
        format!("{VALID}\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n").as_bytes()
    ).is_ok());
}

#[test]
fn record_capture_policy_is_strict_and_does_not_change_views_settings() {
    let defaults = parse_document(VALID.as_bytes()).unwrap();
    assert!(defaults.records.enabled);
    assert!(defaults.records.backfill_since.is_none());
    let configured = parse_document(
        format!("{VALID}\n[records]\nenabled=false\nbackfill_since=1000\n").as_bytes(),
    )
    .unwrap();
    assert!(!configured.records.enabled);
    assert_eq!(configured.records.backfill_since, Some(1000));
    assert_eq!(configured.views, defaults.views);
    for invalid in ["backfill_since=-1", "days=30", "enabled='true'"] {
        assert!(parse_document(format!("{VALID}\n[records]\n{invalid}\n").as_bytes()).is_err());
    }
}

#[test]
fn views_fusion_policy_is_query_only_and_strictly_bounded() {
    let defaults = parse_document(VALID.as_bytes()).unwrap().views;
    let changed = parse_document(
        format!(
            "{VALID}\n[views.query.fusion]\nrank_constant=10\nlexical_weight=4\nsemantic_weight=2\n"
        )
        .as_bytes(),
    )
    .unwrap()
    .views;
    assert_eq!(changed.build, defaults.build);
    assert_eq!(changed.query.fusion.rank_constant, 10);
    assert_eq!(changed.query.fusion.lexical_weight, 4);
    assert_eq!(changed.query.fusion.semantic_weight, 2);
    for invalid in [
        "rank_constant=0",
        "rank_constant=1001",
        "lexical_weight=0",
        "semantic_weight=101",
        "lexical_weight=-1",
        "lexical_weight=1.5",
        "unknown=1",
    ] {
        assert!(
            parse_document(format!("{VALID}\n[views.query.fusion]\n{invalid}\n").as_bytes())
                .is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn views_query_defaults_are_typed_bounded_and_reported_in_status() {
    use crate::search_results::{Page, SearchUnit};
    let page = |offset, limit| Page { offset, limit };
    let defaults = parse_document(VALID.as_bytes()).unwrap().views;
    assert_eq!(
        defaults.query.resolve(None, 0, None).unwrap(),
        (page(0, 20), SearchUnit::Chats)
    );
    let directory = TempDir::new().unwrap();
    let path = write_config(
        &directory,
        &format!("{VALID}\n[views.query]\nunit = 'messages'\nlimit = 7\n"),
    );
    let resolved = load(Some(&path)).unwrap();
    assert_eq!(
        resolved.views.query.resolve(None, 0, None).unwrap(),
        (page(0, 7), SearchUnit::Messages)
    );
    assert_eq!(
        resolved
            .views
            .query
            .resolve(Some(3), 6, Some(SearchUnit::Chats))
            .unwrap(),
        (page(6, 3), SearchUnit::Chats)
    );
    assert_eq!(
        resolved
            .views
            .query
            .resolve(None, 0, Some(SearchUnit::Chats))
            .unwrap(),
        (page(0, 7), SearchUnit::Chats)
    );
    // Pages stay inside the unit's ranked window; sizes share the TOML bound.
    let query = &resolved.views.query;
    assert!(query.resolve(Some(0), 0, None).is_err());
    assert!(query.resolve(Some(201), 0, None).is_err());
    assert!(query.resolve(None, 49, None).is_ok());
    assert!(query.resolve(None, 50, None).is_err());
    assert!(query.resolve(None, 199, Some(SearchUnit::Chats)).is_ok());
    assert!(query.resolve(None, 200, Some(SearchUnit::Chats)).is_err());
    let status = serde_json::to_value(resolved.status()).unwrap();
    assert_eq!(status["views"]["query"]["unit"], "messages");
    assert_eq!(status["views"]["query"]["limit"], 7);
    assert_eq!(status["views"]["query"]["intent"], "related-discussion");
    let exact =
        parse_document(format!("{VALID}\n[views.query]\nintent='exact-mention'\n").as_bytes())
            .unwrap();
    assert_eq!(
        exact.views.query.intent,
        crate::views::exact::Intent::ExactMention
    );
    for invalid in [
        "[views.query]\nlimit = 0",
        "[views.query]\nlimit = 201",
        "[views.query]\nlimit = -1",
        "[views.query]\nlimit = '7'",
        "[views.query]\nunit = 'chunks'",
        "[views.query]\nintent = 'decisions'",
        "[views.query]\nunknown = true",
        "[views]\nprofiles = []",
        "[views.query]\nlimit = 1\n[views.query]\nlimit = 2",
    ] {
        assert_configuration_error(
            &parse_document(format!("{VALID}\n{invalid}").as_bytes()).unwrap_err(),
        );
    }
}

#[test]
fn toml_tables_and_comments_are_supported_without_json_fallback() {
    let document = r"
version = 1 # node configuration
[providers.codex]
roots = ['/missing/codex']
[index]
all_history = true
destination = 'local'
";
    let parsed = parse_document(document.as_bytes()).expect("TOML tables");
    assert_eq!(parsed.since_days, None);
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

/// Both mode publishes exactly the local index; per-destination provider
/// filters were retired and are rejected like any unknown setting.
#[test]
fn destinations_are_validated_and_retired_provider_filters_are_rejected() {
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
            providers = {codex = {roots = ["/codex"]}}
            shared = (shared.clone())
            index = {destination = (mode)}
        };
        let parsed = parse_document(&toml::to_string(&document).unwrap().into_bytes()).unwrap();
        assert_eq!(parsed.destination, expected);
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
    fs::write(&path, portable(contents)).expect("temporary configuration is writable");
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
        vec![PathBuf::from(format!(
            "{}/missing/codex",
            if cfg!(windows) { "C:" } else { "" }
        ))]
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
    let mut exactly = portable(VALID).into_bytes();
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

#[test]
fn removed_remote_embeddings_settings_are_rejected() {
    for setting in [
        "[embeddings]\nurl='https://embed.example/v1/embeddings'\nmodel='test'\ndimensions=384",
        "[embeddings]\napi='voyage'\nmodel='voyage-4'",
        "[redaction]\nremote_embeddings=false",
    ] {
        let error = parse_document(format!("{VALID}\n{setting}\n").as_bytes()).unwrap_err();
        assert_configuration_error(&error);
        assert!(error.error.message.contains("unknown field"));
    }
}

#[test]
fn redaction_switches_default_on_and_reach_each_destination() {
    let directory = TempDir::new().unwrap();
    let defaults = load(Some(&write_config(&directory, VALID))).unwrap();
    assert_eq!(defaults.redaction, crate::redaction::Settings::default());
    assert!(defaults.redaction.output().enabled());
    let path = write_config(
        &directory,
        &format!(
            "{VALID}\n[shared]\ngroup_id='g'\nnode_id='n'\napi_url='https://mash.example'\ntoken_env='T'\n\
             [redaction]\noutput=false\npublication=false\n"
        ),
    );
    let resolved = load(Some(&path)).unwrap();
    assert!(!resolved.redaction.output().enabled());
    assert!(!resolved.shared.unwrap().publication_redaction.enabled());
    let status = serde_json::to_value(load(Some(&path)).unwrap().status()).unwrap();
    assert_eq!(status["redaction"]["output"], false);
    // Each switch is independent, and unknown keys are rejected.
    let partial = parse_document(format!("{VALID}\n[redaction]\noutput=false\n").as_bytes())
        .unwrap()
        .redaction;
    assert!(!partial.output && partial.publication);
    assert_configuration_error(
        &parse_document(format!("{VALID}\n[redaction]\nlogs=false\n").as_bytes()).unwrap_err(),
    );
}
