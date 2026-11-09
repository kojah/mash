//! Uses the real controller and Ansible against an isolated local API fixture.
#[cfg(unix)]
#[test]
#[ignore = "requires the locked Ansible controller environment"]
fn canonical_inventory_drives_real_cli_and_ansible() {
    use std::process::Command;
    let root = tempfile::tempdir().unwrap();
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let binary = env!("CARGO_BIN_EXE_mash-fleet");
    let marker = root.path().join("verified");
    let inventory: mash_fleet::inventory::Inventory = serde_json::from_value(serde_json::json!({
        "version": 1,
        "controller": {"node": "fixture", "repository": repository, "binary": binary, "jobs": root.path().join("jobs"), "allow_actions": ["status"]},
        "machines": [{"id": "fixture", "platform": "linux", "roles": ["api"], "transport": {"kind": "local"},
            "deployment": {"connection_check": ["/usr/bin/true"], "verify": [["/usr/bin/touch", marker]]}}]
    })).unwrap();
    let config = root.path().join("fleet.toml");
    std::fs::write(&config, toml::to_string(&inventory).unwrap()).unwrap();
    let output = Command::new(binary)
        .arg("--fleet-config")
        .arg(&config)
        .args(["status", "--all"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(marker.exists());
    assert!(String::from_utf8_lossy(&output.stdout).contains("succeeded"));
    let requests: Vec<_> = std::fs::read_dir(&inventory.controller.jobs)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .collect();
    assert_eq!(requests.len(), 1);
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(requests[0].path().join("run/fixture.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["state"], "succeeded");
    std::fs::remove_file(&marker).unwrap();
    let rejected = Command::new(binary)
        .arg("--fleet-config")
        .arg(&config)
        .args(["status", "unknown"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(!marker.exists());
    let disabled = Command::new(binary)
        .arg("--fleet-config")
        .arg(&config)
        .args(["deploy", "--all"])
        .output()
        .unwrap();
    assert!(!disabled.status.success());
    assert!(!marker.exists());
}
