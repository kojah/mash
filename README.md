<p align="center">
  <img src="assets/mash-logo.png" alt="mash logo: a smiling bowl of mashed potatoes" width="200">
</p>

# mash - The **M**ulti-**A**gent **S**earch **H**istory Tool

**Find things you’ve already worked out with your coding agents.**

Mash searches your Claude Code, Codex, GitHub Copilot, Cursor, OpenCode, and
Antigravity chats. Ask your agent to find an earlier decision, search from the
terminal, or browse conversations in the dashboard.

- **Pick up where you left off.** Find past discussions and read the messages behind an answer.
- **Search across agents.** Bring supported histories together without changing their original files.
- **Keep it local.** Index and search on your machine, with optional sharing across your own machines.

## Quick start

You’ll need some existing agent history and a running local Qdrant search service.
Qdrant powers both word search and semantic search; semantic models are optional.

### 1. Install Mash

Public release packages have not been published yet. For now, build with Rust:

```bash
cargo +stable install --locked --git https://github.com/kojah/mash mash
```

The first build can take several minutes.

<details>
<summary>Release installers and package managers (available after publication)</summary>

**Homebrew**

```bash
brew install kojah/tap/mash
```

**Shell — macOS / Linux**

```bash
curl -fsSL https://raw.githubusercontent.com/kojah/mash/main/install.sh | sh
```

**PowerShell — Windows x64**

```powershell
irm https://raw.githubusercontent.com/kojah/mash/main/install.ps1 | iex
```

Both scripts verify release checksums. They install to your user’s `.local/bin`
directory; add it to `PATH` if needed.

**Windows package managers — choose one**

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

**apt — Debian / Ubuntu**

```bash
curl -fsSL https://mashapp.dev/apt/mash-archive-keyring.gpg \
  | sudo tee /usr/share/keyrings/mash-archive-keyring.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/mash-archive-keyring.gpg] https://mashapp.dev/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/mash.list
sudo apt update && sudo apt install mash
```

Package channels depend on the release being published to their registry.
See [installation options](docs/installation.md) for npm, mise, and installer settings.

</details>

### 2. Start the local search service

Install and start [Docker Desktop](https://www.docker.com/products/docker-desktop/)
on macOS/Windows, or [Docker Engine with Compose](https://docs.docker.com/compose/install/)
on Linux. Windows must use **Linux containers**.

Save our [compose.yaml](https://raw.githubusercontent.com/kojah/mash/main/compose.yaml)
in a dedicated folder. In a terminal in that folder, run:

```sh
docker compose up -d --wait
```

Compose downloads Qdrant, keeps its data in a persistent volume, and waits for
it to be ready. Mash itself runs on your machine to access your agent histories.
[Setup details and alternatives →](docs/installation.md#start-the-local-search-service)

### 3. Connect your histories

```bash
mash init
```

Setup prints progress and a JSON result for agents and scripts. For a visual
way to browse afterward, run `mash dash`.

Mash finds supported histories in their usual locations and indexes the last
90 days. It also connects Mash to the Claude Code and Codex CLIs it finds.
Restart those agents, then try asking:

> “Use Mash to find where we discussed the database migration.”

Search works with words and phrases out of the box. For search by meaning,
run `mash models install` **before** `mash init` to add the optional local models.
Already initialized? See [model setup](docs/indexing.md).
The history plugin’s hooks also require Python 3.9 or later.

## Try it

```bash
mash search "migration"
mash list-chats --provider codex --limit 10
mash view MESSAGE_ID --context 3
```

Use a message ID returned by search in the last command. CLI results are JSON;
your connected agent can use the same operations through MCP.

Start with one distinctive word you remember. Without semantic models, multiple
words match a phrase: “log rotation” won't find “rotate the application logs.”
Try “rotate,” or enable [search by meaning](docs/indexing.md#enable-semantic-search).

Prefer browsing? Start the local, read-only dashboard:

```bash
mash dash
```

Open the address it prints to browse chats, search history, and read transcripts.

## Keep it up to date

```bash
mash index                 # Pick up recent changes
mash index --all-history   # Include history older than 90 days
mash status                # Check setup and index readiness
```

The history plugin also starts a throttled background refresh when an agent
session begins. For continuous updates, run `mash index --watch` in a separate
terminal. [Synchronization details →](docs/synchronization.md)

## Privacy

With the default local setup, indexing and search stay on your machine.
Sharing history and using remote embedding APIs are optional.

Mash masks recognizable credentials in results by default, but detection is
best effort. Local history and exports can contain sensitive text;
treat backups like the original chats. Mash never modifies the agent’s source
history. [Privacy and redaction →](docs/privacy.md)

## Configuration

`mash init` writes a TOML configuration. Use [the configuration guide](docs/configuration.md)
to choose history locations, search defaults, or remote embeddings.
To search across machines, set up an optional [shared index](docs/shared-index.md).

Upgrading an existing installation? Read [the upgrade guide](docs/upgrades.md)
before replacing an existing installation.

## Learn more

| I want to… | Guide |
| --- | --- |
| Narrow a search by project, provider, or date | [Search and filters](docs/search.md) |
| Connect or troubleshoot an agent | [Agent integrations](docs/agents.md) |
| Browse or save a transcript | [Dashboard](docs/dashboard.md) |
| Find missing or stale history | [Troubleshooting](docs/troubleshooting.md) |
| Search across my machines | [Shared index](docs/shared-index.md) |
| Back up, restore, or upgrade | [Upgrades and backups](docs/upgrades.md) |
| See all commands | [CLI reference](docs/cli.md) |
| Configure an MCP client | [MCP reference](docs/mcp.md) |
| Understand indexing and models | [Indexing](docs/indexing.md) |

[Documentation website](https://mashapp.dev) · [All guides](docs/README.md) · [License](LICENSE)
