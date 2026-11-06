# Install Mash

You need some existing history from a supported coding agent and a local
Qdrant search service. Shared hosting and semantic models are optional.

Already using Mash? Read [upgrades and backups](upgrades.md) before replacing
a release that uses a different database schema.

## Install the CLI

Public release packages have not been published yet. For now, install from
source with a stable Rust toolchain:

```bash
cargo +stable install --locked --git https://github.com/kojah/mash mash
mash --version
```

If your shell cannot find `mash`, add Cargo's binary directory to `PATH`
(usually `~/.cargo/bin`, or `%USERPROFILE%\.cargo\bin` on Windows).

<details>
<summary>Release installers and package managers — available after publication</summary>

Prebuilt release targets are macOS Apple Silicon, Linux x86_64/arm64, and
Windows x64. Choose **one** installation method.

### macOS or Linux

```sh
brew install kojah/tap/mash
```

Or use the shell installer:

```sh
curl -fsSL https://raw.githubusercontent.com/kojah/mash/main/install.sh | sh
```

### Windows x64

Use the PowerShell installer:

```powershell
irm https://raw.githubusercontent.com/kojah/mash/main/install.ps1 | iex
```

Or choose winget, Scoop, or Chocolatey:

```powershell
winget install --id kojah.mash --exact
```

```powershell
scoop bucket add kojah https://github.com/kojah/mash
scoop install kojah/mash
```

```powershell
choco install mash
```

### Debian or Ubuntu

```sh
curl -fsSL https://mashapp.dev/apt/mash-archive-keyring.gpg | sudo tee /usr/share/keyrings/mash-archive-keyring.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/mash-archive-keyring.gpg] https://mashapp.dev/apt stable main" | sudo tee /etc/apt/sources.list.d/mash.list
sudo apt update && sudo apt install mash
```

### Other package managers

```sh
npm install -g @kojah/mash
```

```sh
mise use -g github:kojah/mash
```

```sh
cargo binstall --git https://github.com/kojah/mash mash
```

The shell and PowerShell scripts verify release checksums and default to your
user's `.local/bin` directory. Add that directory to `PATH` if needed.
Set `MASH_VERSION` to a published tag to pin a release, or `MASH_INSTALL_DIR`
to choose another directory. The scripts do not modify your shell profile or
download models. WSL uses the Linux instructions.

</details>

## Start the local search service

With Docker running, this command works in Bash and PowerShell:

```sh
docker run -d --name mash-local-qdrant --memory 2g -p 127.0.0.1:6333:6333 -p 127.0.0.1:6334:6334 -v mash-local-qdrant-data:/qdrant/storage qdrant/qdrant:v1.19.1
```

The named volume keeps search data across container restarts. After a reboot,
start the existing container with `docker start mash-local-qdrant`.
Mash does not install or manage Qdrant itself.

An existing Qdrant 1.19+ service also works. Both HTTP and gRPC ports must reach
the same instance. For nondefault endpoints, see [Qdrant configuration](configuration.md#local-qdrant).

## Connect your histories

```sh
mash init
```

This detects supported histories, writes TOML configuration, sets up Claude Code
and Codex integrations when their CLIs are on `PATH`, and indexes sources modified
within the last 90 days. Restart your agents after setup.

The history plugin's hooks need Python 3.9+; see [agent setup](agents.md).
For CLI-only use, run `mash init --no-agents`. To configure without indexing,
use `--no-index`. Existing configuration is kept; avoid `--force` unless you
intend to replace it (the previous file is backed up).

## Try your first search

```sh
mash status
mash search 'a word you remember' --intent exact-mention
```

Replace the query with a distinctive word from a real chat. To include older
history, run `mash index --all-history`. To browse in your browser, run
`mash dash` and open [http://127.0.0.1:8790](http://127.0.0.1:8790).

You're ready for [searching](search.md), [the dashboard](dashboard.md), or
[agent-assisted recall](agents.md). Add [semantic search](indexing.md#enable-semantic-search)
later if you want to search by meaning.
