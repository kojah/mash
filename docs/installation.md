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

Run Qdrant directly using the instructions for your platform below. It powers
both word and semantic search. Alternatively, use the
[official Qdrant Docker image](#optional-qdrant-docker-image). Mash and the
dashboard run directly on your machine in either setup.

An existing Qdrant 1.19+ service also works. Both HTTP and gRPC ports must reach
the same instance. For nondefault endpoints, see [Qdrant configuration](configuration.md#local-qdrant).

### Native Windows without Docker

On Windows x64, download [Qdrant 1.19.1 for Windows](https://github.com/qdrant/qdrant/releases/download/v1.19.1/qdrant-x86_64-pc-windows-msvc.zip)
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
[Qdrant 1.19.1 release](https://github.com/qdrant/qdrant/releases/tag/v1.19.1):

| Machine | Archive |
| --- | --- |
| macOS, Apple Silicon | [Download](https://github.com/qdrant/qdrant/releases/download/v1.19.1/qdrant-aarch64-apple-darwin.tar.gz) |
| macOS, Intel | [Download](https://github.com/qdrant/qdrant/releases/download/v1.19.1/qdrant-x86_64-apple-darwin.tar.gz) |
| Linux, x86_64 | [Download](https://github.com/qdrant/qdrant/releases/download/v1.19.1/qdrant-x86_64-unknown-linux-musl.tar.gz) |
| Linux, ARM64 | [Download](https://github.com/qdrant/qdrant/releases/download/v1.19.1/qdrant-aarch64-unknown-linux-musl.tar.gz) |

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
docker run -d --name mash-local-qdrant --restart unless-stopped --memory 2g -p 127.0.0.1:6333:6333 -p 127.0.0.1:6334:6334 --mount type=volume,source=mash-qdrant-data,target=/qdrant/storage qdrant/qdrant:v1.19.1
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

This detects supported histories, writes TOML configuration, sets up Claude Code
and Codex integrations when their CLIs are on `PATH`, and indexes sources modified
within the last 90 days. This includes current sessions of all detected supported
agents, including Copilot. The JSON result lists the selected provider folders.
To choose them yourself before indexing, use `mash init --no-index`, edit the
[configuration](configuration.md), then run `mash index`. Restart your agents
after setup.

The history plugin's hooks need Python 3.9+; see [agent setup](agents.md).
For CLI-only use, run `mash init --no-agents`. To configure without indexing,
use `--no-index`. Existing configuration is kept; avoid `--force` unless you
intend to replace it (the previous file is backed up).

If indexing fails because Qdrant is stopped, the configuration and history
already saved are kept. Start Qdrant and rerun `mash init`; `written: false`
means the existing configuration was reused, not that setup failed.

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
