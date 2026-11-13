# Inspect and manage Mash services

Run `mash dash` and open **services** in the dashboard's rail. It shows the
local SQLite schema, registered MCP and watcher processes, the dashboard
process, and configured Qdrant and shared-server endpoints. Checks never start
indexing, migrate SQLite or rebuild Qdrant.

The page is a table of machines against kinds of service: index, search, API,
watcher, MCP and dashboard. Choose a machine's row to list its services, then a
service to see its diagnostics, schema compatibility and any controls.

- A supervised service and its health check share one row, healthy only when
  the process is running and the check passes.
- Agent sessions on an older build show as **reconnect**, and an unsupervised
  watcher shows as **off**. Neither counts against the machine.
- **Needs attention** lists what counts against the selected machine and what
  to do, with a button such as **Start** where a control would fix it.
- **Check again** refreshes the report. Nothing polls on a timer, so an old
  green result is not a current one.

This much needs no configuration. Service controls, other machines and fleet
operations are opt-in and described from
[Enable service access](#enable-service-access) onward.

## Check each machine's agent setup

Under the selected machine's services, **agent setup** gives each agent one
line of status, such as **Ready** or **Hooks need approval**, then each part of
the integration. `mash status --installation` runs the same check on one machine.

- **Connection:** the agent has Mash registered as an MCP server.
- **Plugin** and **Skill:** the Mash plugin is installed and matches the Mash
  binary on that machine.
- **Hooks:** the plugin's hook scripts are present and unmodified.
- **Hook approval:** Codex only. Codex runs a plugin's hooks after you review
  them in its `/hooks` screen, and asks again when they change.

A part that needs attention shows what to do. Mash only reads settings and
files, so a part can read as present without having run, and an approved Codex
hook reads as recorded because only Codex can confirm it matches the current
hooks. A machine running an older Mash cannot report its setup: update it first.

## Interpret status and schema warnings

A running process is not necessarily ready. Supervisor reports, Qdrant
readiness and shared-server checks are separate, and a successful endpoint
check does not certify every collection or client session. A machine's dot
reflects the most severe reported condition, including unknown checks.

For registered MCPs and watchers, compatibility compares the database's
`user_version` with the running binary's reported schema. A mismatch is red even
when the process is alive; the details show both schemas, the running and
observing builds, blocked operations and a next step. Older runtime records
without schema evidence show **Unknown**.

A compatible runtime whose executable was replaced, or differs from the
observer, gets a separate warning. Reconnect agent-owned MCPs from the agent; a
managed watcher restarts itself once its active pass ends. The database does
not need another migration.

Only processes registered for the selected `--db` are listed. Pre-registration
releases and processes using other databases may be absent.

## Enable service access

To add **Start**, **Stop**, **Restart** and **View log** for services your OS
supervisor already runs, list them in a separate, private TOML file. The
dashboard does not install units, so the named unit must already exist.

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

Only services with `allow_control = true` get controls.

- The service APIs have no browser sign-in. Anyone who can reach the dashboard
  can view and control its services.
- For remote access, bind explicitly with `--listen ADDRESS:PORT`, and only on
  a trusted private encrypted network such as Tailscale, or behind a correctly
  configured HTTPS proxy.
- Leave `token_env` unset on any dashboard you open in a browser. The browser
  cannot supply the bearer token and its requests would be refused.
- The request Host must match the dashboard's bound address. Cross-origin
  browser action requests are rejected.

## Supervisor targets and logs

Supported `kind` values for managed targets are `watcher`, `qdrant`, `server` and
`dashboard`. The serving dashboard cannot stop or restart itself. MCPs and
SQLite never have process controls.

| Manager | Existing target | Available controls |
| --- | --- | --- |
| `systemd_user` | User unit, for example `mash-index-watch.service` | Start, stop, restart |
| `systemd_system` | System unit accessible to the dashboard account | Start, stop, restart; no elevation prompt |
| `launchd` | User GUI-domain label; `plist` is required for control | Bootstrap, bootout, kickstart |
| `windows_service` | Service name | Start, stop, restart |
| `windows_task` | Scheduled task name in the root task folder | Start, stop |

- Names may use ASCII letters, digits, `_`, `-` and `.`, with no leading `-`.
  IDs must be unique.
- A dashboard accepts at most 16 supervisor targets and 32 remote machines.
- Unsupported managers and unverifiable states show **Unknown** with no controls.
- OS permissions still apply. Mash never runs sudo or installs a service.
- For launchd, add an absolute
  `plist = "/Users/you/Library/LaunchAgents/…plist"` path matching the label.

**View log** reads the last 100 journal entries for systemd. Other managers need
an absolute `log = "/private/path/to/service.log"`, of which the final 64 KiB
is read. Recognizable credentials are masked, but logs can still contain
private operational data. The API never accepts a log path from a browser.

To add read-only schema checks for other local silver files, such as each node
database owned by a shared server, list up to 64 of them by their real paths:

```toml
[[databases]]
id = "shared-macbook"
name = "Shared silver · dev-macbook"
path = "/private/shared-silver/dev-macbook.sqlite3"
```

These compare against the observing dashboard's schema, not the server
process's.

## Actions, interruptions and recovery

Select a service, choose **Start**, **Stop** or **Restart**, and confirm the
named machine and service. Start is offered for stopped or failed targets; stop
and restart need a running one. A confirmation is rejected if the target's
configuration has changed since it was displayed. There is no fleet-wide restart.

**Recent activity** records each request and its result. An action succeeds
only when the requested process state is observed. Timeouts and unconfirmed
outcomes are **Unknown** and nothing retries automatically: check status and
history before requesting the action again.

Receipts live beside the service configuration in `FILENAME.operations/`. Keep
this directory across dashboard restarts. Closing the browser does not cancel an
action under way. If the dashboard exits before a result is recorded, the
receipt stays unknown. History shows the latest 100 receipts.

## Add other machines

Optional. Run `mash dash --services-config ...` on each machine with its own
`node` name and supervisor targets, then add them to the dashboard you open:

```toml
[[machines]]
id = "dev-macbook"
url = "http://100.64.0.12:8790"
# token_env = "MASH_MACBOOK_SERVICES_TOKEN"
```

The remote `node` must equal `id`. A peer `token_env` is optional: when omitted,
the remote dashboard must also run without a token, and both endpoints must stay
on a trusted private network. The main dashboard reads remote tokens from its
own environment and never sends them to the browser.

Each remote executes only its own configured actions, and requests do not chain
through further dashboards. An unavailable or incorrectly authenticated machine
appears **Unknown**, not **Stopped**, and shows no controls until it reports
again.

## Manage the fleet from one inventory

Optional, for multi-machine setups. One private TOML inventory gives
`mash-fleet` and the dashboard the same machines, services and watcher
definitions. It replaces `--services-config`; do not pass both flags.

Copy [the example inventory](../deployment/fleet.example.toml) outside Git and
replace its paths and machine addresses. The controller needs the Mash
checkout, a built `mash-fleet` binary and its Ansible runtime dependencies; see
[fleet setup](../deployment/README.md).

```sh
mash-fleet --fleet-config /private/mash-fleet.toml status --all
mash dash --fleet-config /private/mash-fleet.toml
```

On each peer, pass its machine ID and an explicit private listen address:

```sh
mash dash --fleet-config /private/mash-fleet.toml --fleet-node macbook --listen 100.64.0.12:8790
```

Keep every dashboard on a trusted private network, because the browser does not
sign in. Keep the inventory and job directory private. Deployment commands
belong in the inventory, never in browser requests.

### Run fleet operations

Open **Services → Fleet operations** on the controller. Choose an operation,
select machines, and review the named targets before confirming.

- The controller's `allow_actions` selects which operations appear.
- Deployment also needs configured build, install and preflight commands.
  Adding `deploy` to the allowlist does not provide an installer.
- Each target shows one line on what the operation found or why it failed.
- Jobs continue when you leave the page.
- An **unknown** outcome means the controller could not confirm completion.
  Check the affected machines before starting another operation.
- Command output stays in the private controller job directory. **Collect
  watcher logs** saves logs there without displaying them in the browser.

### Watchers

Watcher start, or `mash-fleet watch start HOST`, installs the configured
watcher (`mash-index-watch.service` on Linux, `dev.mash.index-watch` on macOS)
and starts `mash index --watch` through your launcher. Stop disables that
supervisor job.

- Fleet watcher operations need their action in `allow_actions`. The watcher's
  `allow_control` separately enables its individual service buttons, which use
  the same definition.
- After an upgrade the watcher starts the new binary itself. Deploys do not
  touch it.
- `mash status` reports a `watcher` whose `running` is false when the job is
  installed but no watcher runs for that index. Start it again.

### Windows machines and shared servers

A Windows machine uses `platform = "windows"` and appears through its own
`mash dash` peer, with the services you list for it under the `windows_task` or
`windows_service` managers. The fleet can check it, index and publish its
history, and rebuild its local search. It can also deploy: the machine builds
its own binary from the published code and the fleet swaps it in (see
[fleet setup](../deployment/README.md)). Its watcher is a scheduled task named
`mash-index-watch` that starts with the machine. Give the machine a
`[machines.watcher]` whose `launcher` is the script or program to run.

A shared server is a service of the machine that runs it, not a machine of its
own. Give its entry under that machine a `deployment` table and it appears under
**services** in the picker, named by its `id`. Machines that publish to it list
that `id` in `requires`; select the server along with them when deploying.
