---
name: mash-deployment
description: Deploy, redeploy or refresh Mash through its Ansible fleet playbooks.
---

# Mash deployment

After validated changes reach main, deploy unless the user says not to or the
change cannot affect nodes. Local gates are authoritative; hosted CI is optional.
Default targets are shared-api and xenia. Include Macs only when requested or
known available, and do not hold up Linux completion for unavailable Macs.

Use `bash tools/fleet.sh deploy shared-api xenia`. Ansible owns inventory,
availability, platform build reuse, API-first installation, migrations and
readiness. The private inventory defaults to ~/.config/mash/dev-inventory.yml;
set MASH_DEPLOY_INVENTORY to override it. Private reports default to
/home/james/scratch/mash-artifacts/dev-jobs; override with MASH_DEPLOY_LOGS.

Install the locked controller runtime with `uv sync --project deployment --locked`,
setting UV_PROJECT_ENVIRONMENT to a private artifact directory. Use that same
environment for fleet commands. The launcher takes a fleet lock, bounds execution
and invokes registered playbooks directly through ansible-runner. Never bypass
Ansible with invented per-host rollout commands. Read private reports after the
command ends; unknown/unverified outcomes fail closed. Skipped means not installed.

Linux builds use tools/build-dev-linux.sh through agent-test, persistent targets
and disk-backed temp paths. Reuse successful gates only for identical source,
toolchain and effective build inputs; always stamp the current release. Build
once per platform. API installation uses authenticated readiness, rollout locking,
atomic release pointers and rollback; identical healthy releases stay running.

Ordinary deployment never ingests history or downloads models. Each release
opens only its silver schema: required upgrades export with the installed binary,
retain private sensitive backups, import with the new binary keeping identity,
then explicitly sync to re-ingest stale source interpretation, rebuild views and
publish. Never add schema aliases or
alter provider files. Missing query models require explicit models install.

For explicit refresh use `bash tools/fleet.sh sync xenia` or
`bash tools/fleet.sh local-views xenia`; the latter stays local. Fresh readiness
uses `bash tools/fleet.sh check shared-api xenia`. The Rust RMCP probe checks the
installed build/contract, local/shared readiness and bounded lexical/available
semantic searches; successful status with ready=false does not pass.

Report the deployed commit, each target's installed/skipped/failed outcome,
local/shared readiness and follow-ups. Existing Mash MCP processes require
reconnection after installing a new binary. Reload updated plugin instructions;
use live help for parameters. See deployment/README.md for controller setup.
