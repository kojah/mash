---
name: mash-deployment
description: Use after every change to this repository is committed and pushed, and whenever asked to deploy, redeploy, roll out or refresh Mash on the development fleet (shared API, xenia, dev-macbook, personal-macbook).
---

# Mash deployment

Redeploy by default after every change that reaches `main`: once validation
passes and the commit is pushed, deploy it, unless the user says not to or the
change cannot affect any node (for example documentation or research only).
Say so in the final report either way.

The fleet and its trusted commands live in `~/.config/mash/dev.toml`. Run only
those commands, in their configured order; do not improvise other deployment
steps. The development MCP's source is `crates/mash-dev`.

## Targets and order

Targets are explicit; there is no implicit fleet-wide deploy. Deploy API
targets before the history nodes that require them:

1. `shared-api`: the shared HTTP service and its Qdrant gold
2. `xenia`: this machine's history node
3. `dev-macbook`, `personal-macbook`: remote history nodes over SSH

A failed API deployment blocks its dependent nodes. A Mac that does not answer
`ssh -o BatchMode=yes -o ConnectTimeout=10 HOST true` is skipped and reported;
deploy it when it is back.

## Preferred path: the development MCP

Use `mash-dev-mcp` (`mash_dev` in `.mcp.json`):

1. `deploy` with `{"nodes": [...]}` in the order above. It returns a job ID.
2. Poll `deployment_status` with `{"job_id": N}` until every node finishes.
3. Then refresh (below) when needed.

It builds once per platform, installs, and checks readiness. It never indexes,
installs models or restarts itself. Existing Mash MCP clients must reconnect
after their binary is replaced. Do not restart the development MCP or replace
its configuration while one of its jobs is running.

The MCP fails to start when `~/.local/bin` is not on the session's `PATH`; the
binary is `~/.local/bin/mash-dev-mcp`. Rebuild it (`mash-dev` crate) when
`crates/mash-dev` changed since it was installed.

## Fallback: the same configured commands

When the MCP is unavailable, run `dev.toml`'s commands yourself, unchanged:

1. **Build gate** (`[builds].linux`): `cargo fmt --check`,
   `tools/test-dev-api.sh`, `cargo test-clif --locked --profile ci`, and the
   release build. Run heavy steps through `/home/james/.local/bin/agent-test`.
   Never deploy a build whose gate failed.
2. **macOS** (`[builds].darwin`): `tools/build-dev-macos.sh` builds on
   `dev-macbook` and copies back the artifact for both Macs.
3. **Install** each target's `install` commands, then its `verify` commands.

## Silver schema changes

A release opens only its own silver schema (`mash --version` reports
`silver_schema`). The configured steps already handle a change:

- `restart-dev-api.sh --retire-incompatible-silver` moves node databases the
  new server cannot open into the deployment log and lists those nodes under
  `republish`; any failure restores them and restarts the previous binary.
- `upgrade-local-index.sh` (xenia) and `upgrade-remote-index.sh` (Macs) export
  with the installed release and import with the new one, keeping identity.

After a schema change, every listed node must republish and rebuild local gold.

## Refresh

Deployment and refresh stay separate. Refresh after deploying when:

- the API reported nodes to `republish`, or
- a node's local index was upgraded, or status reports it not ready.

Use `refresh_index` with `{"nodes": [...], "mode": "local-gold"}` to rebuild
local gold, then `{"mode": "sync"}` to ingest and republish. Nodes refresh
one at a time. Without the MCP, run the node's `mash index` (with
`--rebuild-local-gold` first when gold is missing). Never download models
here; a missing model needs `mash models install` from the user.

## Report

State the deployed commit, each target's outcome (installed, skipped with the
reason, or failed with the failing step), anything awaiting refresh, and
`status --shared` readiness. Keep raw command logs under
`/home/james/scratch/mash-artifacts/dev-jobs`; do not paste them.

## Environment notes

- Deployment is an outward-facing action; Claude Code's auto mode may block it
  until the user adds a permission rule. If blocked, stop and say exactly what
  would run.
- On Xenia, Qdrant collection creation can stall for tens of seconds while the
  disk is busy (build writeback, the `codebase-memory` daemon). An API
  readiness timeout right after a build is more often that than a code fault;
  check IO pressure (`/proc/pressure/io`) before retrying.
