# Mash fleet deployment

Ansible owns fleet execution. The development MCP only launches registered
ansible-runner jobs and reads their safe task/probe reports. History parsing,
indexing, schemas and Qdrant remain in Mash; the standalone Rust RMCP probe calls
the installed history MCP for explicit refresh or read-only readiness.

Install the pinned controller runtime with `uv sync --project deployment --locked`.
Use `UV_PROJECT_ENVIRONMENT` to keep its environment under your private artifact
root. The locked runtime is plain ansible-core/ansible-runner; no resident fleet
service is installed. Prepare `dev.toml` from `dev.example.toml` and a private
inventory from `inventory.example.yml`. The old embedded build/node TOML tables
are removed. Mash node/server configuration stays strict TOML.

The inventory registers trusted argv helpers, node dependencies, platform builds
and credential-loading MCP launchers. The API is deployed and authenticated before
dependent history nodes. Fresh SSH checks avoid stale control masters. Offline
targets are reported as skipped. An available Mac builds one macOS artifact reused
across the available Macs; Linux gates reuse identical validated inputs.

Playbooks reject unknown targets, missing API dependencies and API refresh requests.
Source revision changes between build and install fail closed. The API helper
serializes rollouts, rolls back failed releases, and skips identical healthy
restarts. A fleet lock prevents overlapping jobs from different MCP sessions.

Ordinary deployment performs no ingestion. If the silver upgrade helper reports
an actual upgrade, the playbook separately rebuilds local views and synchronizes
that node to complete the required migration. Retained TOML gold section headers
are renamed before upgrade; backups stay private. Missing models require an
explicit model installation, never automatic downloads or repeated indexing.

The probe verifies the installed build and generated contract, query readiness,
the node's shared publication, and bounded lexical/available semantic searches.
Local-views refresh checks local lexical retrieval without shared API access.
Its callbacks write atomic progress reports; Runner artifacts provide task names.
Raw logs/events never become MCP response text. Canonical exports remain sensitive.

Use `deploy` with explicit targets, `refresh_index` with explicit history targets
and `sync`/`local-views`, or `deployment_status` for a job ID/fresh checks. Poll to
a terminal state and inspect every node, including skipped targets. Existing
history MCP fronts automatically adopt workers with identical tool/storage
contracts after active work drains. Help/status expose worker generation and
rejected updates. Reconnect once for sessions predating the stable front, or
when tool/storage contracts change; reload updated skills when needed. Do not restart/reconfigure the development
MCP while one of its jobs is active.

Validation includes Rust wrapper/probe tests and real Ansible fixture runs through
`tools/test-ansible.sh`, plus fleet-lock and build-gate regression scripts. Keep
controller environments and all job data outside Git.

Local gates are the normal release validation; GitHub Actions is optional and
generally unavailable. Do not wait for hosted CI or treat its availability as a
release prerequisite. Default rollout selects `shared-api` and `xenia`.
Both Macs are normally unavailable and are optional targets, selected only when
explicitly requested or known reachable. Their availability does not block Linux
validation, deployment or completion.
