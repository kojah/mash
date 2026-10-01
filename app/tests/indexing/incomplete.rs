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
}

#[test]
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
    let models = directory.path().join("models");
    create_valid_model_marker(&models);
    let database = directory.path().join("index.sqlite3");
    for (expected_phase, exit_code, malformed_records, ready) in [
        (Phase::Incomplete, 9, 1, false),
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
            .timeout(std::time::Duration::from_secs(5))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(exit_code));
        let summary: Summary = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(summary.malformed_records, malformed_records);
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
        let status: Status = serde_json::from_value(run_json(&[
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "status",
        ]))
        .unwrap();
        assert_eq!(status.ready, ready);
        if !ready {
            let search = Command::cargo_bin("mash")
                .unwrap()
                .args([
                    "--config",
                    config.to_str().unwrap(),
                    "--db",
                    database.to_str().unwrap(),
                    "--models-dir",
                    models.to_str().unwrap(),
                    "search",
                    "cbioportal",
                    "--local",
                    "--no-worker",
                ])
                .output()
                .unwrap();
            assert_eq!(search.status.code(), Some(8));
            assert_eq!(search.stdout, [] as [u8; 0]);
        }
        if malformed_records > 0 {
            std::fs::remove_file(&malformed).unwrap();
        } else {
            std::fs::write(&malformed, corrupt).unwrap();
        }
    }
}
