# Inspect and manage Mash services

Open **services** in the dashboard's rail. With ordinary `mash dash`, you can
inspect the local SQLite schema, registered MCP and watcher processes, the
dashboard process, and configured Qdrant/shared-server endpoints. Checks never
start indexing, migrate SQLite or rebuild Qdrant.

The page is a table of machines against kinds of service: index, search, API,
watcher, MCP and dashboard. Choose a machine's row to list its services, then a
service to see its diagnostics, schema compatibility and any controls.
A supervised service and the health check of the same thing share one row:
it is healthy only when the process is running and its check passes.
Agent sessions on an older build show as **reconnect**, and a watcher that
nothing supervises shows as **off**; neither counts against the machine.
Choosing a machine lists anything counted against it under **needs attention**,
with what to do about it. Where the service has a control that would put it
right, such as **Start** for a stopped watcher, the button is there too. The
service's own page and the machine's page under **machines** say the same.
**Check again** refreshes the report. There is no timer-based polling or page reload.
Dates tell you when a check was made; an old green result is not a continuous
health guarantee. Fleet job progress, when enabled below, updates automatically
while an operation is running; it does not reload the page or the chat history.

## Check each machine's agent setup

Under the selected machine's services, **agent setup** shows each agent with
one line of status, such as **Ready** or **Hooks need approval**, and the
integration part by part, in the order it is installed. A machine's own page
under **machines** shows the same.

- **Connection:** the agent has Mash registered as an MCP server.
- **Plugin** and **Skill:** the Mash plugin is installed and matches the Mash
  binary on that machine.
- **Hooks:** the plugin's hook scripts are present and unmodified.
- **Hook approval:** Codex only. Codex runs a plugin's hooks after you review
  them in its `/hooks` screen, and asks again when they change.

A part that needs attention shows what to do. This is how a machine where the
plugin has been updated but its hooks still await approval shows up.

Mash reads settings and files for this. It does not start an agent, an MCP
server or a hook, so a part can read as present without having run. After
approving Codex hooks the approval reads as recorded, because only Codex can
confirm it matches the current hooks. A machine running an older Mash cannot
report its setup: update it first. The same check is available on one machine
with `mash status --installation`.

## Manage the fleet from one inventory

Use a shared private TOML inventory to give `mash-fleet` and the dashboard the
same machine names, services, and watcher definitions. This is an alternative
to the standalone `--services-config` setup below; do not pass both flags.
Start with [the example inventory](../deployment/fleet.example.toml), copy it
outside Git, and replace its paths and machine addresses. The controller needs
the Mash checkout, built `mash-fleet` binary, and its Ansible runtime dependencies.
See [fleet setup](../deployment/README.md) for those prerequisites.

```sh
mash-fleet --fleet-config /private/mash-fleet.toml status --all
mash dash --fleet-config /private/mash-fleet.toml
```

On each peer, use the same inventory with its machine ID and an explicit private
listen address:

```sh
mash dash --fleet-config /private/mash-fleet.toml --fleet-node macbook --listen 100.64.0.12:8790
```

Keep every dashboard on a trusted private network, because the browser does not
sign in. Keep the inventory and job directory private. Trusted deployment commands belong in
that file, never in browser requests.

Open **Services → Fleet operations** on the controller. Choose an allowed
operation, select machines, and review the named targets before confirming.
The controller's `allow_actions` selects which operations appear. Deployment
requires configured build/install/preflight commands; adding `deploy` to the
allowlist alone does not provide an installer.

A shared server is a service of the machine that runs it, not a machine of its
own. Give its entry under that machine a `deployment` table and it appears
under **services** in the picker, named by its `id`. Machines that publish to
it list that `id` in `requires`; select the server along with them when
deploying.

Watcher start installs the configured systemd user unit or macOS LaunchAgent
and starts `mash index --watch` through your existing launcher. Stop disables
that supervisor job. The same watcher definition supplies the individual
service controls, so unit names and plist paths do not need a second inventory.
Fleet watcher operations require their action in `allow_actions`; the watcher's
`allow_control` separately enables individual dashboard service buttons.

Jobs continue when you leave the page. Each submission has a saved request ID,
and repeating that ID cannot launch it again. An **unknown** outcome means the
controller could not confirm completion; check the affected machines before
starting another operation. Raw logs and command output remain in the private
controller job directory. **Collect watcher logs** saves logs there; it does not
display raw fleet logs in the browser. Individual configured services retain
their existing **View log** button.

## Enable service access

Create a separate, private TOML file. This example assumes the named systemd
user unit already exists. The dashboard does not install units. With an opt-in
fleet inventory, `mash-fleet watch start HOST` installs the watcher as
`mash-index-watch.service` on Linux or `dev.mash.index-watch` on macOS.
Use those same supervisor identifiers in the dashboard inventory so both tools
manage the same process.

The service APIs have no browser sign-in, so only bind `mash dash` to a trusted
private network such as Tailscale. Leave `token_env` unset on any dashboard you
open in a browser, because the browser cannot supply a token and its requests
would be refused.

```toml
node = "xenia"

[[services]]
id = "watcher"
name = "Index watcher"
kind = "watcher"
manager = "systemd_user"
unit = "mash-index-watch.service"
allow_control = true
```

```sh
mash dash --services-config /private/mash-services.toml
```

**Services** then opens directly. Existing history routes retain their local
dashboard access rules.

For remote access, bind explicitly with `--listen ADDRESS:PORT`. Use a private
encrypted connection such as Tailscale, or HTTPS through a correctly configured
proxy. Anyone who can reach the dashboard can view and control its services.
The existing Host check still applies: the request Host must match the dashboard's
bound address. Cross-origin browser action requests are rejected.

## Add other machines

Run `mash dash --services-config ...` on each participating machine with its own
`node` name and locally configured supervisor targets. Add those machines
to the dashboard you use as your main entry point:

```toml
[[machines]]
id = "dev-macbook"
url = "http://100.64.0.12:8790"
# token_env = "MASH_MACBOOK_SERVICES_TOKEN"
```

The remote `node` must equal `id`. A peer `token_env` is optional and follows the
same rule: when omitted, the remote dashboard must also run without a token, and
both endpoints must stay on a trusted private network. The main dashboard
resolves remote tokens from its own environment and never sends them to the
browser. Each remote executes only its own configured actions. Requests do not
chain through further dashboards. An unavailable or incorrectly authenticated
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
