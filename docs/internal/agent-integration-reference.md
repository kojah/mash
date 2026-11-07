# Agent integration reference

[Documentation](../README.md) · [Project home](../../README.md)

Discover local setup and inspect agent wiring without requiring search readiness:

```bash
mash status --installation
mash status --installation --agent codex --agent claude-code
mash status --installation --agent codex --probe
```

The first command inspects local Codex and Claude Code user settings, installed
Mash plugin/skill files, bundled hooks, hook trust records and the stdio MCP
registration. Findings include the inspected path and a repair suggestion.
The `resources` section checks the selected local database's schema, lists every
supported provider with configured roots, root accessibility and recorded scan
results. Missing scan evidence is not a successful connection check.
Use `status --shared` for shared readiness and `status --coverage` for
retained provider coverage. Missing default history directories may simply mean
that provider is not used on this machine.
The report also includes `resources.silver` and flags pending, incomplete or
unverified history. A completed provider scan can coexist with older source
checkpoints; matching the binary's database schema alone does not certify them.
`--agent` explicitly requires an integration; without it, an enabled plugin or
an MCP registration establishes that expectation. A CLI-only installation is valid.
When an agent named with `--agent` needs attention, the command still prints the
report and exits with status 1, so scripts can check wiring. `summary.clients[].connected`
says whether each agent is configured with nothing left to fix, `missing` lists
the parts not installed (`mcp`, `plugin`), and `summary.next_action` names what
would make progress, or is null when there is nothing to do: `mash init` for a
missing MCP registration, or the agent's own plugin commands when only the
plugin is missing. Those need network access; Mash MCP search works without the
plugin, which adds the history skill and session hooks.
Ordinary `status` includes a compact installation summary, and `--help` (also
`help` and subcommand help) adds a short warning when local checks find an
incomplete integration or an incompatible/unreadable local index. Help respects
`--db`, remains JSON and does not start subprocesses. Schema checks never migrate
the index. A missing index alone cannot distinguish a fresh setup from an
interrupted upgrade; recover retained history before indexing after an upgrade
failure.
An upgrade marker is reported even if the active database is absent. Diagnostics
never clear it: first distinguish a running upgrade from an interrupted cutover
and recover the retained database/attachments before retrying.
Detected clients with no enabled Mash integration receive a softer setup notice
and appear in `summary.unconfigured`, without making CLI-only use an error.

Installation inspection works without a valid Mash configuration, index or
models. It never installs plugins, trusts hooks, modifies agent settings or
downloads models. `--probe` explicitly executes each configured user-level stdio
command with its arguments/environment, using a ten-second deadline for MCP
initialization and a Mash `help` contract check. Only probe commands run: hook
scripts and indexing/search tools are never invoked. A configured `--watch` is
not probed. Private launchers can have their own startup effects; inspect those
before probing. Arguments, environment values and child stderr are not printed.

These are observations of user settings on the current machine. Project overrides
are reported as unverified and suppress the user-level MCP probe; managed policy,
client profiles, remote clients and other agent integrations are not certified.
Multiple cached Codex versions are ambiguous, not evidence that a particular
version is active. A Codex trust record does not prove its hash still matches:
review current hooks with `/hooks`. A successful probe checks a **new** process,
not an existing agent connection or actual hook execution. Restart clients after
repairs. Inspection exits successfully when it produces a report; use
`summary.attention` and the individual states for automation.

