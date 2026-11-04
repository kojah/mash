# Mash fleet deployment

Ansible owns fleet selection, platform builds, API-first installation, required
schema migrations and readiness checks. The standalone Rust RMCP probe checks
the installed history MCP and performs explicitly requested refreshes.

Canonical runners select and synchronize the locked controller runtime in
`/home/james/scratch/mash-artifacts/ansible-venv`. Set `UV_PROJECT_ENVIRONMENT`
to override that private location; no manual setup is required. Copy
`inventory.example.yml` into a private inventory; node/server settings remain TOML.
Set `MASH_DEPLOY_INVENTORY` to that inventory (default
`~/.config/mash/dev-inventory.yml`) and `MASH_DEPLOY_LOGS` to private reports
(default `/home/james/scratch/mash-artifacts/dev-jobs`).

```bash
bash tools/fleet.sh preflight shared-api xenia
bash tools/fleet.sh deploy shared-api xenia
bash tools/fleet.sh check shared-api xenia
bash tools/fleet.sh sync xenia
bash tools/fleet.sh local-views xenia
```

Commit and push focused, validated changes without deploying each one. Deploy
at an explicitly requested release or agreed stable-batch milestone. Use
`make iterate FOCUS='test(changed_behavior)'` while developing; it runs formatting,
Clippy and selected regressions without broad acceptance or a release build.
`make validate-dev` validates a stable batch without building a release artifact.
Source/toolchain/input receipts survive commit identity and release-stamp changes;
the release build still stamps the current commit.

For a release, run preflight before broader release preparation, validate the
stable batch, commit/push, then deploy. Deployment repeats the fresh preflight
and reuses eligible validation evidence before stamping the release binary.

Both `preflight` and `deploy` check prerequisites before compilation: available
required APIs, configured lifecycle/release identity checks, authentication and
install access. All targets declare read-only `mash_deploy_preflight` commands.
The legacy API installer uses `check-dev-api.sh --deployment` for identity,
authentication and writable release paths, plus a configured service-manager
check. The example rejects an active supervised API when using the PID installer.
Preflight success reports `deployed: false`.
Failure blocks all builds and installations. Existing service-manager/PID
mismatches must be repaired through the owning lifecycle tooling.

The launcher invokes the existing locked Runner runtime, serializes jobs and
prints per-target outcomes. Inspect its private reports for readiness and errors.
Unknown targets and missing API dependencies fail; offline targets are skipped.
Default rollout selects shared-api and xenia. Include Macs only when requested
or known available. Builds run once per selected platform.

An API installer that preserves the existing shared silver root must include
`bash tools/check-shared-silver.sh app/storage/schema.rs /absolute/shared-silver`
in its read-only deployment preflight. This checks every immediate group/node
SQLite file against the source release's schema before compilation; unknown or
unreadable schemas fail. A successful lifecycle/authentication check alone does
not establish schema compatibility. This check performs no retirement or
upgrade: crossing schemas requires a separate plan preserving node histories,
transfer receipts and accepted writes. Keep ordinary installed-service health
checks separate so the current release remains checkable during preparation.
Repeat the compatibility check at the install boundary before stopping the
service; preflight is an observation, not a fence against later source/state
changes. A planned cross-schema procedure must supply its own tested cutover.

Ordinary deployment performs no ingestion or model downloads. An actual silver
upgrade preserves a private export and runs one explicit sync to re-ingest stale
source interpretation, rebuild views and publish. Missing models require explicit installation. The API helper
locks rollouts, rolls back failed releases and keeps identical healthy releases.

The probe verifies build and contract identity, query readiness, shared freshness
and bounded lexical/available semantic searches. Restart existing Mash MCP
connections after upgrading the binary, and reload updated plugin instructions.
Canonical exports and Runner logs remain private.

Local schema upgrades require a quiescent node: stop index watchers and agent MCP
sessions and keep them stopped through cutover. The upgrader checks open database
files with `lsof` before export and again before cutover; this is not a fence
against newly started processes. Targets need `jq` and `lsof`. It validates a
portable import in a private staging directory before moving any active files,
keeps originals under `retired/`, and installs attachments before the new database.
Publication identity is preserved only when the export contains it.

An interrupted cutover leaves `DATABASE.upgrade-in-progress/backup`, pointing to
the private recovery directory. Subsequent deployment refuses to proceed even if
the active database is absent. Inspect the stopped upgrade and all retained files,
then recover the complete database/WAL/attachments together before clearing the
marker. Do not remove the marker to force a retry over an unknown intermediate state.
Successful moves are not a power-loss durability guarantee.

Schema-45 releases that export format 1 need the intermediate exporter at
`a88c99344f6d03bec4147ff57915ad5a08bdc3d6`, which reads schema 45 without modifying
it and exports portable format 2. Build this separately, verify its artifact,
and select its remote executable as the optional fifth argument to
`tools/upgrade-remote-index.sh` in the private Ansible inventory. The fourth
argument is the explicit database path (or empty for the platform default).
This selects an exporter, not an alternative installed release. Retain the old
database and export until the current importer and explicit refresh pass.

Local gates are authoritative; hosted CI is optional. Validation includes probe
unit tests and real Ansible fixtures through `tools/test-ansible.sh`, plus fleet
lock, API rollback and build-gate regression scripts.
# Full-history refresh after upgrades

Schema upgrades run the refresh probe with `--all-history`, so sources outside
the ordinary age window are reparsed before readiness is checked. For an explicit
repair of an already upgraded node, set `mash_refresh_all_history: true` in its
private inventory and run the normal `fleet.sh sync` operation. Remove that setting
after repair to restore the ordinary refresh window. Checks and local-view replay
never use this option.
