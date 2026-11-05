use super::*;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Phase {
    Complete,
    Incomplete,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct Progress {
    phase: Phase,
    malformed_records: Option<u64>,
    eta_seconds: Option<u64>,
    record_diagnostics: Option<Diagnostics>,
}

#[derive(Deserialize)]
struct Summary {
    silver: Certificate,
    malformed_records: u64,
    record_diagnostics: Diagnostics,
}

#[derive(Deserialize)]
struct Diagnostics {
    samples: Vec<RecordIssue>,
}

impl Diagnostics {
    fn assert_source(&self, source: &std::path::Path, malformed: u64) {
        assert_eq!(u64::try_from(self.samples.len()).unwrap(), malformed);
        if malformed > 0 {
            let issue = &self.samples[0];
            assert_eq!(issue.source, source);
            assert_eq!(issue.provider, Provider::ClaudeCode);
            assert_eq!(issue.line, 2);
            assert_eq!(issue.category, Category::Data);
        }
    }
}

#[derive(Deserialize)]
struct RecordIssue {
    provider: Provider,
    source: std::path::PathBuf,
    line: usize,
    category: Category,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Provider {
    ClaudeCode,
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Category {
    Data,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct Status {
    ready: bool,
    silver: Certificate,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Certificate {
    revision: u64,
    interpretation_version: u32,
    completeness: Completeness,
    rejected_sources: u64,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Completeness {
    Complete,
    Incomplete,
}

#[test]
#[ignore = "requires disposable Qdrant"]
fn incomplete_index_progress_matches_exit_status_and_recovers_after_source_repair() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let claude = directory.path().join("claude");
    let codex = directory.path().join("codex");
    std::fs::create_dir_all(&claude).unwrap();
    std::fs::create_dir_all(&codex).unwrap();
    let malformed = claude.join("broken.jsonl");
    let corrupt = "\n{\"sessionId\":[\"PRIVATE-CLI-RECORD-CANARY\"]}\n";
    std::fs::write(&malformed, corrupt).unwrap();
    write_config(&config, &claude, &codex);
    configure_qdrant(&config);
    let models = directory.path().join("models");
    create_valid_model_marker(&models);
    let database = directory.path().join("index.sqlite3");
    for (expected_phase, exit_code, malformed_records, ready) in [
        (Phase::Incomplete, 9, 1, true),
        (Phase::Complete, 0, 0, true),
        (Phase::Incomplete, 9, 1, true),
    ] {
        let output = Command::cargo_bin("mash")
            .unwrap()
            .args([
                "--config",
                config.to_str().unwrap(),
                "--db",
                database.to_str().unwrap(),
                "--models-dir",
                models.to_str().unwrap(),
                "index",
            ])
            .env_remove("MASH_TRACE")
            // Each run ends on its own; the guard only kills a hung index.
            .timeout(HANG_GUARD)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(exit_code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let summary: Summary = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(summary.malformed_records, malformed_records);
        assert_eq!(summary.silver.interpretation_version, 3);
        assert_eq!(summary.silver.rejected_sources, malformed_records);
        assert_eq!(
            summary.silver.completeness,
            if malformed_records == 0 {
                Completeness::Complete
            } else {
                Completeness::Incomplete
            }
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-CLI-RECORD-CANARY"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-CLI-RECORD-CANARY"));
        summary
            .record_diagnostics
            .assert_source(&malformed, malformed_records);
        let events: Vec<Progress> = output
            .stderr
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        let terminal: Vec<_> = events
            .iter()
            .filter(|event| event.phase != Phase::Other)
            .collect();
        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0].phase, expected_phase);
        assert_eq!(terminal[0].malformed_records, Some(malformed_records));
        assert_eq!(terminal[0].eta_seconds, (exit_code == 0).then_some(0));
        terminal[0]
            .record_diagnostics
            .as_ref()
            .unwrap()
            .assert_source(&malformed, malformed_records);
        assert_readiness(&config, &database, &models, ready, &summary.silver);
        if malformed_records > 0 {
            std::fs::remove_file(&malformed).unwrap();
        } else {
            std::fs::write(&malformed, corrupt).unwrap();
        }
    }
    cleanup_local_gold(&database, "local");
}

fn assert_readiness(
    config: &Path,
    database: &Path,
    models: &Path,
    ready: bool,
    certificate: &Certificate,
) {
    let paths = [
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
    ];
    let status: Status =
        serde_json::from_value(run_json(&[paths.as_slice(), &["status"]].concat())).unwrap();
    assert_eq!(status.ready, ready);
    assert_eq!(&status.silver, certificate);
    if !ready {
        let search = Command::cargo_bin("mash")
            .unwrap()
            .args([paths.as_slice(), &["search", "cbioportal", "--local"]].concat())
            .output()
            .unwrap();
        assert_eq!(search.status.code(), Some(8));
        assert_eq!(search.stdout, [] as [u8; 0]);
    }
}

#[test]
#[ignore = "requires disposable Qdrant"]
fn rejected_source_preserves_cli_search_and_offline_views_rebuild() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("provider");
    std::fs::create_dir(&root).unwrap();
    let broken = root.join("broken.jsonl");
    let corrupt = b"{\"type\":\"response_item\",\"payload\":";
    std::fs::write(&broken, corrupt).unwrap();
    let healthy = root.join("healthy.jsonl");
    std::fs::write(&healthy, concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"healthy-thread\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"request\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"retainedneedle history\"}]}}\n",
    )).unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, format!(
        "version=1\n[providers]\ncodex={{roots=[{}]}}\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n",
        toml::Value::String(root.to_str().unwrap().into()),
    )).unwrap();
    let database = directory.path().join("index.sqlite3");
    configure_qdrant(&config);
    let models = directory.path().join("absent-models");
    let base = [
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
    ];
    for operation in [
        vec!["index"],
        vec!["index", "--rebuild-local-views", "--full"],
    ] {
        let result = Command::cargo_bin("mash")
            .unwrap()
            .args(base)
            .args(operation)
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(9),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let indexed: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(indexed["silver"]["completeness"], "incomplete");
        assert_eq!(indexed["silver"]["rejected_sources"], 1);
        let status = run_json(&[base.as_slice(), &["status"]].concat());
        assert_eq!(status["ready"], true);
        let results = run_json(
            &[
                base.as_slice(),
                &["search", "retainedneedle", "--local", "--unit", "messages"],
            ]
            .concat(),
        );
        assert_eq!(results["results"].as_array().unwrap().len(), 1);
        assert_eq!(std::fs::read(&broken).unwrap(), corrupt);
        // The second operation must use stored silver, without provider files.
        std::fs::remove_file(&healthy).ok();
    }
    assert!(!models.exists());
    cleanup_local_gold(&database, "local");
}