Client-format references: [Codex plugins](https://developers.openai.com/plugins/build/plugins)
and [Codex hook trust](https://learn.chatgpt.com/docs/hooks).

`plugins/mash` packages a shared `history` skill for both clients. It prefers
Mash MCP, falls back to the CLI when MCP is unavailable, and guides chat
discovery, scoped message searches and evidence-backed answers about past
decisions. It uses the existing Mash connection rather than starting a duplicate
MCP server. The plugin requires a configured Mash executable/index; install and
connect Mash using the [installation guide](../installation.md). Its hooks add
session recall and prompt hints, and start a throttled
background indexing pass on session start. `MASH_AUTO_INDEX=off` disables that
pass. The plugin never downloads models.

`mash init` installs the plugin from the `kojah/mash` marketplace. From a
clone of this repository, install the local plugin instead:

```bash
codex plugin marketplace add .
codex plugin add mash@mash

claude plugin marketplace add "$PWD"
claude plugin install mash@mash
```

If Claude Code does not already have the Mash MCP connection, register it:

```bash
claude mcp add --scope user mash -- /absolute/path/to/build/debug/mash mcp
```

Use your private node-environment launcher as the command when credentials or
custom index paths are needed, as in [development setup](development.md#using-mash-in-codex-during-development). The skill uses
`mash-dev` when available for CLI fallback, otherwise the normal Mash executable.

Restart the client after installation. In Claude Code, invoke
`/mash:history`; in Codex, select the plugin's `history` skill or let it activate
for requests about past chats. For developing the Claude plugin directly,
use `claude --plugin-dir ./plugins/mash` instead of installing it. After edits,
reload the plugin or start a new session; installed copies may require a
marketplace/plugin refresh. Rebuilding the Mash binary and reloading skill files
are separate operations.

### Session recall

`plugins/mash/hooks/hooks.json` runs `session_start.py` on SessionStart
(`startup|resume|clear|compact`). It resolves the session's project (the Git
toplevel of `cwd` and its `origin` URL, matched against each chat's recorded
project and repository) and adds at most 2,000 characters of context: this
project's most recent agent chats with copyable `view` lookups, excluding the current session. It also calls local MCP `recall_records` for up to three established current project records, with a 750 ms sub-deadline and 4,096-byte response budget. It needs no models or current lexical views, and it fails open: any
error, missing executable or index, or its 3-second deadline yields no output and
exit status 0. The executable is `$MASH_BIN`, else `mash-dev`, else `mash`.

On UserPromptSubmit it also runs `prompt_recall.py`, which reads only the prompt
and never calls Mash. A prompt that looks back at earlier work ("yesterday",
"do you remember", "what did we decide", "where did we leave off", "the diagram
we made") gets a short note to search Mash history before answering or asking.
A prompt that names an outside person, PR, issue or ticket ("aaron's changes",
"PR 183", "#1394", "MASH-42") gets a softer note to search only if the agent does
not recognize it. Slash commands, pasted content and fenced code are ignored. It
fails open, and `MASH_PROMPT_RECALL=off` disables it.

On PreToolUse for `Read` it runs `file_context.py`. When the file is inside the
project and at least 1,500 bytes, it runs one local lexical
`search --unit chats --intent exact-mention` for the project-relative path,
scoped to the project's repository (else its path), and adds up to four earlier
chats that mention it, with a dated excerpt and `view` lookup, within 1,200
characters. Each file is shown once per session; the shown paths live in
`<recall state>/sessions/`, and session files older than a week are removed. It
can also show current records mentioning that exact relative path. Record hints
remain available when lexical views are unavailable. Atomic per-file claim markers
prevent duplicate hints from concurrent reads and distinguish files in different
projects. The hook has a 2-second deadline, fails open, and
`MASH_FILE_RECALL=off` disables it.

`MASH_RECORD_RECALL=off` disables record context in both hooks, retaining chat hints.

Hook throttling and per-session file hints live in
`<Mash data directory>/recall/` (`~/.local/share/mash/recall` on Linux;
override with `MASH_RECALL_STATE_DIR`). Throttling state is replaced atomically;
file hints use exclusive marker creation and skip repeat lookups once shown.
The scripts use only the Python 3.9+
standard library. Codex runs the same `hooks/hooks.json` once you trust the
plugin's hooks (`/hooks`); it sets `CLAUDE_PLUGIN_ROOT` for compatibility.

Check the hooks with:

```bash
cd plugins/mash/hooks
uv tool run --from ruff ruff check . && uv tool run --from ruff ruff format --check .
uv tool run ty check .
uv run --no-project python -m unittest discover -s tests
```

The manifests/catalogues follow the official
[Codex plugin format](https://developers.openai.com/plugins/build/plugins) and
[Claude Code marketplace format](https://code.claude.com/docs/en/plugin-marketplaces).

## Windows executable replacement

On Windows, long-lived
commands (`mash mcp`, `mash index --watch`, `mash-server`) move their running executable
aside (`.mash-running-<pid>.exe`) and leave a copy at the installed path, so
installers can replace `mash.exe` while an agent is connected; reconnect
afterwards. Between those two renames the installed path is briefly absent;
set `MASH_KEEP_EXECUTABLE=1` where Mash starts constantly, such as test
runners. Windows identifies the installed executable by size and modification
time, so a same-size replacement within one timestamp tick would not be noticed
as an upgrade. WSL works like Linux.
