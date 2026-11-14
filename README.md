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
- Runs on **Linux, macOS, and Windows**. [Tested with →](#tested-with)

**Documentation website:** [getmash.io](https://getmash.io)

## Quick start

Start with Mash on one machine. This is the default setup: your agent histories,
index, and search service stay on that machine. You can add sharing later.

You’ll need some existing agent history and the local Qdrant search service,
which step 2 installs. Qdrant powers both word search and semantic search;
semantic models are optional.

### 1. Install Mash

macOS (Apple silicon) or Linux (x86-64 or ARM64), including WSL:

```bash
curl -fsSL https://raw.githubusercontent.com/kojah/mash/main/install.sh | sh
```

Windows x64, in PowerShell:

```powershell
irm https://raw.githubusercontent.com/kojah/mash/main/install.ps1 | iex
```

Both download a prebuilt release, verify its checksum and install to your
user’s `.local/bin` directory; add it to `PATH` if needed. No Rust, Docker or
administrator rights are required.

> The first release is not published yet. Until it is, these commands and the
> package managers below have nothing to download:
> [build from source](#build-from-source) instead.

Prefer a package manager?

| Channel | Command |
| --- | --- |
| Homebrew | `brew install kojah/tap/mash` |
| apt, Debian and Ubuntu | [add the Mash repository](docs/installation.md#package-manager-alternatives), then `sudo apt install mash` |
| Scoop | `scoop bucket add kojah https://github.com/kojah/mash`, then `scoop install kojah/mash` |
| mise | `mise use -g github:kojah/mash` |
| cargo-binstall | `cargo binstall --git https://github.com/kojah/mash mash` |

Already have a coding agent open? Paste this and let it do steps 1 to 3,
asking you before each download:

> Read https://getmash.io/setup.md and set up Mash on this machine.

### 2. Start the local search service

```bash
mash qdrant install
```

This downloads Qdrant (about 30 MB, checked against a checksum built into Mash)
and runs it in the background as your user, reachable only from this machine.
It starts again when you log in; Docker and administrator rights are not needed.
[Search service setup](docs/installation.md#start-the-local-search-service)
explains where it lives and how to remove it.

Prefer to run Qdrant yourself? Use [the official Docker image](docs/installation.md#optional-qdrant-docker-image)
or [start it by hand](docs/installation.md#run-qdrant-yourself).

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
Setup checks that Qdrant answers before changing anything; if it does not, run
`mash qdrant install`, or start your own Qdrant as the message shows. To read your chats before Qdrant runs, use
`mash init --no-index`, then `mash index`: indexing saves your history first, and
`mash list-chats` and `mash view` work without Qdrant. Search waits for Qdrant.

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

## Tested with

Where each agent's history has been indexed and searched so far:

| Agent | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Claude Code | Daily use | Daily use | Light use |
| Codex | Daily use | Daily use | Light use |
| GitHub Copilot | Light use | Light use | Recorded history only |
| OpenCode | Recorded history only | Light use | Simulated in CI |
| Antigravity | Recorded history only | Light use | Recorded history only |
| Cursor | Recorded history only | Partial | Recorded history only |

- **Daily use:** hundreds of real chats from an installed agent on that system.
- **Light use:** fewer than twenty real chats from an installed agent on that system.
- **Simulated in CI:** every push indexes a history written the way the agent
  writes it on that system, while it is being written.
- **Partial:** chats from Cursor's command-line agent read correctly from a real
  installation. Chats made in the Cursor editor are not read yet when they were
  saved by an older Cursor version.
- **Recorded history only:** the reader is tested on every push against history
  in the agent's format, but no installed copy has been indexed on that system.

The Linux and Windows test suites run on every push; macOS has no automated
tests yet. If an agent's chats are missing or look wrong on your system,
[open an issue](https://github.com/kojah/mash/issues).

## Build from source

For platforms without a prebuilt release, or to run an unreleased revision.
You need a stable Rust toolchain; the first build takes several minutes:

```bash
cargo +stable install --locked --git https://github.com/kojah/mash mash
```

This build leaves out the web dashboard’s pages.
[Install Mash](docs/installation.md#install-the-cli) shows how to include them.

## Documentation

The guides live at **[getmash.io](https://getmash.io)**: search and filters,
agent integrations, the dashboard, sharing across machines, configuration,
troubleshooting, and the CLI and MCP references.

Upgrading an existing installation? Read [the upgrade guide](docs/upgrades.md)
before replacing it.

[Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [License](LICENSE)
