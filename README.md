<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/mash-logo-dark.svg">
    <img src="assets/mash-logo-light.svg" alt="mash" width="360">
  </picture>
</p>

<p align="center">
  <a href="https://github.com/kojah/mash/actions/workflows/ci.yml"><img src="https://github.com/kojah/mash/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="https://github.com/kojah/mash/actions/workflows/windows.yml"><img src="https://github.com/kojah/mash/actions/workflows/windows.yml/badge.svg?branch=main" alt="Windows"></a>
  <a href="https://getmash.io"><img src="https://github.com/kojah/mash/actions/workflows/docs.yml/badge.svg?branch=main" alt="Documentation"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT licence"></a>
</p>

# mash - The **M**ulti-**A**gent **S**earch **H**istory Tool

**Find things you’ve already worked out with your coding agents.**

Mash searches your Claude Code, Codex, GitHub Copilot, Cursor, OpenCode, and
Antigravity chats. Ask your agent to find an earlier decision, search from the
terminal, or browse conversations in the dashboard.

## Features

- **Search across Claude Code, Codex, GitHub Copilot, Cursor, OpenCode, and Antigravity** from one place.
- **An agent memory layer** for recording decisions, facts and preferences, with links back to the chats they came from. [Conversation records →](docs/records.md)
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

Public release packages have not been published yet, so today Mash is built
from source with Rust. The first build can take several minutes:

```bash
cargo +stable install --locked --git https://github.com/kojah/mash mash
```

Every channel below installs a prebuilt binary without Rust, and starts working
with the first published release. Choose one.

| Channel | Command | Works |
| --- | --- | --- |
| Build from source | `cargo +stable install --locked --git https://github.com/kojah/mash mash` | today |
| Shell script, macOS and Linux | `curl -fsSL https://raw.githubusercontent.com/kojah/mash/main/install.sh \| sh` | from the first release |
| PowerShell script, Windows x64 | `irm https://raw.githubusercontent.com/kojah/mash/main/install.ps1 \| iex` | from the first release |
| Homebrew | `brew install kojah/tap/mash` | from the first release |
| apt, Debian and Ubuntu | [add the Mash repository](docs/installation.md#package-manager-alternatives), then `sudo apt install mash` | from the first release |
| winget | `winget install --id kojah.mash --exact` | from the first release |
| Chocolatey | `choco install mash` | from the first release |
| Scoop | `scoop bucket add kojah https://github.com/kojah/mash`, then `scoop install kojah/mash` | from the first release |
| mise | `mise use -g github:kojah/mash` | from the first release |
| cargo-binstall | `cargo binstall --git https://github.com/kojah/mash mash` | from the first release |

The two scripts are the recommended way to install a release. They verify
release checksums and install to your user’s `.local/bin` directory; add it to
`PATH` if needed. [Install Mash](docs/installation.md#install-the-cli) has the
details for each channel.

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

In the last command, use `lookup.view.id` from a search result or a
`list-chats` row (a message ID, not the chat ID). CLI results are JSON;
your connected agent can use the same operations through MCP.

Start with one distinctive word you remember. Without semantic models, multiple
words match a phrase: “log rotation” won't find “rotate the application logs.”
Try “rotate,” or enable [search by meaning](docs/indexing.md#enable-semantic-search).

Prefer browsing? `mash dash` starts a local, read-only [web dashboard](docs/dashboard.md).

To pick up new chats, run `mash index`; `mash index --watch` keeps the index
current. [Synchronization details →](docs/synchronization.md)

## Privacy

With the default local setup, indexing and search stay on your machine.
Sharing history across your machines is optional.

Mash masks recognizable credentials in results by default, but detection is
best effort. Local history and exports can contain sensitive text;
treat backups like the original chats. Mash never modifies the agent’s source
history. [Privacy and redaction →](docs/privacy.md)

## Documentation

The guides live at **[getmash.io](https://getmash.io)**: search and filters,
agent integrations, the dashboard, sharing across machines, configuration,
troubleshooting, and the CLI and MCP references.

Upgrading an existing installation? Read [the upgrade guide](docs/upgrades.md)
before replacing it.

[License](LICENSE)
