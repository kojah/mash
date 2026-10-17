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

Ordinary deployment performs no ingestion or model downloads. An actual silver
upgrade preserves a private export and runs one explicit sync to re-ingest stale
source interpretation, rebuild views and publish. Missing models require explicit installation. The API helper
locks rollouts, rolls back failed releases and keeps identical healthy releases.

The probe verifies build and contract identity, query readiness, shared freshness
and bounded lexical/available semantic searches. Restart existing Mash MCP
connections after upgrading the binary, and reload updated plugin instructions.
Canonical exports and Runner logs remain private.

Local gates are authoritative; hosted CI is optional. Validation includes probe
unit tests and real Ansible fixtures through `tools/test-ansible.sh`, plus fleet
lock, API rollback and build-gate regression scripts.
