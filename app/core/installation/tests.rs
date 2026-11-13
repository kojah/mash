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
    let report = collect(Some(&environment), &[], &[]);
    assert_eq!(report.summary.attention, 0);
    assert!(report.clients.iter().all(|client| !client.expected));
    let report = collect(Some(&environment), &[Agent::Codex], &[]);
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
    let report = collect(Some(&environment), &[], &[]);
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
    let report = collect(Some(&environment), &[Agent::Codex], &[]);
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
    write(
        &plugin.join("skills/mash-history/SKILL.md"),
        "mash-history skill",
    );
    write(
        &plugin.join("skills/mash-history/contract.json"),
        include_str!("../../../plugins/mash/skills/mash-history/contract.json"),
    );
    write(
        &plugin.join("hooks/hooks.json"),
        include_str!("../../../plugins/mash/hooks/hooks.json"),
    );
    for script in [
        "session_start.py",
        "prompt_recall.py",
        "file_context.py",
        "record_capture.py",
        "python.sh",
    ] {
        write(&plugin.join("hooks").join(script), "# hook");
    }
    let report = collect(Some(&environment), &[], &[]);
    let findings = &report.clients[0].findings;
    assert_eq!(
        findings
            .iter()
            .filter(|finding| matches!(finding.component, Component::HookTrust)
                && finding.state == State::Missing)
            .count(),
        3,
        "every bundled hook needs its own Codex approval"
    );
    assert!(
        findings
            .iter()
            .any(|finding| matches!(finding.component, Component::Plugin)
                && finding.state == State::Present)
    );
    std::fs::remove_file(plugin.join("hooks/session_start.py")).unwrap();
    write(&plugin.join("skills/mash-history/contract.json"), "{}");
    let report = collect(Some(&environment), &[], &[]);
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
    let report = collect(Some(&environment), &[], &[]);
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
    let mut report = collect(Some(&environment), &[Agent::Codex], &[]);
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
    let report = collect(Some(&environment), &[Agent::Codex], &[]);
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
        &plugin.join("skills/mash-history/contract.json"),
        include_str!("../../../plugins/mash/skills/mash-history/contract.json"),
    );
    write(&plugin.join("skills/mash-history/SKILL.md"), "mash-history");
    write(
        &plugin.join("hooks/hooks.json"),
        include_str!("../../../plugins/mash/hooks/hooks.json"),
    );
    for name in [
        "session_start.py",
        "prompt_recall.py",
        "file_context.py",
        "record_capture.py",
        "recall_common.py",
        "record_recall.py",
        "python.sh",
    ] {
        write(&plugin.join("hooks").join(name), "# script");
    }
    let report = collect(Some(&environment), &[Agent::ClaudeCode], &[]);
    assert_eq!(report.summary.attention, 0, "{report:?}");
    assert!(report.clients[0].launch.is_some());
    assert!(report.clients[0].expected);
}

/// A complete plugin tree like the one the passing user setup installs.
#[cfg(unix)]
fn write_claude_plugin(plugin: &Path) {
    write(
        &plugin.join(".claude-plugin/plugin.json"),
        include_str!("../../../plugins/mash/.claude-plugin/plugin.json"),
    );
    write(
        &plugin.join("skills/mash-history/contract.json"),
        include_str!("../../../plugins/mash/skills/mash-history/contract.json"),
    );
    write(&plugin.join("skills/mash-history/SKILL.md"), "mash-history");
    write(
        &plugin.join("hooks/hooks.json"),
        include_str!("../../../plugins/mash/hooks/hooks.json"),
    );
    for name in [
        "session_start.py",
        "prompt_recall.py",
        "file_context.py",
        "record_capture.py",
        "recall_common.py",
        "record_recall.py",
        "python.sh",
    ] {
        write(&plugin.join("hooks").join(name), "# script");
    }
}

#[cfg(unix)]
#[test]
fn installation_claude_directory_marketplace_is_inspected_in_place() {
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
    // The registry still names the cache copy from the first installation.
    let stale = root.path().join("cache/mash/0.1.15");
    write(
        &stale.join(".claude-plugin/plugin.json"),
        r#"{"name":"mash","version":"0.1.15"}"#,
    );
    let registry =
        serde_json::json!({"plugins":{"mash@mash-local":[{"scope":"user","installPath":stale}]}});
    write(
        &environment.claude.join("plugins/installed_plugins.json"),
        &registry.to_string(),
    );
    let checkout = root.path().join("checkout");
    write_claude_plugin(&checkout.join("plugins/mash"));
    let marketplaces = environment.claude.join("plugins/known_marketplaces.json");
    let known = |kind: &str, catalog_source: serde_json::Value| {
        write(
            &checkout.join(".claude-plugin/marketplace.json"),
            &serde_json::json!({"name":"mash","plugins":[{"name":"mash","source":catalog_source}]})
                .to_string(),
        );
        write(
            &marketplaces,
            &serde_json::json!({"mash-local":{"source":{"source":kind,"path":checkout},"installLocation":checkout}})
                .to_string(),
        );
        collect(Some(&environment), &[Agent::ClaudeCode], &[])
            .summary
            .attention
    };
    assert_eq!(known("directory", "./plugins/mash".into()), 0);
    // Only a directory marketplace is read in place, and only a path inside it.
    assert_ne!(known("github", "./plugins/mash".into()), 0);
    assert_ne!(known("directory", "../plugins/mash".into()), 0);
    assert_ne!(
        known(
            "directory",
            serde_json::json!({"source":"github","repo":"kojah/mash"})
        ),
        0
    );
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
        let mut report = collect(Some(&environment), &[Agent::Codex], &[]);
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
    let report = collect(Some(&environment), &[Agent::Codex], &[]);
    assert!(report.clients[0].findings.iter().any(|finding| matches!(
        finding.component,
        Component::Plugin
    ) && finding.state
        == State::Unverified));
}

