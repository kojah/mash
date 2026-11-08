# Mash fleet deployment

Ansible owns fleet selection, platform builds, API-first installation, required
schema migrations and readiness checks. The standalone Rust RMCP probe checks
the installed history MCP and performs explicitly requested refreshes.

Builds produce separate `mash` and `mash-server` executables at the same version.
Install `mash-server` on API hosts and use
`mash-server --server-config /absolute/path/to/server.toml` in the service unit.
History nodes and MCP launchers continue to use `mash`; `mash dash` is unchanged.
Do not point both roles at the same installed executable. Existing units using
`mash serve` must be updated during their next planned deployment. Merely
installing the new CLI does not replace or restart a running server.
For an existing systemd service that still runs `mash serve`, the trusted
`tools/restart-systemd-api.sh` installer accepts `--previous-binary PATH` and
`--next-unit PATH`. Set its destination to the separate `mash-server` path and
provide a complete private unit file whose `ExecStart` uses that path with
`--server-config`. Preflight checks the running old executable; installation
stops the service before replacing its unit and reloading systemd. A failure
before activation restores the previous unit and executable. After activation,
the pending marker requires forward recovery. Inspect existing unit drop-ins
before preparing the replacement, because they can override its settings.
Once the split is verified, ordinary deployments use the new server path.
The macOS builder copies its CLI to `OUTPUT` and its server to `OUTPUT-server`,
each with its own checksum and revision file; Linux keeps both in the release
target directory. Release archives and public installers include both binaries.

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
bash tools/fleet.sh replay-views shared-api
```

`replay-views` invokes the API target's explicit `mash_replay_views` commands,
between its existing authenticated `mash_verify` checks. Configure that hook to
restart the owning service with the installed `mash-server --replay-views`,
wait for replay and certification, and restore normal startup settings. This
operation builds and installs nothing, does not upgrade silver, and accepts only
API targets. Replay re-embeds from shared silver and may keep the API unavailable
for a long time; retain private progress logs and verify freshness afterward.

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

For a systemd-owned API, configure the trusted inventory commands to call
`tools/restart-systemd-api.sh` with `preflight`, `install`, or `check`. Its remaining
arguments are the user service unit, candidate artifact, installed binary,
server TOML, environment file, release pointer, API URL, token variable name,
private backup root, and shared-silver directory. The unit must run that binary
with the supplied configuration and environment, and own every server writer to
that directory. Do not substitute the legacy PID-based installer.
Use `--next-environment PATH` to switch a prepared environment file while the
service is stopped, for example to select a fresh Qdrant instance. The old file
is retained and restored with the binary on pre-activation failure. After
activation, keep both the new environment and new state during forward recovery.

The upgrade policy prefers tested in-place SQLite migrations and preservation of
valid Qdrant data. The current schema-62 tools do not implement migrations yet;
the following retirement procedure is a legacy fallback, not the default design
for future schema changes. See [migration design](../docs/internal/database-migrations.md).
Inspect any active rollout and recovery markers before changing its plan.

Unsupported shared schema changes handled by these tools require the explicit
`--retire-incompatible-silver` option and a
verified republish plan for every affected node. While the service is stopped,
the systemd installer retains the **entire** old shared-silver directory,
including its transfer catalogue and spool, under the private backup root on
the same filesystem. All nodes then need to republish; shared search is incomplete
until they do. Verify local export/import compatibility and retained histories
before starting this operation. Archive record versions can change independently
of the SQLite schema; the local upgrade tool does not convert arbitrary archives.

The systemd installer restores old state only if failure occurs before candidate
activation. Once activation starts, the new process may have accepted writes even
if readiness fails, so automatic rollback is forbidden. Failures leave a
`POINTER.systemd-pending` marker identifying the retained directory and phase.
Inspect and recover that release forward before removing the marker; rerunning
deployment does not bypass it. Process-crash recovery is conservative and manual;
the script does not promise power-loss atomicity or control separately launched
servers. See the `SystemdCutover` model and disposable shell fixtures.

The legacy process installer uses the same activation boundary: it can restore
the previous process before activation, but requires forward recovery afterward.
Its marker is `POINTER.deploy-pending`; the retained directory records the phase,
candidate PID, previous release record and process logs. This prevents an old
binary from reopening a database the candidate may already have migrated.
See `LegacyActivation` for the bounded model and its limitations.

The probe verifies build and contract identity, query readiness, shared freshness
and bounded lexical/available semantic searches. Restart existing Mash MCP
connections after upgrading the binary, and reload updated plugin instructions.
Canonical exports and Runner logs remain private.

The existing export/import upgrade path requires a stopped node: stop index watchers and agent MCP
sessions and keep them stopped through cutover. The upgrader checks open database
files with `lsof` before export and again before cutover; this is not a fence
against newly started processes. Targets need `jq` and `lsof`. It validates a
portable import in a private staging directory before moving any active files,
keeps originals under `retired/`, and installs attachments before the new database.
Publication identity is preserved only when the export contains it.

Select an exporter for the node's actual current schema, rather than retaining
an override from an earlier migration. Verify the installed binary's version
and read-only status against the intended database. When the API and local CLI
share an installed binary path, preserve and verify an immutable exporter copy
before the API-first replacement, then pass that copy as `OLD_BINARY` to the
local upgrader. The Macs' installed binaries live on separate hosts and remain
available until their own local upgrade step. A successful executable-access
check alone does not establish exporter compatibility.

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
