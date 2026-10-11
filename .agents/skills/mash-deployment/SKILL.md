---
name: mash-deployment
description: Deploy, redeploy, roll out or refresh Mash through its Ansible fleet playbooks and thin development MCP.
---

# Mash deployment

After validated changes reach main, deploy by default unless the user says not to
or the change cannot affect nodes (for example documentation only).

Ansible owns inventory, availability, build reuse, API-first installation,
required migrations and readiness checks. The development MCP is a thin adapter
to registered ansible-runner playbooks; it never implements fleet policy.

## Configuration and runtime

Trusted settings live in ~/.config/mash/dev.toml: repository, private logs,
inventory, allowed target names, runner argv and registered deploy/refresh/check
playbook paths. Inventory lives in ~/.config/mash/dev-inventory.yml. Examples are
deployment/dev.example.toml and deployment/inventory.example.yml. Mash node and
HTTP server settings remain strict TOML; Ansible inventory describes deployment.

Use the locked deployment runtime (ansible-core and ansible-runner):
uv sync --project deployment --locked. Store the environment under the private
artifact root via UV_PROJECT_ENVIRONMENT, and use the same environment for jobs.
Do not replace dev configuration or restart mash-dev-mcp while its job is active.
Install updated dev binaries only after checking for active jobs.

## Run and observe

1. Call mash_dev deploy with explicit targets, including required API targets.
   Ansible installs APIs before history regardless of selection order.
2. Poll deployment_status with the returned job_id to a terminal state.
   It reports safe task names, per-target states and redacted MCP probe results;
   raw Runner events and command logs stay private.
3. Check each target: skipped is not installed or query-ready. A failed API
   blocks dependent history installs. Unknown/unverified results fail closed.

The fleet is shared-api, xenia, dev-macbook and personal-macbook.
Fresh SSH probes disable control-master reuse and bound connection/keepalive
timeouts. Offline Macs are skipped and reported. A reachable Mac can build the
single macOS artifact reused for all available Macs.

## Build and install

Run only the inventory's trusted helper commands. Linux uses
tools/build-dev-linux.sh through agent-test with the configured persistent target
and disk-backed test temp paths. It verifies generated guidance, formatting,
deployment regressions, Rust tests, Clippy and doctests, then builds Mash and the
development MCP/probe. Successful gates are reusable only for identical source,
toolchain and effective build inputs; release identity is always current.
Changes during validation/build fail closed. Builds run once per selected platform.

API rollout uses authenticated readiness, a cross-process lock, atomic release
pointer and rollback. An identical healthy binary/config/environment is left
running; explicit replay/activation still executes. History installs are atomic.

The Rust mash-dev-probe uses the official RMCP client for live tool/build/contract
checks, local/shared readiness and bounded lexical/available semantic smoke
searches. A successful status RPC with ready=false does not pass readiness.
It does not implement indexing, schema or Qdrant logic.

## Required migrations and explicit refresh

Each release opens only its own silver schema. The upgrade helpers export with
the installed binary, retain private backups, then import with the new binary
keeping identity. Ansible updates legacy gold section headers in retained TOML
before upgrade. Never add compatibility aliases or alter upstream provider files.

An actual silver upgrade triggers a separate local-views rebuild and sync to
complete that required migration. Ordinary deployment never indexes history or
downloads models. Query-model failures require explicit models install, not
repeated indexing. Preserve export/backup privacy: canonical history is sensitive.

For ordinary user-requested refresh, call refresh_index with explicit history
targets and mode sync or local-views. Ansible serializes nodes. Local-views does
not contact the shared API; sync checks prerequisites and republishes.
For fresh read-only readiness use deployment_status with explicit nodes and no
job_id. These playbooks do not deploy or repair indexes.

## Fallback and report

When the dev MCP is unavailable, use the registered runner command and playbook
with the same inventory and explicit selections. Keep Runner's private input,
events and reports under the configured artifact root. Do not invent per-host
shell rollout sequences or bypass gates.

Report the deployed commit, every target's installed/skipped/failed outcome,
local/shared readiness, required follow-ups and client reconnection needs.
Existing MCP processes keep their old executable; reload updated skills and
reconnect history clients after a breaking upgrade.
