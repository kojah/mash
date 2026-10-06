# Mash development MCP

`mash-dev-mcp` is a separate, lightweight Rust program using the official rmcp
SDK. It has no dependency on Mash's inference/search implementation and keeps
running while the installed Mash binary is replaced or the API is restarted.
It exposes three project development tools:

| Tool | Behavior |
| --- | --- |
| `deploy(nodes)` | Run configured validation/build steps once per build key, install selected nodes, then call each installed Mash MCP's `status`. Never calls `index`. |
| `refresh_index(nodes, mode)` | Call each node's existing MCP `index`, then `status`. Modes: `sync`, `local-gold`, `shared-gold`. |
| `deployment_status(job_id)` | Poll per-node phase, latest MCP progress notification (including counts/ETA metadata), result and error. Without an ID, specify nodes for fresh read-only readiness checks. |

Jobs return promptly and run sequentially, including shared publications. Only
one dev job runs at a time. One failed node does not prevent independent nodes
from running; a failed build key is reused as a failure, never installed. The
last 32 jobs are retained for this dev MCP session. Restarting **this** process
loses job history and interrupts its work; restart only after jobs finish. A
timeout or interrupted upload may have committed data: check readiness before
retrying. Replacing Mash does not restart already running history MCP sessions;
reconnect those clients to adopt the new binary.

Successful deployment means configured commands and readiness requests completed,
not that every search representation is ready. Inspect the returned Mash status
(`ready`, artifacts, query models, silver, shared freshness) and index results for
pending/incomplete work. Commands are trusted TOML operator configuration, not
tool arguments. Do not put credentials in command arguments; existing wrappers
can load the node's private environment. Raw workflow stdout/stderr stays in
private per-session/per-job logs rather than MCP responses. MCP results retain
Mash's output redaction. Protect the configuration and log root like executable
code. There is no implicit shell expansion.

Targets explicitly declare `kind = "history"` or `kind = "api"`. History targets
have an MCP command; API targets have read-only `verify` commands instead. A
history target can declare `requires = ["shared-api"]`. Deployment requests must
include that API target **before** the history target. Failed API deployment or
verification prevents dependent installs. Refreshes check the required API first,
without deploying it; API targets themselves cannot be indexed. Independent
targets continue after failures. This is a fixed API prerequisite, not a generic
dependency scheduler.

`tools/build-dev-macos.sh` freezes `git archive HEAD`, builds once on the configured
native Mac using its persistent target, and retrieves one Apple Silicon artifact.
It synchronizes that frozen archive into its owned `build-source` directory using
checksums, retaining unchanged source mtimes and a stable Cargo source path. Cargo
still checks the build inputs on every invocation. An existing local artifact
with the same SHA-256 avoids another download. Serialize builds sharing this root;
the directory is owned by the deployment helper, including removal of old files.
`tools/install-dev-remote.sh` verifies its checksum and atomically replaces the
installed binary, retaining the previous binary. Both scripts restrict remote
host/path syntax before constructing SSH commands. Their SSH access is deployment
transport; product queries still use the HTTP API.

On the Linux API host, `tools/restart-dev-api.sh` verifies the tracked process and
binary hash, preserves a frozen release, gracefully replaces the existing process,
checks authenticated freshness/capabilities and unauthenticated rejection, and
requires publication identities to remain unchanged. It updates the release
pointer atomically after checks. A failed candidate restarts the previous binary
and updates its PID. It neither installs a system service nor changes Qdrant data
or server configuration. Stop publications during this storage-layout rollout;
rollback to an older API is unsafe once upgraded nodes publish a newer layout.
For an intentional shared query/planner migration, append `--activate-query` to
the configured `restart-dev-api.sh` command. It reaches only the candidate API;
rollback starts the previous binary without reapplying activation. Backend graph
rebuilds can continue after HTTP readiness succeeds, so inspect optimizer
progress and actual native search paths separately before measuring completion.
`tools/check-dev-api.sh` supplies the corresponding read-only prerequisite.

## Schema changes

`sync` uses the same core indexing path as the product MCP: it applies supported
local SQLite migrations, ingests changed sources, and updates configured search
artifacts. Gold-only modes require their existing silver certificates and are
not general schema/source migration commands. Deploy code first when a migration
needs the new implementation. Shared storage/API migrations are owned by the
configured deployment workflow; refresh does not upgrade Qdrant or restart the
API. For the partitioned-publication transition, upgrade the API before nodes
publish. See [redeployment.md](redeployment.md).

## Setup

Build independently during development:

```sh
cargo build-clif -p mash-dev --bin mash-dev-mcp
```

Copy [mash-dev.example.toml](mash-dev.example.toml) to a private local configuration,
using existing deployment/build commands and node MCP wrappers. Artifacts sharing
a build key must be compatible with all its nodes; configured install commands
should atomically replace validated artifacts. `tools/install-dev-binary.sh`
implements local atomic replacement without indexing. A production configuration
should use validated release artifacts rather than a Cranelift development build.
API restart commands can be included explicitly in an install workflow; keep the
dev binary separate from binaries that workflow replaces. The existing GitHub
fleet workflow remains unchanged.

With no arguments, the server reads `MASH_DEV_CONFIG`, or defaults to
`~/.config/mash/dev.toml`. The project Codex/Claude registrations use this default
and expect `mash-dev-mcp` on PATH. Reconnect the client after registration changes.
Codex's stdio command/args settings follow the
[official MCP documentation](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).

Codex project config:

```toml
[mcp_servers.mash_dev]
command = "/absolute/path/to/mash-dev-mcp"
args = ["--config", "/absolute/path/to/private/dev.toml"]
startup_timeout_sec = 60
tool_timeout_sec = 120
```

Claude Code project `.mcp.json` entry:

```json
"mash_dev": {
  "type": "stdio",
  "command": "/absolute/path/to/mash-dev-mcp",
  "args": ["--config", "/absolute/path/to/private/dev.toml"]
}
```

The dev MCP is project-specific and separate from the installed history plugin.
Neither setup starts watching or indexing. Names must match configured targets;
all tools require explicit selections, avoiding accidental fleet-wide operations.
