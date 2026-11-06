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

### 1. Install Mash

Public release packages have not been published yet. For now, build with Rust:

```bash
cargo +stable install --locked --git https://github.com/kojah/mash mash
```

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
curl -fsSL https://kojah.github.io/mash/apt/mash-archive-keyring.gpg \
  | sudo tee /usr/share/keyrings/mash-archive-keyring.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/mash-archive-keyring.gpg] https://kojah.github.io/mash/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/mash.list
sudo apt update && sudo apt install mash
```

Package channels depend on the release being published to their registry.
See [installation options](docs/installation.md) for npm, mise, and installer settings.

</details>

### 2. Start the local search service

With Docker installed, run Qdrant with persistent storage:

```bash
docker run -d --name mash-local-qdrant --memory 2g \
  -p 127.0.0.1:6333:6333 -p 127.0.0.1:6334:6334 \
  -v mash-local-qdrant-data:/qdrant/storage qdrant/qdrant:v1.19.1
```

Mash uses these local addresses by default. Keep Qdrant running while indexing
or searching; after stopping the container, use `docker start mash-local-qdrant`.
[Custom endpoints and setup details →](docs/installation.md)

### 3. Connect your histories

```bash
mash init
```

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
mash search "database migration"
mash list-chats --provider codex --limit 10
mash view MESSAGE_ID --context 3
```

Use a message ID returned by search in the last command. CLI results are JSON;
your connected agent can use the same operations through MCP.

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
best effort. Local canonical history and exports can contain sensitive text;
treat backups like the original chats. Mash never modifies the agent’s source
history. [Privacy and redaction →](docs/privacy.md)

## Configuration

`mash init` writes a TOML configuration. Use [the configuration guide](docs/configuration.md)
to choose history locations, search defaults, or remote embeddings.
To search across machines, set up an optional [shared index](docs/shared-index.md).

Upgrading an existing installation? Read [the upgrade guide](docs/upgrades.md)
before replacing binaries when the index schema changes.

## Learn more

| I want to… | Guide |
| --- | --- |
| Narrow a search by project, provider, or date | [Search and filters](docs/search.md) |
| Connect or troubleshoot an agent | [Agent integrations](docs/agents.md) |
| See all commands | [CLI reference](docs/cli.md) |
| Configure an MCP client | [MCP reference](docs/mcp.md) |
| Understand indexing and models | [Indexing](docs/indexing.md) |
| Work on Mash | [Development](docs/development.md) |

[Documentation website](https://mashapp.dev) · [All guides](docs/README.md) · [Architecture](docs/agent-history.md) · [License](LICENSE)