/// An environment whose PATH has discovery-only `claude` and `codex` files.
#[cfg(unix)]
fn with_agent_clis(root: &Path) -> Environment {
    let mut environment = environment(root);
    let bin = root.join("bin");
    for agent in ["claude", "codex"] {
        write(&bin.join(agent), "discovery-only test fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(bin.join(agent), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
    }
    environment.path = bin.into_os_string();
    environment
}

#[cfg(unix)]
fn finding(client: &Client, component: Component) -> &Finding {
    client
        .findings
        .iter()
        .find(|finding| finding.component == component)
        .unwrap()
}

// The stand-in agent CLIs are Unix executables.
#[cfg(unix)]
#[test]
fn installation_expects_recorded_agents_whose_registration_disappeared() {
    let root = tempfile::tempdir().unwrap();
    let environment = with_agent_clis(root.path());
    std::fs::create_dir_all(&environment.claude).unwrap();
    // Without a record, a missing registration is valid CLI-only use.
    let unused = collect(Some(&environment), &[], &[]);
    assert!(!unused.agents_need_attention());
    let configuration = finding(&unused.clients[1], Component::Configuration);
    assert_eq!(
        configuration.remediation.as_deref(),
        Some("To require this agent, run `mash status --installation --agent claude-code`.")
    );
    let report = collect(Some(&environment), &[], &[Agent::ClaudeCode]);
    let client = &report.clients[1];
    assert!(client.expected && client.expectation == Expectation::Recorded);
    let mcp = finding(client, Component::Mcp);
    assert_eq!(mcp.state, State::Missing);
    assert!(mcp.message.contains("`mash init` connected this agent"));
    let remedy = mcp.remediation.as_deref().unwrap();
    assert!(
        remedy.starts_with("Run `claude mcp add --scope user mash -- "),
        "{remedy}"
    );
    let summary = &report.summary.clients[1];
    assert_eq!(summary.connection, Connection::None);
    assert_eq!(summary.next_action.as_deref(), Some(remedy));
    assert_eq!(summary.missing, [Component::Plugin, Component::Mcp]);
    assert!(report.agents_need_attention());
    // The unrecorded Codex client stays optional.
    assert_eq!(report.summary.clients[0].next_action, None);
}

#[cfg(unix)]
#[tokio::test]
async fn installation_names_a_missing_registered_executable_and_its_replacement() {
    let root = tempfile::tempdir().unwrap();
    let environment = with_agent_clis(root.path());
    write(
        &environment.codex.join("config.toml"),
        "[mcp_servers.mash]\ncommand = '/old/mash'\nargs = ['mcp']\n",
    );
    assert_eq!(
        clients::registration(&environment, Agent::Codex),
        Registration::Stale {
            command: "/old/mash".into()
        }
    );
    let mut report = collect(Some(&environment), &[], &[]);
    let mcp = finding(&report.clients[0], Component::Mcp);
    assert_eq!(mcp.state, State::Missing);
    assert!(mcp.message.contains("/old/mash"), "{}", mcp.message);
    let remedy = mcp.remediation.as_deref().unwrap();
    assert!(
        remedy.starts_with("Run `codex mcp remove mash && codex mcp add mash -- "),
        "{remedy}"
    );
    probe::check(&mut report.clients[0]).await;
    assert_eq!(
        finding(&report.clients[0], Component::Probe).message,
        "Skipped: the registered MCP executable does not exist."
    );
    // A registration that runs this binary is current; another binary is stale.
    let current = std::env::current_exe().unwrap();
    write(
        &environment.codex.join("config.toml"),
        &format!(
            "[mcp_servers.mash]\ncommand = {}\nargs = ['mcp']\n",
            toml::Value::String(current.to_string_lossy().into_owned())
        ),
    );
    assert_eq!(
        clients::registration(&environment, Agent::Codex),
        Registration::Current
    );
    write(
        &environment.codex.join("config.toml"),
        "[mcp_servers.mash]\ncommand = '/bin/sh'\nargs = ['mcp']\n",
    );
    assert_eq!(
        clients::registration(&environment, Agent::Codex),
        Registration::Stale {
            command: "/bin/sh".into()
        }
    );
    std::fs::remove_file(environment.codex.join("config.toml")).unwrap();
    assert_eq!(
        clients::registration(&environment, Agent::Codex),
        Registration::Unknown
    );
}

#[cfg(unix)]
#[test]
fn installation_mcp_without_the_plugin_is_mcp_only_with_the_plugin_commands() {
    let root = tempfile::tempdir().unwrap();
    let environment = with_agent_clis(root.path());
    write(
        &environment.home.join(".claude.json"),
        r#"{"mcpServers":{"mash":{"command":"/bin/true","args":["mcp"]}}}"#,
    );
    let report = collect(Some(&environment), &[], &[]);
    let summary = &report.summary.clients[1];
    assert_eq!(summary.connection, Connection::McpOnly);
    let action = summary.next_action.as_deref().unwrap();
    assert!(
        action.contains("The Mash MCP tools work without the plugin"),
        "{action}"
    );
    assert!(
        action.contains(
            "claude plugin marketplace add kojah/mash && claude plugin install mash@mash"
        ),
        "{action}"
    );
    assert!(!action.contains("mash init"), "{action}");
    assert!(report.agents_need_attention());
}
