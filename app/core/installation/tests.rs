use super::*;

fn environment(root: &Path) -> Environment {
    Environment {
        home: root.to_path_buf(),
        codex: root.join(".codex"),
        claude: root.join(".claude"),
        cwd: root.join("project"),
        path: std::ffi::OsString::new(),
    }
}

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn installation_cli_only_is_not_broken_but_expected_clients_are() {
    let root = tempfile::tempdir().unwrap();
    let environment = environment(root.path());
    let report = collect(Some(&environment), &[]);
    assert_eq!(report.summary.attention, 0);
    assert!(report.clients.iter().all(|client| !client.expected));
    let report = collect(Some(&environment), &[Agent::Codex]);
    assert_eq!(report.clients.len(), 1);
    assert_eq!(report.summary.attention, 2);
}

#[test]
fn installation_partial_setup_warns_and_never_serializes_secrets() {
    let root = tempfile::tempdir().unwrap();
    let environment = environment(root.path());
    write(
        &environment.codex.join("config.toml"),
        "[mcp_servers.mash]\ncommand = '/missing/mash'\nargs = ['mcp', 'secret-argument']\n[mcp_servers.mash.env]\nTOKEN = 'secret-token'\n",
    );
    let report = collect(Some(&environment), &[]);
    assert!(report.summary.attention >= 2);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("secret-argument"));
    assert!(!json.contains("secret-token"));
    assert!(report.clients[0].launch.is_none());
}

#[test]
fn installation_malformed_settings_are_findings_not_errors() {
    let root = tempfile::tempdir().unwrap();
    let environment = environment(root.path());
    write(
        &environment.codex.join("config.toml"),
        "[mcp_servers.mash]\ncommand = SECRET_VALUE_INVALID\n",
    );
    let report = collect(Some(&environment), &[Agent::Codex]);
    assert!(report.summary.attention > 0);
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("SECRET_VALUE_INVALID")
    );
    assert!(report.clients[0].findings.iter().any(|finding| matches!(
        finding.component,
        Component::Configuration
    ) && finding.state == State::Invalid));
}

#[test]
fn installation_codex_checks_hook_files_trust_and_versions() {
    let root = tempfile::tempdir().unwrap();
    let environment = environment(root.path());
    write(
        &environment.codex.join("config.toml"),
        "[plugins.'mash@mash-local']\nenabled = true\n",
    );
    let plugin = environment
        .codex
        .join("plugins/cache/mash-local/mash/local");
    write(
        &plugin.join(".codex-plugin/plugin.json"),
        include_str!("../../../plugins/mash/.codex-plugin/plugin.json"),
    );
    write(&plugin.join("skills/history/SKILL.md"), "history skill");
    write(
        &plugin.join("skills/history/contract.json"),
        include_str!("../../../plugins/mash/skills/history/contract.json"),
    );
    write(
        &plugin.join("hooks/hooks.json"),
        include_str!("../../../plugins/mash/hooks/hooks.json"),
    );
    for script in ["session_start.py", "prompt_recall.py"] {
        write(&plugin.join("hooks").join(script), "# hook");
    }
    let report = collect(Some(&environment), &[]);
    let findings = &report.clients[0].findings;
    assert_eq!(
        findings
            .iter()
            .filter(|finding| matches!(finding.component, Component::HookTrust)
                && finding.state == State::Missing)
            .count(),
        2
    );
    assert!(
        findings
            .iter()
            .any(|finding| matches!(finding.component, Component::Plugin)
                && finding.state == State::Present)
    );
    std::fs::remove_file(plugin.join("hooks/session_start.py")).unwrap();
    write(&plugin.join("skills/history/contract.json"), "{}");
    let report = collect(Some(&environment), &[]);
    assert!(
        report.clients[0]
            .findings
            .iter()
            .any(|finding| matches!(finding.component, Component::Hooks)
                && finding.state == State::Missing)
    );
    assert!(
        report.clients[0]
            .findings
            .iter()
            .any(|finding| matches!(finding.component, Component::Skill)
                && finding.state == State::Outdated)
    );
}

