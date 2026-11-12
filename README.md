<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/mash-logo-dark.svg">
    <img src="assets/mash-logo-light.svg" alt="mash" width="360">
  </picture>
</p>

# mash - The **M**ulti-**A**gent **S**earch **H**istory Tool

**Find things you’ve already worked out with your coding agents.**

Mash searches your Claude Code, Codex, GitHub Copilot, Cursor, OpenCode, and
Antigravity chats. Ask your agent to find an earlier decision, search from the
terminal, or browse conversations in the dashboard.

## Features

- **Search across Claude Code, Codex, GitHub Copilot, Cursor, OpenCode, and Antigravity** from one place.
- **An agent memory layer** for recording decisions, facts, preferences, and open questions, with links back to the chats they came from. [Conversation records →](docs/records.md)
- **MCP integration, plugins, hooks, and skills** so agents can look up past conversations as they work. [Agent integrations →](docs/agents.md)
- **Export and import indexed history**, including chats from different providers, for backups or moving between machines. [Backups →](docs/upgrades.md)
- **BM25 keyword search + vector search with Qdrant**, so you can search for an exact error or describe what you remember. [Search →](docs/search.md)
- **Search across machines** with `mash-server`, or keep everything local. [Multi-machine setup →](docs/shared-index.md)
- **Local embeddings by default**, with opt-in [remote batch inference](docs/configuration.md#embedding-inference).
- Runs on **Linux, macOS, and Windows**.

**Documentation website:** [getmash.io](https://getmash.io)

## Quick start

Start with Mash on one machine. This is the default setup: your agent histories,
index, and search service stay on that machine. You can add sharing later.

You’ll need some existing agent history and a running local Qdrant search service.
Qdrant powers both word search and semantic search; semantic models are optional.

### 1. Install Mash

Public release packages have not been published yet. For now, build with Rust:

```bash
cargo +stable install --locked --git https://github.com/kojah/mash mash
```

The first build can take several minutes.

**Recommended for published releases: the install scripts.** They download a
prebuilt binary without requiring Rust. These commands will work once the first
release is published:

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

Prefer a package manager? See [alternative installation methods](docs/installation.md#package-manager-alternatives)
for Homebrew, apt, winget, Chocolatey, Scoop, mise, and cargo-binstall.

### 2. Start the local search service

Run Qdrant directly on your machine—Docker is not required. Follow the short
instructions for [macOS/Linux](docs/installation.md#native-macos-or-linux-without-docker)
or [Windows](docs/installation.md#native-windows-without-docker), then leave it
running while you use Mash.

Prefer a container? [Run the official Qdrant Docker image](docs/installation.md#optional-qdrant-docker-image).

### 3. Connect your histories

Mash finds supported histories in their usual locations and indexes the last
90 days, including current sessions of supported agents. It also connects Mash
to the Claude Code and Codex CLIs it finds.

Word and phrase search works immediately. Want search by meaning instead?
Run `mash models install` **before** the setup command below to add the optional
local models. Already initialized? See [model setup](docs/indexing.md).

Prefer to choose folders first? Use `mash init --no-index`,
[edit the configuration](docs/configuration.md), then run `mash index`.
Otherwise, start the setup wizard:

```bash
mash init
```

In a terminal, Mash asks about local or shared search, history folders, coverage,
and agent integrations, then shows a confirmation before changing anything.
Choose **Just this computer** for the default local setup. Use `mash init --yes`
to accept defaults without prompts; non-terminal runs are also non-interactive.

Setup prints progress to stderr and a final JSON result to stdout. If the result
has `"agents": []`, no agents were connected; check `next` for the reason and
[connection instructions](docs/agents.md). You can still search with the CLI.
For a visual way to browse afterward, see [dashboard setup](docs/dashboard.md).

The setup result lists the provider folders selected;
[customize them](docs/configuration.md) if needed.
Restart any agents that setup connected, then try asking:

> “Use Mash to find where we discussed the database migration.”

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

Prefer browsing? [Build the web interface](docs/dashboard.md) for a source
installation, then start the local dashboard (chat history is read-only):

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
Sharing history across your machines is optional.

Mash masks recognizable credentials in results by default, but detection is
best effort. Local history and exports can contain sensitive text;
treat backups like the original chats. Mash never modifies the agent’s source
history. [Privacy and redaction →](docs/privacy.md)

## Configuration

`mash init` writes a TOML configuration. Use [the configuration guide](docs/configuration.md)
to choose history locations and search defaults.
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

[Documentation website](https://getmash.io) · [All guides](docs/README.md) · [License](LICENSE)
