# Installation

[Documentation](README.md) · [Project home](../README.md)

## Choose an installer

Public releases and package channels have not been published yet. Until they
are available, build with Rust:

```bash
cargo +stable install --locked --git https://github.com/kojah/mash mash
```

The release workflow prepares the following installer channels. Commands that
download a release or use a package registry require publication to that channel.

Prebuilt binaries support macOS Apple Silicon, Linux x86_64 or arm64, and
Windows x64. Choose one of the following installers for your platform.

With Homebrew:

```bash
brew install kojah/tap/mash
```

or

```bash
curl -fsSL https://raw.githubusercontent.com/kojah/mash/main/install.sh | sh
```

For Windows x64, the standalone PowerShell installer is:

```powershell
irm https://raw.githubusercontent.com/kojah/mash/main/install.ps1 | iex
```

Both scripts verify the archive against the release’s `SHA256SUMS` before
installing. Set `MASH_VERSION` to a release tag such as `v0.8.0` to pin a version,
or `MASH_INSTALL_DIR` to change the destination from your user’s `.local/bin`.
They print a reminder if that directory needs adding to `PATH`; they do not
change your shell profile or install models. For an existing index, follow
[the upgrade guide](upgrades.md) before crossing database schemas.

On Debian or Ubuntu, from the signed apt repository:

```bash
curl -fsSL https://kojah.github.io/mash/apt/mash-archive-keyring.gpg | sudo tee /usr/share/keyrings/mash-archive-keyring.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/mash-archive-keyring.gpg] https://kojah.github.io/mash/apt stable main" | sudo tee /etc/apt/sources.list.d/mash.list
sudo apt update && sudo apt install mash
```

or with a tool you already use:

```bash
npm install -g @kojah/mash
mise use -g github:kojah/mash
cargo binstall --git https://github.com/kojah/mash mash
```

On Windows x64, use one of:

```powershell
winget install --id kojah.mash --exact
scoop bucket add kojah https://github.com/kojah/mash; scoop install kojah/mash
choco install mash
```

The history plugin's hooks need Python 3.9 or later as `python3`, `python` or
the Windows `py` launcher; without one they do nothing. On Windows, long-lived
commands (`mash mcp`, `index --watch`, `serve`) move their running executable
aside (`.mash-running-<pid>.exe`) and leave a copy at the installed path, so
installers can replace `mash.exe` while an agent is connected; reconnect
afterwards. Between those two renames the installed path is briefly absent;
set `MASH_KEEP_EXECUTABLE=1` where Mash starts constantly, such as test
runners. Windows identifies the installed executable by size and modification
time, so a same-size replacement within one timestamp tick would not be noticed
as an upgrade. WSL works like Linux.

## Start Qdrant

Start Qdrant 1.19+ with persistent storage before indexing or searching. For example:

```bash
docker run -d --name mash-local-qdrant --memory 2g \
  -p 127.0.0.1:6333:6333 -p 127.0.0.1:6334:6334 \
  -v mash-local-qdrant-data:/qdrant/storage qdrant/qdrant:v1.19.1
```

Mash neither installs nor manages this service. Both ports must address the
same instance. The defaults above need no node configuration; custom endpoints
go in strict TOML:

```toml
[qdrant]
url = "http://127.0.0.1:6333"
grpc_url = "http://127.0.0.1:6334"
# api_key_env = "MASH_LOCAL_QDRANT_KEY"
memory = "durable"
```

## Initialize Mash

Then set Mash up:

```bash
mash init
```

`mash init` finds the Claude Code, Codex, Copilot, Cursor, OpenCode and
Antigravity histories in their default locations, writes `config.toml`,
registers the Mash MCP server and history plugin with the Claude Code and Codex
CLIs it finds on `PATH`, and indexes the last 90 days. Restart your agents and
ask them about past work. Without models, search uses lexical matching; for
semantic search run `mash models install` (about 170 MB), then
`mash init --force`. `--no-agents` leaves agent settings alone, `--no-index`
skips indexing, and an existing configuration is kept unless `--force` is given
(the old file is saved as `config.toml.previous`).

The plugin keeps the index current: when an agent session starts it begins a
low-priority incremental `mash index` in the background, at most every ten
minutes (`MASH_AUTO_INDEX=off` disables it). For continuous updates run
`mash index --watch` yourself; Mash installs no daemon.

To build from source instead: `cargo +stable install --locked --git https://github.com/kojah/mash mash`.