#[test]
fn installation_claude_disabled_hooks_and_mcp_are_reported() {
    let root = tempfile::tempdir().unwrap();
    let environment = environment(root.path());
    write(
        &environment.claude.join("settings.json"),
        r#"{"enabledPlugins":{"mash@mash-local":true},"disableAllHooks":true,"disabledMcpServers":["mash"]}"#,
    );
    let report = collect(Some(&environment), &[]);
    let client = &report.clients[1];
    assert!(client.expected);
    assert!(client.launch.is_none());
    for component in [Component::Hooks, Component::Mcp] {
        assert!(
            client
                .findings
                .iter()
                .any(|finding| std::mem::discriminant(&finding.component)
                    == std::mem::discriminant(&component)
                    && finding.state == State::Disabled)
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn installation_probe_reports_failed_handshake_without_leaking_stderr() {
    let root = tempfile::tempdir().unwrap();
    let mut environment = environment(root.path());
    environment.cwd = root.path().to_path_buf();
    write(
        &environment.codex.join("config.toml"),
        "[mcp_servers.mash]\ncommand = '/bin/sh'\nargs = ['-c', 'echo SECRET >&2; exit 1']\n",
    );
    let mut report = collect(Some(&environment), &[Agent::Codex]);
    probe::check(&mut report.clients[0]).await;
    assert!(
        report.clients[0]
            .findings
            .iter()
            .any(|finding| matches!(finding.component, Component::Probe)
                && finding.state == State::Failed)
    );
    assert!(!serde_json::to_string(&report).unwrap().contains("SECRET"));
}

#[cfg(unix)]
#[test]
fn installation_project_override_does_not_probe_user_registration() {
    let root = tempfile::tempdir().unwrap();
    let environment = environment(root.path());
    write(
        &environment.codex.join("config.toml"),
        "[mcp_servers.mash]\ncommand = '/bin/true'\n",
    );
    write(
        &environment.cwd.join(".codex/config.toml"),
        "[mcp_servers.mash]\nenabled = false\n",
    );
    let report = collect(Some(&environment), &[Agent::Codex]);
    assert!(report.clients[0].launch.is_none());
    assert!(report.summary.unverified > 0);
}

#[cfg(unix)]
#[test]
fn installation_complete_claude_user_setup_has_no_attention_findings() {
    let root = tempfile::tempdir().unwrap();
    let mut environment = environment(root.path());
    environment.path = "/usr/bin:/bin".into();
    write(
        &environment.claude.join("settings.json"),
        r#"{"enabledPlugins":{"mash@mash-local":true}}"#,
    );
    write(
        &environment.home.join(".claude.json"),
        r#"{"mcpServers":{"mash":{"command":"/bin/true"}}}"#,
    );
    let plugin = root.path().join("plugin");
    let registry =
        serde_json::json!({"plugins":{"mash@mash-local":[{"scope":"user","installPath":plugin}]}});
    write(
        &environment.claude.join("plugins/installed_plugins.json"),
        &registry.to_string(),
    );
    write(
        &plugin.join(".claude-plugin/plugin.json"),
        include_str!("../../../plugins/mash/.claude-plugin/plugin.json"),
    );
    write(
        &plugin.join("skills/history/contract.json"),
        include_str!("../../../plugins/mash/skills/history/contract.json"),
    );
    write(&plugin.join("skills/history/SKILL.md"), "history");
    write(
        &plugin.join("hooks/hooks.json"),
        include_str!("../../../plugins/mash/hooks/hooks.json"),
    );
    for name in [
        "session_start.py",
        "prompt_recall.py",
        "recall_common.py",
        "record.py",
    ] {
        write(&plugin.join("hooks").join(name), "# script");
    }
    let report = collect(Some(&environment), &[Agent::ClaudeCode]);
    assert_eq!(report.summary.attention, 0, "{report:?}");
    assert!(report.clients[0].launch.is_some());
    assert!(report.clients[0].expected);
}

#[cfg(unix)]
#[tokio::test]
async fn installation_probe_skips_watch_and_preserves_disabled_registration() {
    let root = tempfile::tempdir().unwrap();
    let environment = environment(root.path());
    for config in [
        "[mcp_servers.mash]\ncommand = '/bin/true'\nargs = ['mcp', '--watch']\n",
        "[mcp_servers.mash]\ncommand = '/bin/true'\nenabled = false\n",
    ] {
        write(&environment.codex.join("config.toml"), config);
        let mut report = collect(Some(&environment), &[Agent::Codex]);
        if config.contains("--watch") {
            assert!(report.clients[0].findings.iter().any(|finding| matches!(
                finding.component,
                Component::Mcp
            ) && finding.state
                == State::Outdated));
        }
        probe::check(&mut report.clients[0]).await;
        assert!(report.clients[0].findings.iter().any(|finding| matches!(
            finding.component,
            Component::Probe
        ) && finding.state
            == State::Unverified));
    }
}

#[test]
fn installation_multiple_codex_copies_are_ambiguous() {
    let root = tempfile::tempdir().unwrap();
    let environment = environment(root.path());
    write(
        &environment.codex.join("config.toml"),
        "[plugins.'mash@mash-local']\nenabled = true\n",
    );
    for version in ["0.0.1", "0.0.2"] {
        std::fs::create_dir_all(
            environment
                .codex
                .join("plugins/cache/mash-local/mash")
                .join(version),
        )
        .unwrap();
    }
    let report = collect(Some(&environment), &[Agent::Codex]);
    assert!(report.clients[0].findings.iter().any(|finding| matches!(
        finding.component,
        Component::Plugin
    ) && finding.state
        == State::Unverified));
}
