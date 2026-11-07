---
name: mash-deployment
description: Deploy, redeploy or refresh Mash through its Ansible fleet playbooks.
---

# Mash deployment

Deploy when explicitly requested or at an agreed stable release milestone.
A commit or push alone does not request deployment; batch ordinary small fixes.
Local gates are authoritative; hosted CI is optional.
Default targets are shared-api and xenia. Include Macs only when requested or
known available, and do not hold up Linux completion for unavailable Macs.

Use `bash tools/fleet.sh preflight shared-api xenia` for a read-only release
preflight. `deploy` includes the same checks before any build or installation.
All targets declare read-only `mash_deploy_preflight` commands for install access
and lifecycle. Legacy API installation uses `check-dev-api.sh --deployment` to
verify release identity, authentication and write access; configure a separate
service-manager check matching the chosen installer (see the example inventory).
Unavailable required APIs fail preflight. A stale PID record is a blocked release,
not permission to kill a supervised process through the legacy installer.

Use `bash tools/fleet.sh deploy shared-api xenia` for the actual release. Ansible owns inventory,
availability, platform build reuse, API-first installation, migrations and
readiness. The private inventory defaults to ~/.config/mash/dev-inventory.yml;
set MASH_DEPLOY_INVENTORY to override it. Private reports default to
/home/james/scratch/mash-artifacts/dev-jobs; override with MASH_DEPLOY_LOGS.

The canonical runners select and synchronize the locked controller runtime in
the private default environment. Set `UV_PROJECT_ENVIRONMENT` only to override
that location; callers need no manual setup step. The launcher takes a fleet lock, bounds execution
and invokes registered playbooks directly through ansible-runner. Never bypass
Ansible with invented per-host rollout commands. Read private reports after the
command ends; unknown/unverified outcomes fail closed. Skipped means not installed.

Linux builds use tools/build-dev-linux.sh through agent-test, persistent targets
and disk-backed temp paths. Reuse successful gates only for identical source,
toolchain and effective validation inputs. `make validate-dev` can establish the
receipt before committing without building a release. Commit identity and the
release-stamp override do not invalidate validation; always stamp the current release. Build
once per platform. API installation uses authenticated readiness, rollout locking,
atomic release pointers and rollback; identical healthy releases stay running.

Ordinary deployment never ingests history or downloads models. Prefer tested
in-place SQLite migrations for explicitly supported starting versions, shared
by local and server databases. Stop writes and exclude stale runtimes during
upgrades. Preserve valid Qdrant data independently of SQLite version changes;
refresh only data affected by a source, model or search-contract change.
Migration support is not implemented in schema-62 binaries: retain the existing
export/import recovery path for those binaries and unsupported legacy transitions,
with private backups and verified archive compatibility. Never bypass version
guards or alter provider files. Inspect an ongoing rollout before changing its
upgrade plan. Missing query models require explicit models install.

For explicit refresh use `bash tools/fleet.sh sync xenia` or
`bash tools/fleet.sh local-views xenia`; the latter stays local. Fresh readiness
uses `bash tools/fleet.sh check shared-api xenia`. The Rust RMCP probe checks the
installed build/contract, local/shared readiness and bounded lexical/available
semantic searches; successful status with ready=false does not pass.

Report the deployed commit, each target's installed/skipped/failed outcome,
local/shared readiness and follow-ups. Existing Mash MCP processes require
reconnection after installing a new binary. Reload updated plugin instructions;
use live help for parameters. See deployment/README.md for controller setup.
