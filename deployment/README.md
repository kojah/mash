# Mash fleet deployment

Ansible owns fleet selection, platform builds, API-first installation, required
schema migrations and readiness checks. The standalone Rust RMCP probe checks
the installed history MCP and performs explicitly requested refreshes.

Install the controller runtime with `uv sync --project deployment --locked`,
setting `UV_PROJECT_ENVIRONMENT` to a private directory outside Git. Copy
`inventory.example.yml` into a private inventory; node/server settings remain TOML.
Set `MASH_DEPLOY_INVENTORY` to that inventory (default
`~/.config/mash/dev-inventory.yml`) and `MASH_DEPLOY_LOGS` to private reports
(default `/home/james/scratch/mash-artifacts/dev-jobs`).

```bash
bash tools/fleet.sh deploy shared-api xenia
bash tools/fleet.sh check shared-api xenia
bash tools/fleet.sh sync xenia
bash tools/fleet.sh local-views xenia
```

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
