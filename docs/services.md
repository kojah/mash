# Inspect and manage Mash services

Open **Services** in the dashboard menu. With ordinary `mash dash`, you can
inspect the local SQLite schema, registered MCP and watcher processes, the
dashboard process, and configured Qdrant/shared-server endpoints. Checks never
start indexing, migrate SQLite or rebuild Qdrant.

The front page shows a status dot for each reported machine. Open a machine,
then a service, to see its diagnostics and schema compatibility. **Check for
updates** refreshes the report. There is no timer-based polling or page reload.
Dates tell you when a check was made; an old green result is not a continuous
health guarantee.

## Enable authenticated service access

Create a separate, private TOML file. This example assumes the named systemd
user unit already exists; Mash does not install it.

```toml
node = "xenia"
token_env = "MASH_SERVICES_TOKEN"

[[services]]
id = "watcher"
name = "Index watcher"
kind = "watcher"
manager = "systemd_user"
unit = "mash-watch.service"
allow_control = true
```

Set `MASH_SERVICES_TOKEN` in the dashboard process's environment to a distinct,
random secret of at least 32 printable ASCII characters. Use your secret manager
or a private environment file. Do not put secrets in URLs, Git or shared logs.

```sh
mash dash --services-config /private/mash-services.toml
```

Open **Services** and enter that token to unlock it. The browser keeps it only
in memory for the current tab; **Lock services** clears it and its cached service
reports. This authentication protects the service APIs. Existing history routes
retain their local dashboard access rules.

For remote access, bind explicitly with `--listen ADDRESS:PORT`. Use a private
encrypted connection such as Tailscale, or HTTPS through a correctly configured
proxy. A plain HTTP connection on an untrusted network would expose the token.
The existing Host check still applies: the request Host must match the dashboard's
bound address. Cross-origin browser action requests are rejected.

## Add other machines

Run `mash dash --services-config ...` on each participating machine with its own
`node` name, token and locally configured supervisor targets. Add those machines
to the dashboard you use as your main entry point:

```toml
[[machines]]
id = "dev-macbook"
url = "http://100.64.0.12:8790"
token_env = "MASH_MACBOOK_SERVICES_TOKEN"
```

The remote `node` must equal `id`. The main dashboard resolves this remote token
from its own environment and forwards requests; it never sends remote tokens to
the browser. Each remote executes only its own configured actions. Requests do
not chain through further dashboards. An unavailable or incorrectly authenticated
machine appears **Unknown**, not **Stopped**. A fresh remote report is required
before its service controls are shown.

## Supervisor targets and logs

Supported `kind` values for managed targets are `watcher`, `qdrant`, `server` and
`dashboard`. The serving dashboard's own stop/restart controls are disabled:
manage that process externally. MCPs belong to their agent sessions and are never
kill/restart targets. SQLite is a database file and never has process controls.

| Manager | Existing target | Available controls |
| --- | --- | --- |
| `systemd_user` | User unit, for example `mash-watch.service` | Start, stop, restart |
| `systemd_system` | System unit accessible to the dashboard account | Start, stop, restart; no elevation prompt |
| `launchd` | User GUI-domain label; `plist` is required for control | Bootstrap, bootout, kickstart |
| `windows_service` | Service name | Start, stop, restart |
| `windows_task` | Scheduled task name in the root task folder | Start, stop |

Names are restricted to ASCII letters, digits, `_`, `-` and `.`; leading `-` is
rejected. Configured IDs must be unique. At most 16 supervisor targets and 32
remote machines are accepted per dashboard. Unsupported platform managers and
unverifiable states show **Unknown** and do not expose controls. OS permissions
still apply; Mash never runs sudo or installs a service.

For launchd, add an absolute `plist = "/Users/you/Library/LaunchAgents/…plist"`
path matching the configured label. Other managers do not use `plist`.

**View log** reads the last 100 journal entries for systemd. Other managers need
an optional absolute `log = "/private/path/to/service.log"`; this reads only
the final 64 KiB of that regular file. Recognizable credentials are masked, but
logs can still contain private operational data. Only authenticated service
access exposes configured logs. The API never accepts a log path from a browser.

## Interpret status and schema warnings

Process existence alone does not establish application readiness. Supervisor
reports say whether a process is running; the Qdrant readiness and shared-server
checks are separate. A successful endpoint check does not certify every
collection or client session. A machine's dot reflects the most severe reported
condition, including unknown checks.

For registered MCPs and watchers, compatibility compares the database's
`user_version` with the running binary's reported schema. A mismatch is red even
when the process is alive. The details show database and supported schema,
running build, the observing dashboard's build, blocked operations and a next
step. The dashboard build is not claimed to be the installed binary's version.

An old-but-compatible runtime has a separate warning when its executable was
replaced or differs from the observer. Reconnect agent-owned MCPs from the agent;
restart a managed watcher through its supervisor. An already upgraded database
does not need another migration just because an old MCP is still running.
Older runtime records without schema evidence show **Unknown** compatibility.

Inventory covers registered processes for the selected `--db` only. Pre-registration
releases and processes using other databases may be absent. To inspect a shared
node's silver database, run the observing dashboard with that database's `--db`
path; do not treat a successful check of one database as fleet-wide coverage.

You can also list additional local silver files, including each node database
owned by a shared server. These have read-only schema checks and no process controls:

```toml
[[databases]]
id = "shared-macbook"
name = "Shared silver · dev-macbook"
path = "/private/shared-silver/dev-macbook.sqlite3"
```

Use the actual file paths from your server configuration. At most 64 additional
databases are accepted. These checks compare against the observing dashboard's
schema, not an unobserved server process's schema.

## Actions, interruptions and recovery

Select a service, choose **Start**, **Stop** or **Restart**, and confirm the named
machine and service. Only actions enabled by `allow_control = true` are admitted,
and the server checks supervisor state again before sending the command.
The request is bound to the displayed target configuration; an old confirmation
is rejected if that target has changed.
Start is offered for stopped/failed targets; stop and restart require a running
target. There is no fleet-wide restart.

**Recent activity** records the request and its result. A supervisor exit code
alone is insufficient: success requires observing the requested process state.
Application readiness and schema compatibility still need their own checks.
Timeouts and unconfirmed outcomes are **Unknown**. Check status and history
before requesting another action; the browser does not automatically retry.

Receipts live beside the service configuration in `FILENAME.operations/`. Keep
this directory across dashboard restarts. A duplicate request ID reads its old
receipt and never repeats the command. A disconnected browser does not cancel
an admitted worker. If the dashboard process exits before a result is recorded,
the receipt stays unknown. This protects against process interruption; it does
not establish power-loss durability on every filesystem. Do not delete receipts
while requests may still be retried. History shows the latest 100 receipts and
fails explicitly if the directory exceeds its bounded scan limit.
