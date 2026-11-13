# Install Mash

This guide sets up Mash on one machine, the default for everyday use. Install
Mash and run Qdrant on the same computer as your coding-agent histories.
Indexing and search stay local; semantic models are optional.

Follow the steps below to install, connect your histories, and search. Add
[multi-machine sharing](shared-index.md) later if you want to search chats from
another computer.

Already using Mash? Read [upgrades and backups](upgrades.md) before replacing
a release that uses a different database schema.

## Install the CLI

Public release packages have not been published yet. For now, install from
source with a stable Rust toolchain.

CLI output is JSON, including `--version` and `--help`.

```bash
cargo +stable install --locked --git https://github.com/kojah/mash mash
mash --version
```

This installs the command line and the MCP server. The web dashboard's pages
are built separately, so `mash dash` from this install says the dashboard has
not been built. To include it, install [Node.js](https://nodejs.org/) and
[pnpm](https://pnpm.io/), then build from a checkout:

```bash
git clone https://github.com/kojah/mash
cd mash/dashboard
pnpm install --frozen-lockfile
pnpm build
cd ..
cargo +stable install --locked --path .
```

If your shell cannot find `mash`, add Cargo's binary directory to `PATH`
(usually `~/.cargo/bin`, or `%USERPROFILE%\.cargo\bin` on Windows).
For the current terminal, use `export PATH="$HOME/.cargo/bin:$PATH"` on
macOS/Linux, or `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"` in PowerShell.
Add the same directory through your shell profile or Windows environment settings
to keep it available in new terminals.

### Recommended release installers

The shell and PowerShell scripts are the primary way to install published Mash
releases. They download a prebuilt binary without requiring Rust. The commands
below will work once the first release is published.

Prebuilt release targets are macOS Apple Silicon, Linux x86_64/arm64, and
Windows x64. Choose **one** installation method.

#### macOS or Linux

```sh
curl -fsSL https://raw.githubusercontent.com/kojah/mash/main/install.sh | sh
```

#### Windows x64

Use the PowerShell installer:

```powershell
irm https://raw.githubusercontent.com/kojah/mash/main/install.ps1 | iex
```

The scripts verify release checksums and default to your user's `.local/bin`
directory. Add that directory to `PATH` if needed. Set `MASH_VERSION` to a
published tag to pin a release, or `MASH_INSTALL_DIR` to choose another directory.
The scripts do not modify your shell profile or download models. WSL uses the
Linux instructions.

### Package manager alternatives

If you prefer a package manager, choose one of these instead. Each channel must
have a published release before its command works.

**Homebrew — macOS or Linux**

```sh
brew install kojah/tap/mash
```

**apt — Debian or Ubuntu**

```sh
curl -fsSL https://getmash.io/apt/mash-archive-keyring.gpg | sudo tee /usr/share/keyrings/mash-archive-keyring.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/mash-archive-keyring.gpg] https://getmash.io/apt stable main" | sudo tee /etc/apt/sources.list.d/mash.list
sudo apt update && sudo apt install mash
```

**Scoop — Windows**

```powershell
scoop bucket add kojah https://github.com/kojah/mash
scoop install kojah/mash
```

**mise**

```sh
mise use -g github:kojah/mash
```

**cargo-binstall**

```sh
cargo binstall --git https://github.com/kojah/mash mash
```

## Start the local search service

Indexing and every search use a local [Qdrant](https://qdrant.tech) service.
Install and start it with one command:

```sh
mash qdrant install
```

This downloads Qdrant 1.20.0 for your platform (about 30 MB) from
[Qdrant's GitHub releases](https://github.com/qdrant/qdrant/releases/tag/v1.20.0),
checks it against a SHA-256 checksum built into Mash, and starts it in the
background as your user. It needs no administrator rights or Docker, listens
only on this machine (`127.0.0.1`, ports 6333 and 6334), and starts again when
you log in. The JSON result has `"outcome": "installed"`; next, run `mash init`.

| Platform | Runs as | Qdrant's output |
| --- | --- | --- |
| Linux | systemd user service `mash-qdrant.service` | `journalctl --user --unit mash-qdrant.service` |
| macOS | LaunchAgent `dev.mash.qdrant` | `~/Library/Application Support/dev.jamesqo.mash/qdrant/qdrant.log` |
| Windows | logon task `MashQdrant` | `%LOCALAPPDATA%\jamesqo\mash\data\qdrant\qdrant.log` |

Qdrant keeps its data next to the program, in `~/.local/share/mash/qdrant` on
Linux and `~/Library/Application Support/dev.jamesqo.mash/qdrant` on macOS. On
Windows the data goes to `C:\mash-qdrant`, because Qdrant fails with longer
data paths there. `mash status` shows the installed version and whether the
service is running, under `local_qdrant.installed`.

Run `mash qdrant install` again at any time. It restarts a stopped service,
restores missing files, and upgrades Qdrant when a newer Mash pins a newer
release, without changing Qdrant's data. When nothing needs doing, the outcome
is `current`. If a compatible Qdrant already answers on those ports, such as
the Docker container below, the outcome is `already_running` and nothing is
installed. If another program holds a port, the command stops and says which.

On Linux, a systemd user service runs while you are logged in. To keep it
running on a machine nobody logs in to, enable lingering for your account with
`loginctl enable-linger` (some systems allow this only for administrators).
The command needs a systemd user session; where there is none, as in some
containers and WSL setups, run Qdrant yourself as described below.

Only `mash qdrant install` downloads Qdrant: `mash init` checks that it
answers, but never downloads or starts it. To use other ports, set `url` and
`grpc_url` in the [Qdrant configuration](configuration.md#local-qdrant) to
`127.0.0.1` addresses before installing.

To remove it, run `mash qdrant uninstall`. It stops the service, removes it from
your login items and deletes the program, but keeps Qdrant's data. Add
`--delete-storage` to delete the data too; it holds only Mash's search data,
which `mash index --rebuild-local-views --full` rebuilds from your index.

### Run Qdrant yourself

You can run Qdrant directly with the instructions below, or use the
[official Qdrant Docker image](#optional-qdrant-docker-image). An existing
Qdrant 1.20+ service also works. Both HTTP and gRPC ports must reach
the same instance. For nondefault endpoints, see [Qdrant configuration](configuration.md#local-qdrant).
Mash and the dashboard run directly on your machine in either setup.

If you later switch to `mash qdrant install`, stop your own Qdrant first. The
installed service starts with empty storage; populate it with
`mash index --rebuild-local-views --full`.

### Native Windows without Docker

On Windows x64, download [Qdrant 1.20.0 for Windows](https://github.com/qdrant/qdrant/releases/download/v1.20.0/qdrant-x86_64-pc-windows-msvc.zip)
and extract it into `C:\mash-qdrant` (or another short, writable folder).
Use a short absolute data path: longer paths under your user profile can make
Qdrant's payload indexes fail even when its startup health check succeeds.
In PowerShell, run:

```powershell
Set-Location 'C:\mash-qdrant'
$env:QDRANT__SERVICE__HOST = '127.0.0.1'
$env:QDRANT__STORAGE__STORAGE_PATH = 'C:\mash-qdrant\storage'
$env:QDRANT__STORAGE__SNAPSHOTS_PATH = 'C:\mash-qdrant\snapshots'
.\qdrant.exe
```

Keep this window open and use a second terminal for Mash. Qdrant stores its
data in these folders; use the same settings next time. Binding to
`127.0.0.1` keeps the service accessible only on this machine.
In the second terminal, check readiness with
`curl.exe --fail http://127.0.0.1:6333/readyz` before running `mash init`.

### Native macOS or Linux without Docker

Download the archive for your machine from the
[Qdrant 1.20.0 release](https://github.com/qdrant/qdrant/releases/tag/v1.20.0):

| Machine | Archive |
| --- | --- |
| macOS, Apple Silicon | [Download](https://github.com/qdrant/qdrant/releases/download/v1.20.0/qdrant-aarch64-apple-darwin.tar.gz) |
| macOS, Intel | [Download](https://github.com/qdrant/qdrant/releases/download/v1.20.0/qdrant-x86_64-apple-darwin.tar.gz) |
| Linux, x86_64 | [Download](https://github.com/qdrant/qdrant/releases/download/v1.20.0/qdrant-x86_64-unknown-linux-musl.tar.gz) |
| Linux, ARM64 | [Download](https://github.com/qdrant/qdrant/releases/download/v1.20.0/qdrant-aarch64-unknown-linux-musl.tar.gz) |

Extract it into a dedicated folder, such as `~/qdrant`. Open a terminal there:

```sh
cd "$HOME/qdrant"
QDRANT__SERVICE__HOST=127.0.0.1 ./qdrant
```

Keep that terminal open and run Mash in another. Start Qdrant from the same
folder next time to reuse its data. The service listens only on this machine;
Mash's default settings already point to it.
In the second terminal, check readiness with
`curl --fail http://127.0.0.1:6333/readyz` before running `mash init`.

### Optional Qdrant Docker image

If you prefer containers, install and start
[Docker Desktop](https://www.docker.com/products/docker-desktop/) on macOS/Windows,
or [Docker Engine](https://docs.docker.com/engine/install/) on Linux.
Windows needs Linux containers; Windows Server/Engine-only hosts may not support
them. Use the native Windows instructions above in that case.

Run this command in Bash or PowerShell:

```sh
docker run -d --name mash-local-qdrant --restart unless-stopped --memory 2g -p 127.0.0.1:6333:6333 -p 127.0.0.1:6334:6334 --mount type=volume,source=mash-qdrant-data,target=/qdrant/storage qdrant/qdrant:v1.20.0
```

Docker downloads Qdrant and keeps its data in a persistent volume. Only this
machine can access it. Open [the readiness check](http://127.0.0.1:6333/readyz)
and wait for “all shards are ready” before running `mash init`.

```sh
docker logs mash-local-qdrant
docker stop mash-local-qdrant
docker start mash-local-qdrant
```

Qdrant restarts with Docker unless you explicitly stopped it. Stopping preserves
data; do not delete the `mash-qdrant-data` volume unless you intend to discard it.
If the container already exists, use `docker start` rather than `docker run`.
When switching from native Qdrant, stop it first to free ports 6333/6334. The
container has separate storage; populate it with
`mash index --rebuild-local-views --full`.

## Connect your histories

Word search needs no models. If you want search by meaning, run
`mash models install` before `mash init`. See [model setup](indexing.md) for
download details and enabling it after setup.

```sh
mash init
```

When stdin and stderr are terminals, `mash init` opens a setup wizard:

1. Choose **Just this computer**, connect to an existing shared server, or set up
   sharing later. Local search is the default.
2. Select detected histories, or add or replace a provider's history path.
3. Choose the last 90 days, all history, or a custom number of days.
4. Select Claude Code and Codex integrations found on `PATH`.
5. Optionally backfill memory from chats active in the last 3 days using a
   selected signed-in agent. It runs here after indexing, uses agent tokens,
   and sends selected chat text to that agent's service. Older chats are never
   assigned to future conversations. See [memory backfill](records.md#backfill-earlier-memory).
6. Review the summary and confirm. You can save configuration only or cancel.

Mash checks local Qdrant connectivity and, when selected, shared-server
authentication before confirmation. If a connection fails, save configuration
and finish setup after fixing it. The Qdrant check verifies its HTTP endpoint;
indexing also requires its configured gRPC endpoint.

Without the wizard, `mash init` checks Qdrant before changing anything. If
Qdrant does not answer, it exits with the commands to start it and leaves
configuration and agent settings untouched. `mash init --no-index` skips the
check and lists starting Qdrant among the next steps.

You can read your history before Qdrant runs: run `mash init --no-index`, then
`mash index`. Indexing saves your chats on this machine before it needs Qdrant,
then stops with the commands to start it and says how many chats it saved.
`mash list-chats` and `mash view` read them meanwhile; once Qdrant runs, run
`mash index` again to make search current.

Press Esc or Ctrl-C during the prompts to cancel without changing configuration
or agent settings. Prompts go to stderr; stdout remains JSON. No models are
downloaded during setup.

For scripts or to accept the detected defaults, use `mash init --yes`. Runs
without terminal input or terminal stderr are non-interactive too. Defaults
include sources modified within the last 90 days and available agent
integrations, without historical memory backfill. This includes current sessions of all detected supported agents,
including Copilot. The JSON result lists the selected provider folders.
To choose them yourself before indexing, use `mash init --no-index`, edit the
[configuration](configuration.md), then run `mash index`. Restart your agents
after setup.

The history plugin's hooks need Python 3.9+; see [agent setup](agents.md).
For CLI-only use, run `mash init --no-agents`. Each connected agent in the
result lists the settings `files` its CLI edited and the `undo` commands that
remove the connection. To configure without indexing,
use `--no-index`. The wizard offers to keep existing settings or review them;
reviewing preserves settings outside the wizard and backs up the previous file.
Non-interactive setup keeps existing configuration unless `--force` is passed.
With `--yes --force`, Mash regenerates configuration from detected histories;
in the wizard, `--force` opens the settings review directly.

If the first index fails after setup has been applied (for example, Qdrant
stops or its gRPC port is unreachable), the configuration, agent registrations
and history already saved are kept. The result still lists them, adds
`index_error`, and exits with that error. Fix the cause, then run `mash index`
to finish. On a rerun of `mash init`, `written: false` means the existing
configuration was reused, not that setup failed.

Setup writes progress to stderr and the final JSON result to stdout. Both appear
in your terminal by default. `"agents": []` means no agents were connected;
successful indexing does not mean an agent connection was set up. Read `next`
in the result for the reason and follow [agent setup](agents.md) to connect one.
You can use CLI search without connecting an agent.

Searchable messages are the user and
assistant dialogue; tool output and reasoning remain available when reading a
chat but are excluded from search, so the searchable count can be much smaller.

## Try your first search

```sh
mash status
mash search 'migration'
```

Replace the query with a distinctive word from a real chat. To include older
history, run `mash index --all-history`. To browse in your browser, follow
[dashboard setup](dashboard.md); source installations need to build the web
interface first.

Without semantic models, multiple words must occur together as a phrase.
“Log rotation” won't match “rotate the application logs”; try “rotate” first.

You're ready for [searching](search.md), [the dashboard](dashboard.md), or
[agent-assisted recall](agents.md). Add [semantic search](indexing.md#enable-semantic-search)
later if you want to search by meaning.
