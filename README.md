<p align="center">
  <img src="assets/mash-logo.png" alt="mash logo: a smiling bowl of mashed potatoes" width="200">
</p>

# mash

mash (Multi-Agent Search History) searches Claude Code, Codex, Copilot, Cursor,
OpenCode, and Antigravity histories
through a JSON CLI and an MCP server built with the official Rust SDK. Nodes can
keep a local index and optionally publish it to a shared service over HTTP.
SQLite FTS5, local FastEmbed embeddings, reciprocal-rank fusion, and bounded
cross-encoder reranking provide
hybrid retrieval. Nodes optionally publish to a joint shared index, one per
node group, so shared history remains searchable while source machines are offline.
The service keeps shared silver (published canonical history) in SQLite and derived
shared views (search documents and vectors) in Qdrant. Local SQLite is required for offline search.

Local gold (derived search views) uses SQLite FTS5 and exact cosine vector
retrieval. SQLite is the chosen local backend following evaluation of USearch
and Qdrant Edge: the additional snapshot, freshness, filtering and recovery
complexity is not justified by the demonstrated end-to-end benefit. Local ANN
adoption is out of scope; the shared service continues to use Qdrant.

## Quick start

Install a prebuilt binary (macOS Apple Silicon, Linux x86_64 or arm64, Windows x64):

```bash
brew install kojah/tap/mash
```

or

```bash
curl -fsSL https://raw.githubusercontent.com/kojah/mash/main/install.sh | sh
```

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

On Windows x64 (CI runs the library tests, setup, search, MCP, hooks and an
upgrade while MCP is running on a native Windows runner, and releases require
it), use one of:

```powershell
winget install kojah.mash
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

Then set it up:

```bash
mash init
```

`mash init` finds the Claude Code, Codex, Copilot, Cursor, OpenCode and
Antigravity histories in their default locations, writes `config.toml`,
registers the Mash MCP server and history plugin with the Claude Code and Codex
CLIs it finds on `PATH`, and indexes the last 90 days. Restart your agents and
ask them about past work. Without models search matches exact words (several
words must appear together, as one phrase); for
semantic search run `mash models install` (about 170 MB), then
`mash init --force`. `--no-agents` leaves agent settings alone, `--no-index`
skips indexing, and an existing configuration is kept unless `--force` is given
(the old file is saved as `config.toml.previous`).

The plugin keeps the index current: when an agent session starts it begins a
low-priority incremental `mash index` in the background, at most every ten
minutes (`MASH_AUTO_INDEX=off` disables it). For continuous updates run
`mash index --watch` yourself; Mash installs no daemon.

To build from source instead: `cargo +stable install --locked --git https://github.com/kojah/mash mash`.

## Privacy

Mash masks recognizable credentials in CLI/MCP search results, chat metadata,
and `view` output, shared HTTP read responses and outgoing publications, and text
sent to remote embedding APIs. Full values use `[REDACTED_SECRET]`; excerpts use
masking characters to retain their source offsets. Original provider histories
and local canonical SQLite data remain intact. Detection is best effort, not a
guarantee that arbitrary or obfuscated secrets are absent.

Text a person wraps in `<private>…</private>` (any case, closed in the same
message) is replaced by `[PRIVATE]` before it is stored: in native message
text, search projections, tool inputs and chat titles, and in connector titles,
content and metadata. Every index, view, export and publication derives from
silver, so none of them holds it, whatever the `[redaction]` settings. An
unclosed tag masks nothing. Mash cannot change the provider's own history files,
and retained text attachments are stored as submitted.

Masking is on by default and can be turned off per destination in the node
configuration:

```toml
[redaction]
output = true             # CLI/MCP search, view and chat listing
publication = true        # history uploaded to the shared server
remote_embeddings = true  # text sent to a remote embedding API
```

Turning `output` off shows canonical text, and lets search match inside
credentials. Changing `publication` republishes this node's whole history on
its next index, because masked and unmasked uploads must not mix. The shared
server has its own `[redaction]` table with `responses` (HTTP read responses)
and `remote_embeddings` (connector text it embeds remotely). Text attachments
are masked when they are retained, so they stay masked regardless of these
settings. Existing deployments
and stored shared history require an upgrade and explicit re-indexing; rebuilding
alone does not delete previous publications. See the
[redaction evaluation and limits](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/redaction-evaluation.md).

## Commands

SQLite query models are generated with sqlc 1.31.1 / sqlc-gen-rust 0.1.12.
Generated output is checked in; normal Cargo builds require no generator.
To regenerate as part of a build, install these versions and run
`make build CODEGEN=1` (add `CARGO_ARGS='--release --locked'` for release).
`make generate-queries` regenerates without compiling; `make check-generated`
checks for stale output. Generation reads the same SQLite schema SQL used at runtime.
CI checks regeneration against committed output. Dynamic predicates and FTS ranking
reuse the generated projection and decoder; migrations and writes remain explicit.

```text
mash init [--force] [--no-agents] [--no-index] [--marketplace SOURCE]
mash index [--full|--watch [--debounce-ms N]] [--provider claude-code|codex|opencode|copilot|cursor|antigravity]... [--since-days N|--all-history]
mash index --rebuild-local-views [--full]
mash search <query> [--unit chats|messages] [--local | --shared] [--limit N] [--offset N] [--provider PROVIDER] [--days N]
mash list-chats [--local | --shared] [--provider PROVIDER] [--kind KIND] [--title-contains TEXT] [--after UNIX_SECONDS] [--before UNIX_SECONDS] [--sort updated-at|created-at|title] [--direction asc|desc] [--limit N] [--offset N]
mash view <message-id> [--shared NODE_ID] [--context N]
mash status [--shared]
mash forget <chat-id>
mash export <new-directory>
mash import <export-directory> --into <new-database> [--keep-identity]
mash models install
mash mcp [--watch [--debounce-ms N]]
mash serve --server-config PATH
```

Chat discovery uses `list-chats` / MCP `list_chats`, `chats` / `messages`
search units, `--chat` / MCP `chat` filters, and `chat_id` in search and listing
results. Update clients and explicit `views.query.unit` settings and reconnect
existing MCP sessions after upgrading. Title-and-dialogue embeddings now use
`Chat:` with a new recipe identity; rebuild affected local views explicitly.
The shared views' chat state also uses the new names: replay shared views from silver
with `mash serve --server-config PATH --replay-views` when upgrading an existing
server. Canonical silver is retained.

Search results, chat matches and listed chats identify their origin with a tagged
`source`: `{"provider": "codex"}` for native history or
`{"connector": "<source_id>"}` for external connector records. Every result also
carries `object`, its core role and its source's kind label (for example
`{"role": "entry", "kind": "codex:message"}`), and `ancestors`: the containers
and segments enclosing it, nearest first. A native message's ancestors are its
turn (`codex:turn`, a prompt with the answers that follow it, derived from
silver) and then its chat. Chat-level results group under an
entry's nearest container, or a document on its own. Native messages keep
`role`; connector entries carry `author_kind` (`human`, `bot` or `unknown`)
instead, and documents carry neither. The local views schema is now version 4 and
silver version 55: existing local views are rebuilt from silver by the next
`mash index`, and silver from an earlier release crosses over through export and
`import --keep-identity`.

Silver compresses canonical text of at least 1 KiB with zstd when it saves space.
This applies to native providers and connectors in `objects`, and to retained
Claude source messages. Reads expose the original text through a generated
`content` column; raw SQLite connections need Mash's silver codec functions.
CLI, MCP and portable exports continue to return decoded canonical text.
Compression does not remove sensitive history from an export. Upgrading an
older silver schema requires export/import and a views rebuild; shared silver
must be republished with the new release.

One-shot data commands emit one JSON value; watch mode emits newline-delimited
results, and `mash mcp` serves the stdio protocol. `mash serve` runs the shared
index HTTP API; it reports its listening address as JSON on stderr and keeps
stdout empty. `mash --help` and subcommand `--help` return `{"help":"..."}`;
`mash --version` returns `{"version":"..."}`. These informational requests
exit successfully before loading configuration, models or indexes. Bare `mash`
returns a JSON usage error containing help with exit code 2. Global
options are `--config PATH`, `--db PATH`, and
`--models-dir PATH`.

CLI searches execute directly. MCP sessions reuse loaded models within their
process. Search never starts a background process.

## Using Mash in Codex during development

Point Codex at the build output so rebuilding does not require copying or
reinstalling the executable. Build with Cranelift in a stable target directory:

```bash
env CARGO_TARGET_DIR=/absolute/path/to/build cargo build-clif --bin mash
```

Add this to your local `~/.codex/config.toml`, or a trusted project's
`.codex/config.toml`:

```toml
[mcp_servers.mash]
command = "/absolute/path/to/build/debug/mash"
args = ["mcp"]
startup_timeout_sec = 30
tool_timeout_sec = 120
```

Mash uses its normal node configuration, SQLite index and model directory.
To override those paths, put `--config PATH`, `--db PATH` and
`--models-dir PATH` before `mcp` in `args`. Forward the configured token
environment variable using Codex's `env_vars`, or use a local launcher that
loads your private node environment before executing the same build output.
Keep credentials and machine-specific launchers outside Git.

`mash mcp` runs in one process. Restart it after installing a new binary or
changing startup settings, and reload updated plugin instructions. Accepted
mutations are never replayed. Opted-in watching lasts for the process lifetime.

MCP `help` provides live schemas and typed examples without an index or models.
Help/status report the running build revision and contract fingerprint; supply
the installed skill fingerprint as `expected_contract` to detect stale guidance.

The history skill's workflow template is `app/templates/history_skill.md`.
Its small live-help bootstrap and release stamp are generated from Rust with
`make generate-skills`; do not edit the generated `SKILL.md` or `contract.json`.
`make check-skills` checks generation, real argument validation and CLI examples.
Generation compares the contract stamp with the previous release and requires an increased
version in both plugin manifests whenever the contract or workflow changes.
Typed live-help examples also pass the real core validators. Update the installed
plugin and reload its instructions after changing the skill; consult live help
for current parameters instead of duplicating the API reference in the skill.

Development deployment uses `tools/build-dev-linux.sh` to reuse successful gates
for identical source/toolchain/build inputs while still stamping the release
binary for the current commit. Source changes invalidate the gate receipt, and
changes during validation or compilation fail the rollout. The API helper locks
concurrent rollouts and skips restarting an identical healthy release.
Ansible owns fleet selection, builds, API-first rollout, offline skips and
required schema migrations. `mash-dev-probe` verifies the installed build,
contract, query readiness and shared publication over MCP. Ordinary deployment
never ingests history or installs models. A silver upgrade runs one explicit sync
to re-ingest stale interpretation, rebuild local views and publish.

Use `bash tools/fleet.sh deploy shared-api xenia` with a private inventory.
Refresh remains explicit: `bash tools/fleet.sh sync xenia` or
`bash tools/fleet.sh local-views xenia`. See [deployment setup](deployment/README.md).
Runner logs and reports stay private. Skipped targets are not installed or ready.

## Codex and Claude Code history plugin

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
results, and lists locally committed connector delivery timestamps. Missing scan
or delivery evidence is not a successful connection check. Connector credentials,
live transport health and shared-only sources cannot be inferred from local
receipts; use `status --shared` for shared readiness and `status --coverage` for
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
connect Mash using the setup above. Its hooks are the read-only session recall
and prompt nudge below; the plugin never indexes history or downloads models.

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
custom index paths are needed, as in the Codex setup above. The skill uses
`mash-dev` when available for CLI fallback, otherwise the normal Mash executable.

Restart the client after installation. In Claude Code, invoke
`/mash:history`; in Codex, select the plugin's `history` skill or let it activate
for requests about past conversations. For developing the Claude plugin directly,
use `claude --plugin-dir ./plugins/mash` instead of installing it. After edits,
reload the plugin or start a new session; installed copies may require a
marketplace/plugin refresh. Rebuilding the Mash binary and reloading skill files
are separate operations.

### Session recall

`plugins/mash/hooks/hooks.json` runs `session_start.py` on SessionStart
(`startup|resume|clear|compact`). It resolves the session's project (the Git
toplevel of `cwd` and its `origin` URL, matched against each chat's recorded
project and repository) and adds at most 2,000 characters of context: this
project's recall records first, then its most recent chats with copyable `view`
lookups, excluding the current session. It reads only `list-chats --local` and
`view`, so it needs no models or current lexical views, and it fails open: any
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
needs current lexical views, has a 2-second deadline, fails open, and
`MASH_FILE_RECALL=off` disables it.

Distillation is lazy rather than tied to session end. Up to three project chats
active in the last seven days with messages newer than their watermark are listed
as undistilled; the agent is asked to read them, open its first reply with a short
recap and pass durable decisions, preferences, facts and open questions to
`plugins/mash/hooks/record.py`:

```json
{"records": [{"kind": "decision", "statement": "...",
              "evidence": [{"message_id": "...", "quote": "exact text"}],
              "supersedes": [{"id": "connector#entry#mash-recall#record/..."}]}],
 "covered": [{"chat_id": "...", "message_id": "newest message read"}]}
```

The helper checks every quote against `mash view MESSAGE_ID` and drops any record
with a quote that is not an exact substring. A `decision` or `preference` must
also quote at least one of the user's own messages (a native `user` turn or a
human-authored connector message): an assistant proposal alone is a suggestion,
and the helper drops it with that reason, so cite the user's approval or record
it as a `fact` or `open_question`. Survivors go to local silver through
`mash index --connector-batch` as connector source `mash-recall`: one
`recall:project` container per project and `recall:decision`, `recall:preference`,
`recall:fact` or `recall:open-question` entries whose text and metadata name the
project and the cited message IDs and quotes. They become searchable after the
next `mash index` (`mash search ... --source mash-recall`); like other connector
records they are not published by node publication. Watermarks advance only for
the covered chats, after delivery succeeds.

Records change through supersession rather than deletion. The optional
`supersedes` list names up to four earlier records of the same project, by record
ID (with or without the `connector#entry#mash-recall#` prefix) or by exact
`statement` (plus `kind` when it differs from the new record's). The new record
carries its own evidence and a `Supersedes: <id> (<heading>)` line; each replaced
record is rewritten in place as kind `recall:superseded`, its first lines becoming
`Superseded <heading>` and `Superseded by: <id> (<heading>)`, with its original
evidence and time kept. Replacements form chains (Biome, then oxlint, then oxlint
with type-aware rules): a record that is already superseded can only be replaced
again through its replacement, so the helper rejects self-supersession, cycles
and forks, and names that are missing or belong to another project. Within one
submission a record may supersede an earlier one. Re-recording is idempotent: a
superseded record stays superseded and keeps its links. The SessionStart hook
lists only current records; `view` of either record shows the neighbouring link,
and `python3 plugins/mash/hooks/record.py --history RECORD_ID` prints the whole
chain, oldest first, without writing. Each record lists the UTC days it was recorded on a `Confirmed:` line (the
latest ten), so re-recording a statement that still holds confirms it. The
SessionStart hook ranks current records by ACT-R base-level activation, the log of
the sum of each confirmation's age in days to the power -0.5, so a statement
confirmed repeatedly outranks one recorded once, recently; records written before
this line count their time as one confirmation. Before writing, the helper
compares statements with up to 64 current records around the project's latest
one: a statement equal to a stored one of the same kind apart from case,
punctuation and spacing re-records the stored statement, and a close rewording
(at least two shared words, IDF-weighted cosine of 0.8 or more, and no differing
word with a digit) is still written but listed under `similar` with the record it
resembles, to supersede or keep deliberately. Search the current records with the
current kinds, for example
`mash search oxlint --source mash-recall --kind recall:decision`, and their
history with `--kind recall:superseded`. Supersession is read and written
without a lock, like the watermarks: run one helper per project at a time.

Other agents record decisions the same way. An agent that already reads Mash
over MCP (for example a DeepSeek harness worker) pipes the same JSON to
`record.py --cwd PROJECT` through its shell, with `MASH_BIN` naming the Mash
executable, and recalls with MCP `search` filtered by `source` and the current
kinds. Evidence must cite messages Mash has indexed, so a decision taken in a
history Mash does not ingest is recorded only once its words appear in indexed
history.

The connector revision and
per-project watermarks live in `<Mash data directory>/recall/`
(`~/.local/share/mash/recall` on Linux; override with `MASH_RECALL_STATE_DIR`),
replaced atomically without a lock. The scripts use only the Python 3.9+
standard library. Codex runs the same `hooks/hooks.json` once you trust the
plugin's hooks (`/hooks`); it sets `CLAUDE_PLUGIN_ROOT` for compatibility.

Check the hooks with:

```bash
cd plugins/mash/hooks
uv tool run --from ruff ruff check . && uv tool run --from ruff ruff format --check .
uv tool run ty check .
python3 -m unittest discover -s tests
```

`MASH_RECALL_TEST_BINARY=/path/to/mash` additionally delivers records through a
real Mash into a disposable index.

The manifests/catalogues follow the official
[Codex plugin format](https://developers.openai.com/plugins/build/plugins) and
[Claude Code marketplace format](https://code.claude.com/docs/en/plugin-marketplaces).

## Models and indexing

Search is semantic by default. A `lexical-only` views build supports
`exact-mention`; semantic `related-discussion` requires `lexical-and-dense`.
Lexical retrieval defaults to `--match-mode flexible` (`match_mode: "flexible"`
in MCP/HTTP): prefixes of at least three characters and conservative typo
corrections work in both SQLite and Qdrant. Use `strict`, `prefix`, or `typo`
to select a narrower mode. Quoted whole queries retain exact spelling, and
numbers are never expanded or corrected. Expanded results include
`lexical_match` with the matched source term. Vocabulary expansion is scoped by
metadata filters and bounded to 100,000 vocabulary terms, 32 query terms and eight alternatives per query
term; a budget error asks for narrower filters or strict matching.
Shared exact-mention queries have a four-second execution budget, including
vocabulary expansion. Use `--match-mode strict` to skip the vocabulary lookup.
The CLI preserves recognized budget errors from the API with this guidance.
Install models explicitly, then build the local index:

```bash
mash models install
mash index
```

Initial indexing is explicit. CLI and MCP search never ingest history, build an
index, or migrate its schema. A missing or incomplete local index returns a typed
error recommending `mash index`, before loading inference models or contacting
the shared search service. An explicitly completed empty index is valid.

Database errors retain SQLite's original reason. Their optional `error.database`
object identifies the operation and includes numeric `sqlite_code` and
`sqlite_extended_code` when SQLite supplies them. `index-busy` remains the
retryable category for both `SQLITE_BUSY` and `SQLITE_LOCKED`; it reports lock
contention, not proof that an indexing writer caused it. For example, a blocked
status read can report `operation: "inspect-status"`, `sqlite_code: 5` and
`sqlite_extended_code: 5` alongside "database is locked". CLI stderr and MCP
structured errors carry the same evidence. No SQL statements or bound values
are added to this diagnostic metadata.

Local indexing separates durable canonical ingestion (silver) from rebuilding
built-in retrieval views, reusing the existing stored history. Lexical indexes,
dense embeddings and connector search units are built-in views: derived from
silver, certified against it and replayable from it. They are two SQLite
files: `mash.sqlite3` holds silver and the paired `mash.views.sqlite3` beside it
holds embeddings, the full-text index and readiness. Silver uses the same schema
as each node's history on the shared server. Views never hold history that
silver lacks: if it is deleted, damaged or belongs to another silver database,
mash replaces it with empty, unready views. `mash index --rebuild-local-views`
rebuilds it from stored silver offline, reusing valid vectors; add `--full` to
discard all local views and replay it from silver.

The built-in views rename is a breaking change. Replace `[gold.build]` and
`[gold.query]` with `[views.build]` and `[views.query]` in node and server TOML.
The commands now use `--rebuild-local-views`, `--activate-views` and
`--replay-views`; MCP uses `rebuild_local_views`, and the development MCP refresh
mode is `local-views`. Receipts expose `views_ready` and status uses `views`
fields. Old keys and flags are rejected; reconnect MCP clients after installing.
The paired database is now `.views.sqlite3` and shared Qdrant collections use
the `mash_views_` prefix. Rebuild local views from retained silver and replay
shared views with `serve --replay-views` before checking readiness. Retired
`.gold.sqlite3` files and `mash_gold_` collections are not read or converted;
they can be removed after verifying the rebuilt views.
Silver schema 53 stores the renamed live connector receipts. Use the existing
export/import upgrade for local schema 52 indexes. Portable archive version 1
retains its original receipt encoding, including its historical `gold_ready`
key, so exports from earlier releases remain importable; live receipts use
`views_ready`.
Update the history plugin to version 0.1.4 and reconnect Mash MCP after the
rollout. Older cached plugin instructions still use `list_threads` and
`unit: "threads"`; the current protocol uses `list_chats` and `unit: "chats"`.
Replacing the executable leaves already running MCP processes on their previous
binary, so those connections must be restarted before using the new schema.

There are no in-place schema migrations. A release opens only its own silver
schema and rejects any other untouched. To upgrade across a schema change,
export with the release that wrote the index, retire its files, and import with
the new release using `mash import --keep-identity`, which continues the node's
shared identity; `tools/upgrade-local-index.sh` does exactly this and is a no-op
when the schema is unchanged. Then rebuild views with
`mash index --rebuild-local-views`. The shared server never migrates either: an
empty silver directory is refilled by node publications and `--replay-views`.
`mash --version` reports the silver schema a release opens (`silver_schema`).
For development fleets, `tools/restart-dev-api.sh --retire-incompatible-silver
DIR` moves node databases the new server cannot open into its deployment log
and lists those nodes under `republish`. Their nodes then republish on their
next refresh. `tools/upgrade-remote-index.sh` runs the local upgrade on a
remote node before its binary is replaced. See the
[history pipeline](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/history-pipeline.md) for layer ownership, storage schemas
and recovery behavior.

`mash index` reports live progress on stderr, including file/vector counts,
elapsed time, and an estimated remaining time for the current phase. Estimates
use recent throughput and adjust as the workload changes; they are approximate,
especially when message lengths vary. Ingestion estimates cover the current
provider; shared-recipe embeddings estimate the current conversation.
Model loading, search rebuilding, snapshot preparation, and server
commit show activity with an unknown ETA until work can be measured. Shared
uploads report bytes and an upload ETA; completion is reported only after the
server acknowledges publication (or `shared-pending` if publication fails).
An attempt that skips malformed records ends with `incomplete` and the malformed
count, rather than `complete`; either incomplete state reports that a retry is
required. The CLI exits with status 9 and MCP sets `isError: true` while retaining
the structured partial counts. Correct the source or publication failure and run
`mash index` again; already committed usable data is preserved.
Source coverage and search readiness are reported separately. After a finished
scan, views can be certified over retained history even when some sources were
rejected; valid sources can keep advancing and remain searchable. Ingestion still
reports `incomplete` and exits with status 9 until the source failures are resolved.
Unfinished scans, obsolete interpretation and stale or incomplete search views
remain unready. `index --rebuild-local-views` can rebuild retained history offline
without repairing source coverage or changing the provider files.
Malformed JSONL sources contribute no readable fragments to the index. Their
last complete messages, vectors and checkpoint remain intact until a clean
retry; healthy sources in the same run can still advance.
After a malformed JSONL record, decoding continues to count further failures,
but normalization stops so broken fragment ancestry cannot hide that diagnostic.
For malformed Claude Code and Codex JSONL, `record_diagnostics` in the final
CLI/MCP result contains the first rejected record per source: provider, path,
one-based physical line, byte column, and `syntax`, `data`, or `unexpected-eof`
category. Samples are capped at 32 sources; `omitted_sources` counts additional
affected sources, while `malformed_records` still counts all rejected records.
These diagnostics also appear in progress notifications. Opt-in tracing records
a source fingerprint in place of its path, plus the provider, line, column and
category. Neither includes record content or raw decoder error messages. Final-result diagnostics
remain available when intermediate progress notifications are dropped.
For a recovery checklist that preserves source records and existing index data,
see [recovering incomplete indexing](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/index-recovery.md).
Interactive terminals show an indicatif progress bar or spinner; redirected stderr retains
newline-delimited JSON with `eta_seconds` (`null` when unknown) and `eta_scope`.
The final command response stays JSON on stdout. Watching remains opt-in with
`index --watch` in a separate process.

No other command downloads models. Indexing fingerprints sources and messages,
skips unchanged input, updates FTS incrementally for small changes, embeds only
new or changed searchable messages, and excludes tool commands, raw tool output,
reasoning blocks, and unknown roles from FTS and
embeddings while preserving it for `view`. `--full` rebuilds derived search
state. The default source horizon is 90 days; `--since-days N` overrides it and
`--all-history` removes it for one run.

Claude Code and Codex JSONL records are limited to 8 MiB of UTF-8 payload per
line, excluding LF/CRLF. Oversized Codex completed `FileChange` and
`CommandExecution` lifecycle mirrors are validated as JSON and UTF-8 while
streaming, then omitted; their canonical tool records remain indexed as context.
These envelopes have a separate 64 MiB input limit, including line endings.
Malformed envelopes, oversized canonical messages, and unsupported oversized
event types fail indexing. Oversized input fails indexing with a source-specific
error; it is never truncated or marked
complete. A failed configured provider scan retains its previous messages and
checkpoints. These are record limits, not a total conversation memory limit.
Codex SQLite history permits individual items up to 64 MiB because file-change
and tool-result items can exceed the JSONL limit. The 65,536-node structural JSON
limit still applies, and large normalized histories spill to disk. Tool bodies
remain canonical context rather than searchable dialogue.

Upgrading an existing index to schema 18 requires an explicit `mash index`.
The migration retains canonical history and existing vectors, preserves recorded
project/repository metadata, and invalidates source checkpoints and search
readiness so projections are refreshed before searching.

On Apple Silicon, indexing runs the FP32 embedding model through Core ML and
quantizes only the resulting vectors for storage. Batches select a 64-, 128-,
256-, or 512-token session using the model's tokenizer, retaining the 512-token
limit for long messages. Each shape compiles on first use and is cached on disk;
at most four sessions are retained during indexing. Other targets use the
quantized ONNX CPU model. After upgrading an existing Mac installation, rerun
`mash models install` once to install the FP32 asset and compile its Core ML
cache.

Core ML cache construction is serialized across processes for each shape.
If an SDK compilation fails, mash tries one fresh cache directory and publishes
it only after successful model construction. Loaded sessions retain their old
cache until they finish; retired caches are cleaned up when no reader uses them.
Indexing reports model-loading progress while waiting or compiling.

Embedding planning uses a temporary SQLite file to deduplicate and sort pending
texts, loading one inference wave at a time. Duplicate message IDs are written
in bounded pages. Large rebuilds therefore avoid retaining all pending texts in
memory; the model workspaces and parsed conversations have separate memory costs.

Parser admission limits concurrent estimated retention to a 128 MiB reservation
budget, keeping reservations through queued delivery and SQLite application.
Large sources run alone. Structured indexing progress includes admission counts,
peak reservations and wait time for diagnosing parser backpressure. This is an
estimate rather than a process-memory ceiling; a single large history may need
more memory.

Claude Code and Codex JSONL parsing uses a private, length-bounded file capture.
The parser keeps the original source path and message identities. Before accepting
it, mash verifies that the original file still contains the captured prefix,
using file identity and a content hash. Later appends can remain pending for the
next refresh; checkpoints describe the captured size and modification time.
Edits, truncations or replacement invalidate the old capture and require a fresh
read. Malformed captured records still reject that source; incomplete writes are
not silently dropped or certified. Other providers retain their stable-read
checks. Allow scratch disk space for concurrent input captures as well as
normalized staging: JSONL capture and verification add file I/O, without loading
the whole input into memory.

Normalized message payloads spill to private temporary files above a 16 MiB
inline budget. JSONL parsing, native Copilot CLI parsing and Claude reconciliation
feed this staging incrementally; local storage and shared uploads read it in
bounded batches.
Scratch files use `TMPDIR` and are removed when their owner finishes or fails.
Sensitive scratch SQLite stores use owner-only directories and files on Unix.
Checksums detect corrupt staging before publication. Metadata still grows with
message count, and some native adapters buffer their input before normalization.
See the [large-history staging measurement](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-01-normalized-message-staging.md)
for the measured memory and disk-I/O tradeoff.

Claude native ancestry spills to private scratch SQLite above a 2 MiB inline
accounting budget; parent-chain cycle detection retains only two UUIDs. Exact
parent states and compaction replay behavior are preserved, and scratch failures
abort ingestion of that source. Compaction duplicate membership and saved original
parents share a separate store with the same 2 MiB inline accounting budget;
restoration streams one entry at a time, and each finished or interrupted replay
block releases its scratch store. These budgets are accounting thresholds, not
process RSS limits. Ordered dialogue UUIDs and their first native positions use
a third private store with a 2 MiB inline accounting budget. Missing UUID slots,
duplicate first positions and forward ancestry references remain exact.
Reconciliation lineage, spool offsets, model workspaces and other adapters still
have separate memory costs.
The [ancestry staging measurement](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-02-claude-ancestry-staging.md)
records the whole-index memory reduction and indexing-time tradeoff.
The [replay staging diagnostic](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-03-claude-replay-staging.md)
separately compares large compaction metadata with its previous in-memory rules.
The [dialogue identity diagnostic](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-03-claude-dialogue-identities.md)
records its separate memory reduction and additional scratch-I/O cost.

Without a configuration file, mash indexes these built-in roots when present:

- Claude Code: `~/.claude/projects`, `~/.config/claude/projects`
- Codex: `~/.codex/sessions`, `~/.local/share/codex/sessions`, and each directory's sibling `thread_history_1.sqlite`

Claude subagent transcripts share their parent's native session ID. mash uses
the session and native `agentId` together to index each subagent as an independent
chat, so it cannot replace the parent's history. An explicit `mash index`
upgrades older indexes and reprocesses Claude source checkpoints to repair this
identity collision; search never performs the upgrade or indexing itself.
The schema 32/33-to-34 upgrade preserves existing vectors and search readiness;
reprocessing changed Claude transcripts updates their derived data normally.

Claude message UUIDs are scoped to their native session and agent, independent
of the transcript's physical path. SQLite retains each physical source's
contribution and derives one logical chat from their union. Event-only workflow
journals do not create chats or require
a single agent identity. Agent consistency is enforced when the source contains
retained dialogue, including identity supplied by metadata records. Matching copies
are deduplicated; divergent branches retain their own causal context. Rewinding,
emptying, or deleting one copy removes only that copy's contribution. Conflicting
payloads or ancestry for the same native message UUID fail transactionally.
Repeated metadata events with conflicting parents are ignored when no retained
dialogue depends on them; ambiguous events in a message's parent chain still fail.
Compaction may append reduced copies of earlier dialogue with rewritten parents.
Mash retains the original archived ancestry only for an uninterrupted block of
existing UUIDs followed by a native compaction boundary whose logical parent and
preserved tail identify the preceding event outside that block. Replayed message
ancestry can also survive a new native `stop_hook_summary` directly linked to
the last duplicate, when that summary is confirmed by the next boundary. Another
new event or duplicate block discards the pending replay evidence. Replayed message
content must still match; unrelated reparenting and incomplete replay evidence
fail transactionally. The native compaction boundary remains a context root.
Native forks may retain the original session ID on their copied history. When
the new session's dialogue explicitly links into that history, the whole copied
transcript belongs to the fork's native session; the original remains a separate
chat. Nested forks follow the same rule. Unlinked transitions and interleaved
session IDs fail rather than silently combining unrelated histories. Records
without a UUID retain path-based fallback identity.

Shared publication uses this same reconciliation. Changing old path-based message IDs can require
re-embedding Claude messages; unrelated providers' vectors remain intact.
Source snapshots increase SQLite storage, especially with many physical copies.

For pipeline diagnostics, set `MASH_TRACE=1`. mash writes JSON trace events to
stderr, including process/request IDs, request elapsed milliseconds, nested stage spans, span busy/idle timing,
safe error kinds, and HTTP status codes. CLI JSON and MCP messages remain on
stdout. Only mash's own trace targets are enabled; dependency logs are excluded.
Trace fields omit query/history text, paths, filters, credentials, and vectors.
Existing indexing progress events can also appear on stderr.

Index traces separate discovery and checkpoint filtering, parser admission waits,
parsing retries, queue sends, application, embedding planning and wave reads,
CPU/Core ML inference batches, vector writes, and commits. Admission events
include the estimated reservation, budget, elapsed wait, and whether capacity
forced a wait. Inference events report batch counts and Core ML sequence shape;
they never include the texts or vectors. Request context follows parser and CPU
inference threads through the same propagation helper.

For example, `MASH_TRACE=1 mash search cbioportal --local 2>trace.jsonl`
exposes model loading, embedding, lexical retrieval, vector loading/scoring and
reranking. Use `MASH_TRACE=1 mash mcp` for repeated requests or
`MASH_TRACE=1 mash serve --server-config server.toml` on the shared server.
Spans describe inclusive lifetimes, not CPU time; nested and concurrent timings
must not be added together. Request IDs are
local to a process, paired with process IDs. With tracing enabled, each shared
HTTP call sends an opaque `x-mash-trace-id`; its `shared_http_request` client
span and the server's `http_request` span record the same `trace_id`. Enable
tracing on both processes and match that field to correlate status and timing.
The server replaces missing, malformed or duplicate IDs with a fresh local ID;
client tracing disabled sends no correlation header.

For async task and resource inspection, build with `--features console` and
`RUSTFLAGS='--cfg tokio_unstable'`, then run that binary with `MASH_CONSOLE=1`.
Connect `tokio-console http://127.0.0.1:6669` to its loopback telemetry listener.
This is independent of `MASH_TRACE`; ordinary builds omit console instrumentation.
See the [performance profiling skill](.agents/skills/mash-performance-profiling/SKILL.md)
for isolated builds, remote access and measurement limitations.

Successful shared response bodies are decoded into typed models on blocking
workers so large JSON responses do not occupy an async runtime worker. The
`shared_http_decode` child span records byte count, `mode` and success/error, with the
same request ancestry and no response contents. HTTP request timing includes
admission, network I/O, blocking-worker queueing and decoding. In `buffered` mode
the child measures parsing; in `streamed` mode it also includes waiting for body
frames. Streamed byte counts include reader read-ahead. The request span records `queue_ms`, the fixed `endpoint` name and
`admission` class. Each process admits up to eight interactive requests and one
bulk state/publication request. They use separate capacity, acquired before
sending HTTP. The 5-second search, 2-second metadata/context or 30-minute bulk deadline
includes queueing and waiting for decoding. A canceled decoder retains its
capacity until the blocking work finishes; cancellation does not interrupt
already-running JSON parsing. This bounds concurrent response work, not response
bytes or the number of pending callers. Node state streams immediately; other
responses use the fast buffered parser up to a 1 MiB preview, then stream through
a 64 KiB reader. This removes whole-wire retention for large responses without
truncating histories. Canceling a streamed request wakes blocked body I/O; it
does not preempt CPU parsing. Owned typed results and upstream frame buffers still
consume memory. Large-history paging and owned-data budgets
remain tracked in `mash-c0d.21.4`.
See the [response decoding checks](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-03-shared-response-decoding.md)
for concurrency and relevance verification.
The [response admission checks](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-03-shared-response-admission.md)
cover queue expiry, cancellation, separate capacity and remaining memory limits.
The [streaming control](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-03-shared-response-streaming.md)
documents the large-state memory/parsing tradeoff and body cancellation checks.

OpenCode is opt-in through `[providers.opencode]`, with `roots` pointing to
`opencode.db` (normally `~/.local/share/opencode/opencode.db`) or its containing
directory. It reads sessions and project metadata from native SQLite snapshots,
including WAL changes. Each session becomes one chat. Sessions with
`session_message` records use that history; other sessions use the native
`message`/`part` tables. User and assistant text is searchable; reasoning, tools,
files, shell output, compaction and synthetic/ignored text remain context only.
Operational events and unknown part types are omitted. Session activity controls
the history horizon. OpenCode JSON records are checked for the 8 MiB byte limit
and a 65,536-node structural limit before typed decoding. OpenCode sessions have
a 64 MiB cumulative text-read budget and a 65,536-record limit; normalized
messages and individual legacy-message assembly have a 32 MiB retention budget.
Session metadata is capped at 8 MiB before fetching it. Exceeding these limits
fails indexing and preserves the previously indexed snapshot and checkpoint.
These parser budgets do not measure process RSS across concurrent workers.
The adapter follows the upstream
[database schema](https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/sql.ts)
and [message schema](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/session-message.ts).

GitHub Copilot CLI is opt-in through `[providers.copilot]`, with
`roots = ["/absolute/path/to/.copilot/session-state"]` or explicit `.jsonl`
event files. CLI directory discovery accepts native `events.jsonl` logs.
The adapter reads the v1 event schema: displayed human requests and complete
main-agent answers are searchable. Streaming deltas are omitted; augmented
model prompts and binary attachments are not indexed. Tools, reasoning,
injected prompts, autopilot continuations and subagent messages remain context
only. Each agent has a separate context chain, so interleaved subagents cannot
become neighbours of a main-agent search match. Clearing model context starts
a new chain while preserving earlier history; native file rewinds remove the
discarded messages on the next index run. Session IDs namespace message IDs,
including when a session is forked. The first settled logged workspace supplies
project and repository metadata; repository identifiers are kept as logged.
Missing headers, unsupported versions and incomplete records fail explicitly,
preserving the previous indexed snapshot until a complete refresh succeeds.
The native format follows the official [Copilot event schema](https://github.com/github/copilot-sdk/blob/main/nodejs/src/generated/session-events.ts)
and [VS Code history reader](https://github.com/microsoft/vscode-copilot-chat/blob/main/src/extension/chatSessions/copilotcli/common/copilotCLITools.ts).

Regular **Copilot Chat in VS Code** uses a separate adapter under the same
`[providers.copilot]` setting. Add VS Code's `User/workspaceStorage` and
`User/globalStorage` directories, or explicit `state.vscdb` files, alongside
the CLI root. On macOS these are under
`~/Library/Application Support/Code/User`; on Linux usually `~/.config/Code/User`;
on Windows `%APPDATA%/Code/User`. Insiders and custom user-data directories can
be configured the same way. Both formats can be indexed together:

```toml
[providers.copilot]
roots = [
  "/absolute/path/to/.copilot/session-state",
  "/absolute/path/to/Code/User/workspaceStorage",
  "/absolute/path/to/Code/User/globalStorage",
]
```

The read-only VS Code catalog selects local sessions, including empty-window
chats. Orphaned files and external CLI/cloud session entries are excluded.
Native JSON snapshots (including older plain-string responses) and JSONL
mutation logs are supported; logs take
precedence over retained snapshots. Edits, response updates and array rewinds
retain stable identities and remove discarded history on refresh. A partial
or invalid log preserves the previous indexed snapshot for retry. Draft input
is excluded. Human requests and answer markdown are searchable; hidden requests,
reasoning, tools and explicitly identified third-party participants remain
context only. Workspace and logged repository metadata support normal filters.
CLI and Chat session/message IDs have distinct namespaces. Both use the existing
`copilot` provider filter and per-index allowlists. Their upstream schemas are
the [VS Code session store](https://github.com/microsoft/vscode/blob/f5f73df304d9c5eeab64d4d07eed57e007742a92/src/vs/workbench/contrib/chat/common/model/chatSessionStore.ts)
and [typed mutation schema](https://github.com/microsoft/vscode/blob/f5f73df304d9c5eeab64d4d07eed57e007742a92/src/vs/workbench/contrib/chat/common/model/chatSessionOperationLog.ts).

Cursor Agent is opt-in through `[providers.cursor]`. Configure its native `chats`
directory or an explicit `store.db`. Its config root is `CURSOR_CONFIG_DIR`, then
`$XDG_CONFIG_HOME/cursor`, then `~/.cursor`; chat databases are below
`chats/<workspace-hash>/<session>/store.db`. The reader takes read-only WAL-aware
snapshots and follows the latest root's reachable archives and current messages.
Repeated requests retain distinct identities through archive moves and rewinds.
Tools, reasoning, model summaries, simulated prompts and subagent histories stay
context-only; current user messages require matching native turn evidence.
Native workspace URIs and tracked repositories supply project/repository filters.

Each blob is limited to 8 MiB, each protobuf to 65,536 fields, cumulative reads
to 64 MiB and retained normalized history to 32 MiB. Missing, corrupt or oversized
reachable history fails indexing and preserves the previous revision. These are
parser limits rather than process RSS limits. Cursor IDE is also supported through
explicit `state.vscdb` files or directories containing them. It reads the native
application KV schema and v18 composer roots, follows saved bubble order, and
keeps native bubble IDs stable through edits and rewinds. Drafts, summaries,
orphaned bubbles, simulated/agent-sent prompts and operational rows do not enter
search prose. Older or unknown root versions fail explicitly. Each JSON record
is limited to 8 MiB and 65,536 nodes; each conversation has a 64 MiB read budget
and 32 MiB retained-history budget. IDE and Agent identities remain distinct.
Its [native storage contract](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/cursor-ide-ingestion-contract.md) records the
fixture provenance and remaining captured-session validation gates.
The [Cursor ingestion contract](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/cursor-ingestion-contract.md)
records first-party fixture provenance and remaining real-session validation gates.

Antigravity IDE 2.5.5 is opt-in through `[providers.antigravity]`. Configure
`~/.gemini/antigravity-ide/conversations` or explicit `<cascade-id>.db` files.
Mash opens a read-only SQLite snapshot, including committed WAL changes; no
editor or language server is required. Repeated prompts have distinct native
step IDs, and surviving IDs remain stable after a history suffix is removed.
Displayed user prose and assistant responses enter search. Thinking, attached
text-block bodies, system steps, subagent context and copied fork history do not.
Supported reasoning and text-block attachments remain available in canonical
message views. Other attachment types, tool-step bodies and subtrajectories are
not currently reconstructed.

Each native BLOB is limited to 8 MiB, each conversation to 64 MiB of reads,
65,536 steps/decoded fields and 32 MiB of retained normalized history. Unknown
step formats, SQL/protobuf disagreements, malformed timestamps and oversized
input fail explicitly and preserve the previous index revision. Native workspace
and Git-root URIs supply project/repository filters when unambiguous. The
[Antigravity contract](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/antigravity-ingestion-contract.md) records native
writer/fork evidence and remaining real UI, successful rewind, CLI/Hub and
cross-version validation gates. These limits do not establish aggregate RSS.

For tree histories, `view --context N` includes up to N actual ancestors and
up to N messages along the first saved continuation from the match. It never
adds sibling replies because their file ordinals happen to be nearby. Contextual
reranking follows the same rule on SQLite and Qdrant. Linear histories
keep their existing ordinal neighbourhoods. SQLite migrations preserve existing
messages and vectors. Update the shared service before
publishing Copilot, Cursor or Antigravity histories from upgraded nodes.

Codex paginated history is read from an explicitly configured
`thread_history_1.sqlite`, with its sibling `state_5.sqlite` as a required metadata
dependency. Both databases are opened read-only and changes in either WAL invalidate
the snapshot. Built-in discovery includes these stores; explicit provider roots
must include them separately from `sessions`. Installation diagnostics warn when
a store exists beside a configured sessions directory but is not covered.
Thread IDs are preserved. For a paginated thread with stored items, the SQLite
source replaces only the JSONL mirror named by Codex metadata. Other JSONL sources
remain readable. Dialogue is searchable; reasoning, hooks and tool context remain
canonical context. Realtime transcript segments share the recorded rollout order;
promotion markers reference existing items without duplicating their content.
Unsupported item types reject the source
instead of silently replacing retained history with a partial transcript.
This reads locally available history; it does not fetch cloud-only pages or recover
records that Codex has removed. Keep exports when migrating old indexes.

Codex JSONL histories include raw response records and completed `UserMessage` /
`AgentMessage` turn items. Nearby raw/completed counterparts are paired once;
unmatched dialogue and separate repeated requests are retained. Started items,
streaming deltas and legacy event mirrors are omitted. See the
[native Codex contract](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/codex-ingestion-contract.md) for migration fixtures
and reconciliation limits. Existing nodes require an explicit `mash index`
refresh for schema 32 / `search-text-v6`; search does not migrate or ingest.

## Configuration

Supported providers are Claude Code, Codex, GitHub Copilot (Chat and CLI),
Cursor (IDE and Agent), OpenCode, and Antigravity. Removed provider names are
rejected in configuration, CLI arguments, MCP filters, and shared uploads.
Remove obsolete provider sections and allowlist entries before upgrading.
Explicit `mash index` upgrades local caches to schema 41, removing cached
histories and derived state for unsupported providers while retaining supported
histories and vectors. Original provider history files are never modified.
Shared indexes containing retired providers require a fresh shared group and
republication from upgraded nodes; legacy snapshots are not converted.


The rename changes the default configuration and data directories to `mash`,
and the database filename to `mash.sqlite3`. To reuse an existing achi installation,
pass its paths with `--config`, `--db`, and `--models-dir`, or move its configuration,
database, and installed models into the new locations reported by `mash status`.

mash reads `config.toml` from the platform mash configuration directory. Use
`mash status` to see the resolved path, or pass `--config PATH` explicitly.
The file is strict, versioned TOML. Provider presence enables that provider;
roots must be absolute. `index.since_days` defaults to 90. Set
`index.all_history = true` instead for all history; these settings cannot be
combined. CLI indexing flags override these settings.

Optional `[views.query]` settings select the default `unit` (`"chats"` or
`"messages"`) and `limit` (1–200). Omitting them preserves chats and 20 results.
CLI/MCP search parameters override either setting without rebuilding the index.
`[views.query.fusion]` controls hybrid candidate fusion independently of views
builds: `rank_constant` defaults to 60 (1–1000), and `lexical_weight` and
`semantic_weight` each default to 1 (1–100). Higher weights favor that candidate
stream before contextual reranking. `recency_weight` (default 50, 0–100) favours
newer messages in the final order: recency is ranked like another candidate
stream and combined with the relevance order, at that percentage of
relevance's weight, after reranking and before paging. A newer
message can overtake comparably relevant older ones, but a strong old match
still outranks a weak new one; undated messages get no recency credit and 0
ranks by relevance alone. These query-only settings apply to local,
combined and fallback searches, take effect on the next request, and require no
index rebuild. Exact-mention search is independent of fusion settings.
Set `[views.query] ranking = "fusion"` to keep fused hybrid ordering and skip
contextual reranking and its context fetches. The default is
`"contextual-rerank"`. This query-only policy applies across local, shared and
combined searches through the same CLI/MCP core, using the same typed requests.
It changes no stored vectors or silver. Dense query embeddings still require
the local embedding model or configured remote service. Fusion skips loading
the reranker. Local installation inventory validation remains required; remote
fusion needs no local model installation. Status reports `query_models` as
`"available"` or `"unavailable"` for current TOML query defaults,
separately from physical `models_installed`. Switching ranking reloads the
matching cached backend.
Use diagnostics to compare execution and results before choosing a default.
For one search, use `--ranking fusion` or MCP `"ranking":"fusion"`; the other
choice is `contextual-rerank`. Omission inherits current TOML. Explicit ranking
overrides require related-discussion intent, do not persist, and never rebuild
the index.
For one hybrid request, CLI `search` accepts `--fusion-rank-constant`,
`--lexical-weight`, `--semantic-weight` and `--recency-weight`; MCP `search` accepts a partial
`fusion` object with the corresponding underscore-named fields. Omitted fields
inherit current TOML defaults. Overrides are validated before model work,
apply to local/shared/fallback ranking, and are rejected for exact-mention.
For resolved policy and retrieval outcomes, use CLI `search --diagnostics` or
MCP `search` with `"diagnostics":true`. The optional report includes selected
artifacts, completed retrieval sources, fallback, result count and core time.
It does not run indexing or change search results.
See [silver enrichment and views configuration](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/silver-gold-configuration.md)
for the layer contract and current configuration scope.

```toml
version = 1

[providers.claude-code]
roots = ["/home/james/.claude/projects"]

[providers.codex]
roots = ["/home/james/.codex/sessions"]

[index]
since_days = 90
destination = "both"

[shared]
group_id = "personal"
node_id = "xenia"
api_url = "https://mash.example.net"
token_env = "MASH_NODE_TOKEN"
```

This example indexes both providers in SQLite and publishes that index to the
shared service. SQLite is required on every node. `index.destination` accepts `local` or `both`;
`both` requires the top-level `shared` connection settings. Without an explicit
destination, mash selects both when shared settings exist, and local otherwise.
Shared-only configurations are rejected. Both mode publishes exactly the local
index; the former per-destination `[index.local]` and `[index.shared]` provider
filters are rejected. Disable a provider entirely to keep it off the node.

Omit the top-level `shared` for a fully local installation. Search defaults to SQLite plus other nodes in the shared group when shared settings exist; otherwise it uses SQLite. Configure the same
group ID and API URL on every member, with a distinct node ID and token for each
publisher. The service authorizes each token for a fixed group and node. Group IDs select
separate Qdrant collections; every node within a group contributes to one joint
index. The database can live wherever you choose.

The former `nodes`, `local_node`, SSH destinations, `--local-node`,
and hidden federation workers are removed. To migrate a configuration, move this
machine's `providers` and `index` settings to the top level and optionally add
`shared`. Old configuration shapes are rejected rather than guessed.

## External connectors

All external connectors currently live in [blueprints](connectors/blueprints/README.md).
They are experimental and will move out individually after testing establishes
readiness. Their automated checks remain enabled.

[Obsidian vault ingestion](connectors/blueprints/obsidian/README.md) is a runnable external
connector through the Python SDK. It reads selected Markdown notes without
changing the vault, retains frontmatter and stable identities, and delivers to
explicit local and/or shared destinations. No Obsidian API key is needed.

Out-of-process connectors (see `sdk/python` and `connectors/`) deliver typed
batches of objects. Each object has a core `role`: a `container` (a channel,
thread or pull request) groups results; a `segment` (such as a review) is a
structural part of a container; an `entry` is searchable text with an author; a
`document` is standalone text. A namespaced `kind` such as
`discord:reply-thread` labels it, and a `parent` places it in the source's own
hierarchy: an entry may own a thread container, for example. Ingestion enforces
the role rules (only entries and documents carry text; entries and segments sit
under a container; documents contain nothing; no parent or reply cycles); kind
labels are free unless the source declares a schema. A `declare-schema` change
(SDK: `Changes.declare(KindRule(...), ...)`) lists the source's kinds, each with
its role, the kinds it may sit under and whether it may be top-level. From then
on ingestion rejects objects that use an undeclared kind, a different role or a
disallowed parent, and a new declaration is rejected if anything the source
already stored would violate it.
`mash index --connector-batch PATH` commits a batch to local silver and returns a
durable receipt with `views_ready: false`; it loads no models. Records become
searchable after the next `mash index` (or `index --rebuild-local-views`), which
indexes their text, embeds them with the configured model and recertifies local
views. Until then, local search reports views as stale rather than omitting them.

Connector results use namespaced anchors `connector#<role>#<source_id>#<id>`,
such as `connector#entry#discord:guild#message-1`; pass them to `view`, `chat`
filters and the history skill like native IDs. `view` shows an entry with its
chronological neighbours among the entries of its nearest container (segments
included), or a document on its own. Connector attachments are source
references, not captured files. `list-chats` lists connector containers, nested
ones included, unless a `provider` selection names native providers.

Connectors deliver to each destination they list: local silver, a node's shared
silver (`POST /v1/connector-batches`, authenticated as that node), or both. Node
publication never carries connector records. After committing a shared batch,
the server embeds changed records with its `--models-dir` model (or the
configured remote embeddings) and writes them to Qdrant alongside native
documents; a server without a model cannot certify that node's dense views. If
synchronization fails, the receipt still confirms durable silver and the node
stays uncertified until its next batch, upload or `--replay-views`, which also
rebuilds connector points from shared silver.

## Search filters

Search evidence includes `lookup.view` and `lookup.list_attachments`: copy these
argument objects into the corresponding MCP tool. They preserve local/shared
origin, including this node's own data returned by a shared-only search. Both
compact and extended results keep them.

For scoped history questions, use MCP `status` with an optional `coverage`
object containing `provider`, `node`, `after`, and `before`, or run
`mash status --coverage --provider claude-code --node xenia --shared`.
Coverage time selectors use message timestamps in Unix seconds; observed bounds
are Unix milliseconds. The report separates recorded provider scan state from
scoped retained-row counts. It never certifies absence or complete history:
historical scan bounds and source freshness are not recorded by these facts.
Shared aggregate certificates do not prove provider/time coverage; scoped shared
counts and provider scan details are `null` when unreported. Shared status failures
preserve local facts with `shared_error`.

The current models support English queries. Unsupported scripts and confidently
detected non-English queries return a usage error asking for an English query,
before loading models or opening an index. Short names and ambiguous technical
terms remain searchable; language detection cannot reliably classify every short
or mixed-language query.

`--sort recent` (MCP `"sort":"recent"`) orders the results that qualify newest
first, after relevance has chosen them; a chat sorts by its latest matching
message, and undated results come last. With paging, relevance chooses each
page and recency orders only that page, so pages never repeat or skip results. The default, `relevance`, keeps the
relevance order. Exact-mention search already breaks ties by recency.

CLI search accepts repeatable `--provider`, `--node`, `--role`, `--chat`,
`--project`, `--repository`, `--source` and `--kind`. Repeated values for one field are alternatives;
different fields are combined with AND. `--node` filters publishing identity and
does not connect directly to another node. A standalone local installation uses
the node label `local`. `--days` is a rolling message-age window. `--after` is
inclusive and `--before` exclusive; both accept `YYYY-MM-DD` (UTC midnight) or
RFC3339 timestamps with an explicit offset.

MCP exposes the same selection fields: `provider`, `chat`, `node`,
`role`, `project`, `repository`, `source` and `kind` are arrays, including for a single
value. `provider` and `role` name native history, so they never match connector
records; `source` names connector source IDs, so it never matches native history.
`kind` names object kinds from either: `codex:message` for native messages, or a
connector's own labels such as `slack:message`.
Connector records have no project or repository.
`after` and `before` use Unix seconds. Omitted or empty shortcut arrays impose
no restriction; an empty list inside a structured predicate, such as
`{"chat": []}`, matches nothing. Unknown chat IDs match nothing.
Use canonical `chat_id` values returned by chat search to drill down
into messages from promising chats:

```bash
mash search 'deployment' --unit chats
mash search 'rollback procedure' --unit messages \
  --chat THREAD_A --chat THREAD_B \
  --provider codex --provider claude-code
```

The corresponding MCP drill-down request is:

```json
{
  "query": "rollback procedure",
  "unit": "messages",
  "chat": ["THREAD_A", "THREAD_B"],
  "provider": ["codex", "claude-code"]
}
```

```bash
mash search 'database migration' --provider codex --provider claude-code \
  --role user --role assistant --after 2026-09-01 --before 2026-10-01
mash search 'deployment' --repository https://github.com/kojah/mash.git \
  --node dev-macbook --node xenia
```

For Boolean combinations, use `--filter` with a structured JSON expression, or
the MCP search tool's `filter` argument. This is a query expression; configuration
files remain TOML. Each expression contains exactly one operator:

- `all`: AND of expressions; an empty list matches everything.
- `any`: OR of expressions; an empty list matches nothing.
- `not`: negation of one expression.
- `node`, `provider`, `role`, `chat`, `project`, `repository`, `source`: lists of exact, case-sensitive values.
- `after`, `before`: inclusive/exclusive Unix timestamps **in seconds**.

An MCP request can select either repository while excluding tool messages:

```json
{
  "query": "database migration",
  "filter": {
    "all": [
      {"provider": ["codex", "claude-code"]},
      {"any": [
        {"repository": ["https://github.com/kojah/mash.git"]},
        {"project": ["/home/james/scratch/mash"]}
      ]},
      {"not": {"role": ["tool"]}}
    ]
  }
}
```

The CLI accepts the same expression through `--filter '...JSON...'`. Convenience
flags and the structured expression are combined with AND. Filters apply before
lexical/semantic candidate limits on SQLite and Qdrant, then the
combined results are reranked. Raw SQL, regular expressions, and glob patterns
are not supported. Limits are eight levels of nesting, 128 expressions, and 100
values per field list.

Project means the session's first recorded working directory. Repository means
the session's logged Git repository URL, or the locally available Git origin for
that directory at ingestion time. They are conversation metadata; the first
directory applies to the whole session. Unavailable metadata remains unknown,
so a positive filter does not match it and a negated filter does. Date filters do
not match messages without timestamps. Schema upgrades preserve history and
embeddings and invalidate local ingestion checkpoints, allowing the next
`mash index` to capture metadata from source files.

## Shared index

Nodes access the shared index through an authenticated Axum HTTP service. Only
that service connects to Qdrant. There is one
joint index per group, with node ownership enforced by the service.

`POST /v1/chats` lists chat metadata without model inference, message bodies
or vectors. Use the same bearer token as other shared API calls. Its typed JSON
request accepts `provider` and `node` arrays, literal ASCII-case-insensitive
`title_contains`, inclusive `after` and exclusive `before` activity bounds in
Unix seconds, and `exclude_self`. Activity refers to recorded chat `updated_at`;
unknown activity does not match a time bound. Omitted or empty arrays impose no
restriction. Sort by `updated_at` (default), `created_at` or `title`, with
`direction` set to `asc` or `desc` (default). Title ordering uses binary string
order. Missing sort values always come last; provider, chat ID and node break
ties deterministically.

The response contains `chats` with canonical `chat_id`, `source`,
representative `node_id`, title/project/repository and timestamps in Unix
milliseconds, plus `next_offset`. Copies with the same provider/chat ID are
deduplicated before paging; the best matching copy for the requested sort order
supplies metadata. `limit` defaults to 50 (1–100); `offset` defaults to 0
(0–10000). Pagination stops before advertising an offset beyond 10000; narrow
filters to browse larger collections. Pages are stable while the index is unchanged; updates between page
requests can shift boundaries.

Use `mash list-chats` or the MCP `list_chats` tool for the same metadata
discovery. These thin wrappers call the shared application core. The default
scope combines local SQLite and other nodes in the configured shared index;
`--local`/`local=true` stays offline, and `--shared`/`shared=true` reads only the
shared index including this node. Local and remote reads run concurrently, with
deduplication and ordering applied before the final page. A shared read has a
two-second total deadline; combined discovery returns local metadata with
`shared_error` on failure, while shared-only discovery fails. `shared` in the
response indicates a successful shared read.

Use `list-chats --diagnostics` or MCP `list_chats` with `diagnostics=true`
to include metadata-only artifact selection, requested/completed sources,
shared fallback and listing/merge counts and timings. Diagnostics make no extra
HTTP calls and require no models. `local-metadata` and `shared-metadata` measure
the complete source listing (including pagination); `metadata-merge` measures
deduplication, ordering and final paging. Candidate counts reflect the filtered
source windows, not the entire corpus. Concurrent timings overlap and must not
be summed as wall time. Listing elapsed time excludes startup/configuration
loading. Ordinary responses omit this optional report.

SQLite metadata reads require an existing current schema and never create or
migrate the database. They require no installed models or completed embedding
generation, so metadata already recorded during indexing can be inspected.
This does not make semantic search ready. MCP sort values use `updated_at` and
`created_at`; CLI values use `updated-at` and `created-at`.

For example, orient first and then search within a returned chat:

```bash
mash list-chats --provider codex --title-contains cbioportal --limit 10
mash search 'database migration' --unit messages --provider codex --chat CHAT_ID
```

### Tool calls and results

Claude Code and Codex messages record their tool activity as structured silver
metadata beside the unchanged flattened text. `view` (CLI and MCP, local and
shared) returns it as each message's `tools`; messages without activity omit it:

```json
{"tool_calls":[{"name":"Bash","call_id":"toolu_01A","input":{"command":"ls fixtures","description":"List fixtures"}}]}
{"tool_results":[{"call_id":"toolu_01A","is_error":false},{"call_id":"toolu_01B","is_error":true}]}
```

A message can carry several calls or results. `input` is the provider's arguments
kept verbatim as JSON: Claude `tool_use.input`, or Codex `arguments`/`input`,
which Codex records as a string. Results link to calls by `call_id` within the
chat and keep their output in the message text. `is_error` is present only where
the provider records it (Claude `tool_result.is_error`); Codex results carry
none. Other providers' tool activity is not yet structured. Tool text and
metadata stay out of search projections and embeddings, and output and
publication redaction mask credentials in every string of tool inputs.

This release changes the silver schema and the native interpretation version.
The upgrade helper exports and imports local silver; the next explicit
`mash index` replays every provider source to record tool activity, rebuilds
local views and republishes shared silver. History whose source files are gone
keeps its text without tool metadata.

The HTTP service stores shared silver in SQLite and shared views in
[Qdrant](#qdrant-shared-storage). Configure `qdrant_url_env` and `qdrant_grpc_url_env`; there is no backend
selector. See [shared storage](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/shared-storage.md)
for the publication sequence, recovery and cutover from Qdrant-only servers.

Each node has a different randomly generated token (at least 32 printable ASCII
bytes), supplied through its configured `shared.token_env`. Server group membership
comes from the `nodes` list; restart to change membership or rotate tokens.
A node can search its whole group and can only publish or forget its own data.
Database credentials belong on the HTTP server, supplied through environment variables.
Use an HTTPS reverse proxy for remote access or HTTP on a trusted private network.
The service binds to `127.0.0.1:8787` by default. Node clients disable redirects.

The API exposes `/v1/sync` (authenticated WebSocket mutation transport),
`/v1/freshness`, `/v1/search`, `/v1/contexts`, `/v1/chats`, `/v1/view` and the
attachment read routes. Nodes plan publication from their `/v1/freshness` entry
and forget through delta uploads. Requests use bearer credentials. `/v1/sync`
carries typed Protobuf envelopes with zstd publication data, durable piece
checkpoints and resume; a first publication of a large history is planned into
independently committed batches, so recent conversations become searchable
while the rest uploads; publication, connector-batch, attachment-upload and
progressive-backfill operations share one transfer protocol. The former
`POST /v1/index` (NDJSON) and `/v1/attachment-upload` routes and handlers were
removed in the protocol-matched release; `/v1/connector-batches` remains until
the WebSocket client returns the same source-scoped connector receipt. Search carries a query
embedding and returns candidates for node-side fusion and reranking. The HTTP
server needs no model assets. Interrupted or invalid uploads leave the visible
revision and freshness unchanged. Uploads are bounded by 128 records per batch,
64 MiB per event, 8 GiB per request, and the configured `max_upload_mib` (64 MiB
default, range 1–4,096). Ordinary JSON requests are limited to 64 KiB.
Node/revision checks reject stale uploads; full snapshot retries are idempotent.

Set `max_concurrent_uploads` (default 4, range 1–16) to bound concurrent publication
work; large histories should use 1. The same capacity also covers `forget`
mutations. A complete upload or accepted forget operation
keeps its slot until the work succeeds or fails, including if the requester
disconnects or times out. Cancellation does not imply rollback: retry using the
existing node/revision checks. Busy mutations return a retryable HTTP 503;
search and metadata reads do not consume mutation slots. This bounds the number
of mutations, not their total RAM.

Shared silver is the source of truth. A publication is validated and committed
to the silver SQLite database in one transaction, and only then synchronized into
Qdrant: documents are replaced in place per message and the node's views record
is certified for the new silver revision when every chat is written. Until
then the node's views are reported stale and excluded from dense search; there is
no Qdrant staging or conditional head protocol. A node that sees stale views
resends a complete snapshot, which repairs views without changing silver.
Thread lists, `view` and context reads come from shared silver.

If Qdrant data is lost or damaged, start the service with `--replay-views` to
rebuild every configured node's documents and vectors from shared silver before
serving. Replay embeds with the server's configured `[embeddings]` model space:
a remote endpoint when `url` is set, otherwise local model assets from
`--models-dir`. Replay is the only shared views rebuild; nodes never re-embed
shared history.
Benchmark actual search latency, update cost, memory use, and retrieval
quality for a group. See [the measured comparison](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-09-30-shared-index.md).

In both mode without destination filters, local indexing commits first. Changes
mark a durable local publication revision;
the service commits a changed node's upload to shared silver and then synchronizes its views. Raw tool outputs are uploaded
for context but remain excluded from search. Unchanged revisions are acknowledged
without replacing shared records. A failed upload returns local success with
`shared.state: "pending"`; subsequent `index` runs retry even with no new history.
`status` checks publication state locally; `status --shared` contacts the API.

`forget` deletes a locally owned conversation and publishes a persistent
shared tombstone. Check its `shared.state`: an offline deletion remains pending
until a later successful `index` or `forget`. Copies owned by other nodes remain.
In local/both modes, node identity is tied to the local database instance; a
recreated database must use a new node ID. Stale snapshots cannot replace newer
accepted revisions.


Default search queries SQLite and shared records belonging to other nodes. The
shared service excludes the authenticated node before candidate limits and
embedding-readiness checks. After computing one query embedding, local candidate
retrieval and the shared HTTP request run concurrently. Both sources use independent
lexical/semantic reciprocal-rank fusion, followed by bounded combined reranking.
Local message hits have no `node_id`; shared hits include their
publishing node for `view --shared NODE_ID`. Plain `view` always reads SQLite.
Shared candidate searches have a five-second HTTP deadline; metadata and context
reads retain a two-second deadline. Fetching nearby shared
context has a one-second total budget, preserving completed responses and using
available message evidence for the rest. Transport failures appear in shared
search status; combined search retains local results. Bulk publication and
retained-history operations have a separate thirty-minute budget, covering
upload, validation and views synchronization. The service uses the same bulk budget. A timed-out
or disconnected requester does not cancel an already admitted mutation: it keeps
its capacity until completion, and views are not reported current until synchronization completes.

Search defaults to `unit = "chats"` in CLI and MCP. Each chat result includes
its conversation ID, provider, representative node, available title/project/repository
and conversation timestamps, a ranking score, and up to three distinct matching
excerpts in relevance order. Each excerpt has a `message_id`, its own `node_id`,
role, timestamp, and relevance scores. Open an excerpt with `view MESSAGE_ID
--context N`, supplying `--shared NODE_ID` for a shared excerpt. Timestamps are
Unix milliseconds; unknown metadata is null. Search never returns an entire
conversation automatically. Every returned message `id` in a `view` response is
itself a view anchor: to keep reading, view the first or last returned message
with the same `--shared` value. `--context` has no upper bound and bodies are
never truncated.

Local lexical search treats query text as literal words, including URLs, dotted
names and hyphenated identifiers; double quotes require contiguous literal words
in each returned message or excerpt. Punctuation
does not become SQLite FTS syntax. Queries must contain a letter or number and
fit within 16,384 bytes. Explicit PR/issue references require that exact reference
in each returned excerpt/message, so `PR 180` cannot be replaced by `PR 183`
or an incidental numeric value of 180. PR and issue references remain distinct;
a complete reference URL also constrains its host and repository.
Use the metadata filters for Boolean scopes.
See [the search quality audit](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/search-audit-2026-09-30.md) for measured failures
and remaining relevance limitations.

Thread retrieval considers up to 200 lexical and 200 semantic candidates per
index. It keeps up to six distinct candidate messages per conversation. A first
reranking pass evaluates up to 20 grouped chat documents, then individual
evidence is reranked for the ten most promising chats (at most 60 messages).
Each evidence document includes bounded text from its two nearest searchable
neighbours. Threads are grouped before applying `limit`;
their score is reciprocal rank of the best qualifying message plus a bounded
bonus for up to two additional qualifying excerpts. The bonus is smaller than
the gap to the next message rank, so weaker matches cannot push a chat above
one with stronger primary evidence. Scores express relative relevance, not probabilities.
Lexical matches and verified exact PR/issue references remain eligible outside
the reranking budget. Other semantic-only
matches qualify only after being evaluated by the reranker; unreranked neighbours
do not fill out chat results. Raw model logits are not calibrated probabilities,
so a negative score alone does not disqualify a semantic paraphrase. A query may
return fewer chats than requested. Message mode applies the same evidence gate.
Recency does not change ranking; use date filters to restrict history.
Copies with the same provider and conversation ID are grouped across nodes;
identical role and whitespace-normalized message content earns no additional credit.
Replicas are collapsed before fusion and chat scoring; each excerpt's `copies`
lists alternate message/node references for viewing the same evidence. Independent conversations
with different IDs remain separate.

Use `mash search QUERY --unit messages` or MCP `unit = "messages"` for full
individual message results with the existing 50-candidate and ten-message
reranking budgets. Copies with the same provider, conversation, canonical message
ID, role and whitespace-normalized content occupy one message result, with
alternate view anchors in `copies`. Repeated content with different message IDs
and divergent copies remain separate. Replicas do not earn additional fusion
credit. `limit` counts the selected unit, and filters apply equally
to both modes before retrieval.

Search results are paged. `limit` (1–200) sets the page size and `offset`
(default 0) selects where the page starts in one final ranked list, which holds
at most 200 chats or 50 messages; an offset at or beyond that window is a usage
error. Responses include `next_offset`, the offset of the following page, or
null on the last page. Repeat the same search with that offset to continue.
Candidate retrieval, fusion and both reranking passes use fixed budgets that do
not depend on `limit` or `offset`, and qualification filters apply before the
page is cut, so for an unchanged index the pages of local, shared, combined and
exact-mention searches partition the same ranking that one large page returns.
Boundaries are best effort when the index changes between requests, or when a
combined search's shared source or shared context fetch fails for some pages
and not others (reported by `shared_error` and `shared_context_error`). Thread excerpts are bounded to 800 characters
plus ellipses, with a window around a query term when present. Results describe
the bounded candidate pool, not an exhaustive count of matches in each chat.
Unknown upstream metadata remains null.

Reranking uses batches of up to four documents grouped by length, reducing
inference overhead and padding between short and long candidates.
For messages longer than 2,400 characters, reranking uses a bounded window around
the first query term (or the start for semantic-only matches), keeping late lexical
evidence visible instead of always truncating the message's opening. `view` still
returns the canonical message.
Search projections exclude system/developer/tool messages, dedicated skill bodies,
and known injected instruction/context blocks. Ordinary Markdown examples remain
searchable. `view` retains canonical provider history. Existing candidates are
also filtered at query time; the next explicit `mash index` rescans source files
to refresh stored projections and titles. Shared-only indexing similarly refreshes
its source checkpoints once. Search never performs that refresh itself.

Use `search --local` to avoid the shared API, or `search --shared` for the complete
shared index, including this node. MCP search exposes equivalent `local` and
`shared` booleans. A candidate API failure during default search returns local results
with `realized_mode = "local-hybrid"` and `shared_error`; explicit shared search
returns an error. Later context failures retain the retrieved shared evidence,
`shared = true`, and the merged/shared mode, and report `shared_context_error`
separately. These source flags work independently of opt-in diagnostics.
Incomplete local embeddings still require `mash index`.
Models remain local and are installed only by `models install`.
Installation metadata is checked before loading models: the marker is limited to
1 MiB and 4,096 asset entries, revisions to 128 bytes, and each tokenizer/config
file to 16 MiB. Invalid paths, duplicate entries, wrong local dimensions and
missing or truncated assets return a model error recommending `mash models install`.
Hugging Face snapshot symlinks remain supported. Markers are published atomically
after installation checks, using a unique staging file for each installer.
Only one installer can run against a model directory at a time. A second
`models install` returns a retryable `model-busy` error (exit code 6); retry
after the active installer finishes. The operating system releases the lock
when the installer exits, so an interrupted process does not leave a stale lock.
Installers copy available assets into a separate generation, run SDK checks
there, then atomically select that generation. Failed or interrupted staging
keeps the previous installation available. Searches and indexers retain a lease
on their selected generation; an explicit installation collects abandoned and
retired generations once no reader holds them. The original flat cache is
retained so readers started before the first generation install can finish.
Reinstallation needs disk space for a second copy of the assets. This staging
also isolates mutable SDK revision references from the active cache.
Model downloads always use mash's selected model directory, even when `HF_HOME`
is set for another application. Set `--models-dir` to change mash's model location;
`HF_ENDPOINT` remains available for a Hugging Face mirror.
On Unix, installers flush the asset files and directories before activation,
then flush the marker's directory entry after activation. If that final flush
fails, the retryable error states that the installation is active but its
durability is unconfirmed. Both generations are retained; retry `models install`.
Activation errors also retain prepared assets until a later confirmed cleanup.
Other platforms retain atomic activation without these Unix flush barriers.

For unfiltered both mode, full snapshot replacement favors correctness and
simplicity over upload throughput. Large histories incur a full transfer on each changed revision, and
exact retrieval scores the matching shared vectors. Qdrant also supports opt-in HNSW.
The API spools each upload to a private temporary file and applies it to shared
silver in one SQLite transaction; temporary disk must hold concurrent uploads.
View synchronization then reads the committed upload and writes Qdrant documents
per chat. Account for concurrent uploads when provisioning the API.
Shared-service provisioning remains explicit. Measure corpus size, latency and
recall tradeoffs before choosing approximate retrieval.

### Qdrant shared storage

Configure `qdrant_url_env` and `qdrant_grpc_url_env` on the HTTP service. Point
publication uses gRPC; reads and collection setup use REST. Both endpoints must
address the same Qdrant instance. Nodes continue using the same
API URL, bearer token, CLI and MCP tools. Qdrant **1.19+ in the 1.x series** is
required; older versions are rejected before creating collections. Shared
silver lives on the API host under `silver_dir` (absolute path; default
`shared-silver` in the platform data directory), one SQLite database per
configured node at `<silver_dir>/<group_id>/<node_id>.sqlite3`, using the same
silver schema as local indexes. Back it up: Qdrant can be rebuilt from it, but
not the reverse. One `mash serve` process owns a `silver_dir`; a second one using
the same directory fails at startup. Filtered HNSW vector retrieval is the default, with
`ef_runtime = 200`; exact vector retrieval remains an explicit option.

```toml
version = 1
listen = "127.0.0.1:8787"
qdrant_url_env = "MASH_QDRANT_URL"
qdrant_grpc_url_env = "MASH_QDRANT_GRPC_URL"
# Optional for an authenticated Qdrant service:
# qdrant_api_key_env = "MASH_QDRANT_API_KEY"
# silver_dir = "/var/lib/mash/shared-silver"
max_upload_mib = 64
qdrant_memory = "durable" # Default: pinned HNSW/sparse indexes, cached dense vectors, cold payloads.

[[nodes]]
group_id = "personal"
node_id = "xenia"
token_env = "MASH_XENIA_TOKEN"

[views.query.vector_search]
algorithm = "hnsw"
ef_runtime = 200
# To request exhaustive vector retrieval instead:
# algorithm = "exact" # Omit ef_runtime with exact.

# Optional default for a configured group:
# [views.query.groups.personal]
# algorithm = "hnsw"
# ef_runtime = 80
```

The server's `views.query.vector_search` controls retrieval from the existing
dense index. Defaults are persisted per group. Use `mash serve --server-config
server.toml --activate-query` to intentionally change existing defaults; conflicting
startup settings otherwise fail. Running API processes read the selected policy
on subsequent hybrid searches. This does not rebuild vectors, change views build
identity, or modify silver. The previous
top-level `qdrant_vector_index` table is rejected; move it to the table above.
Groups inherit the server default unless named in `views.query.groups.GROUP`.
Status exposes the selected policy in `group_views.vector_search`. Node-side
`views.query` still controls intent, fusion and result shape independently.

For a single hybrid search, use `--shared-vector-search exact`, or
`--shared-vector-search hnsw --shared-hnsw-ef 80` (effort 1..4096, default 200).
MCP accepts `shared_vector_search: {"algorithm":"hnsw","ef_runtime":80}`.
Overrides affect only shared dense candidates and do not persist or rebuild
views. They require related-discussion intent and configured dense shared views;
local-only and exact-mention requests reject them. Omitting the override uses
the current persisted group default.

```bash
docker run --name mash-qdrant -p 127.0.0.1:6333:6333 \
  -v mash-qdrant-data:/qdrant/storage qdrant/qdrant:v1.19.1
export MASH_QDRANT_URL='http://127.0.0.1:6333/'
# Set MASH_XENIA_TOKEN to the node's existing API token.
mash serve --server-config /absolute/path/to/qdrant-server.toml
```

Shared views settings are persisted per group. Startup with conflicting build/model
settings fails. To deliberately replace the selected settings, update the server
TOML and start with `--activate-views --replay-views`, which re-embeds every node's
shared silver on the API host. Old API settings fail closed, and dense search
requires compatible manifests across the group. This preserves canonical history
but interrupts dense search during replacement. See
[the activation contract](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/silver-gold-configuration.md#durable-shared-group-configuration)
for publication races and remaining lifecycle gates.
Embedding dimension changes retain the same group and canonical history. Mash
prepares an additional named dense field in the existing collection, then the
same `--activate-views --replay-views` workflow selects and populates it.
The original dense field and lexical data are retained. Preparation alone does
not activate the new model; dense search requires current manifests after the
switch, while compatible metadata and lexical queries remain available.

Use an HTTPS origin and `qdrant_api_key_env` for hosted Qdrant. Endpoints cannot
contain credentials, paths, queries or fragments; redirects are disabled so keys
are not forwarded elsewhere. Qdrant credentials belong only on the HTTP server.
Populating a new shared group requires an explicit `mash index` from each publishing node.
`qdrant_memory` selects a durable Qdrant 1.19+ storage profile. The default,
`durable`, pins HNSW and sparse indexes in RAM, preloads dense vectors into the
OS cache and keeps payloads cold on disk; search reads payloads only for returned
hits. Qdrant 1.19 cannot pin dense vectors or payloads. Database files and the
write-ahead log remain on persistent disk. Cached dense vectors can still be
evicted under memory pressure; this is not a volatile RAM filesystem and does
not eliminate ingestion/optimizer disk writes.

The explicit `cached` profile preserves the previous placement: cached dense,
HNSW and sparse indexes with cold payloads. `cold` uses cold placement for those
components. Payload filter indexes retain Qdrant's own defaults. Pinning requires
enough RAM for the indexes, and optimizer conversion can temporarily increase
resource use.

Changing a profile on an existing group requires explicit activation:

```bash
mash serve --server-config /absolute/path/to/qdrant-server.toml --activate-memory
```

Mash validates the existing vector geometry, graph settings and payload schema
before updating placement. It preserves publications, canonical history, vectors
and retrieval policy; no reindexing is required. Activation can be rerun after
interruption. Subsequent starts with the selected profile omit the flag. Other
API processes must use the same configured profile when restarted.

Qdrant independently applies conversions through its optimizer, so the configured
policy can be visible before conversion finishes. Its
[memory-tier documentation](https://qdrant.tech/documentation/ops-configuration/memory-tiers/)
describes residency and low-memory behavior. Lower peak RAM and search latency
need workload-specific measurement.

Each group has an isolated message collection with named cosine dense vectors
and IDF-weighted sparse lexical vectors, plus a state collection for the group's
views configuration and each node's views certificate. Canonical history,
checkpoints, tombstones and chat catalogs live in shared silver. Lexical retrieval uses
lowercase alphanumeric tokens, hashed sparse term IDs, saturated term frequency,
and a fixed 256-token length reference; exact token predicates prevent hash
collisions from producing false lexical matches. This is a backend-specific
lexical ranking function; it differs from SQLite FTS5 ranking.
The existing application core still fuses, reranks, and groups message candidates.
All expressive filters apply inside Qdrant before candidate limits. HNSW is
approximate; exact mode stays available without rebuilding the collection.
Quantization is disabled. Foreign vector fields, incompatible vector settings,
or payload index types are rejected without deleting data. Dimension-specific
fields are prepared without changing the original vector space.

Each node's views record names the silver revision it was certified against.
Publication clears the certificate, replaces changed documents in place, deletes
removed chats, and then recertifies; a crash between those steps leaves views
stale rather than wrong-and-current. Activation replaces the group configuration
point; API processes with superseded settings fail closed. The 64 MiB upload
limit is configurable. Keep the silver database on persistent, backed-up storage.

## Remote embeddings

Local FastEmbed inference remains the default. Add `embeddings` to the node
configuration to use Voyage-4 for both document and query embeddings:

```toml
[embeddings]
api = "voyage"
url = "https://api.voyageai.com/v1/embeddings"
model = "voyage-4"
dimensions = 1024
token_env = "VOYAGE_API_KEY"
```

Set `VOYAGE_API_KEY` in the node's environment. It is separate from the shared
index token; the index service never receives it. History search projections
and search queries are sent to the configured inference endpoint. There is no
automatic fallback to another model. `model` is configurable, including
`voyage-code-4`; Voyage dimensions can be 256, 512, 1024, or 2048. Requests use
`input_type: document` for indexing and `input_type: query` for searching.
Overlong inputs fail explicitly rather than being silently truncated.

For an OpenAI-compatible endpoint, set `api` to `openai-compatible` and provide
its complete embeddings URL, model, dimensions, and optional `token_env`.
Set `request_dimensions = true` if the endpoint should receive the `dimensions`
parameter. Fixed-dimension self-hosted endpoints can omit it. Mash supports
1–2048 dimensions and validates vector count, ordering, dimensions, and values.
HTTP requests are asynchronous, bounded, and time out after 60 seconds. Local
SQLite pipelines remain on blocking workers while remote requests run on Tokio.

Remote indexing needs no local model assets. Reranking remains local; run
`mash models install` with remote embeddings configured to install only the
reranker before searching. No models are downloaded by indexing or searching.
If a valid local embedding installation already exists, installing for remote
embeddings preserves it. Switching back to local inference does not require
reinstalling those assets; the changed embedding space still requires indexing.

For shared indexing, configure the service's embedding space as well:

```toml
[embeddings]
model = "voyage-4"
dimensions = 1024
```

The service configuration applies to every group it serves. Publishers and
searchers must use the matching model and dimensions. Changing them rebuilds
local embeddings on the next index pass; reindex all shared publishers before
searching. For a shared model or dimension change, explicitly activate the server
settings and rebuild each member's shared views; a new group ID is unnecessary.
Model identifiers and dimensions prevent mismatched
queries from being compared with the stored embedding space.

## Automatic synchronization

The history plugin's SessionStart hook starts a detached, low-priority
incremental `mash index` at most every ten minutes and never waits for it, so
new sessions find a recent index; `MASH_AUTO_INDEX=off` disables it. A pass that
finds the database busy (another index or a watcher) exits without changes.

Run `mash index --watch` for opt-in synchronization while the process remains
running. It indexes once at startup, then waits for one second of quiet after
history changes before refreshing. Set `--debounce-ms 500` to change that delay
(50–60000 ms). Continuous writes trigger a pass within ten seconds, or the
configured delay if longer. Passes run sequentially; changes during indexing
queue another pass. Existing provider, age and destination settings still apply. `--full` cannot be combined with `--watch`.

Only one watcher indexes each database at a time, across all processes. Every
watching process (`index --watch`) first takes
an exclusive advisory lock on `<database>.watch-lock` beside the canonical
database path. A process that cannot take it becomes a standby: it neither
watches files nor indexes, and retries the lock every second. The operating
system releases the lock when the leader stops or exits, including a crash; the
standby that takes it over starts with a full initial pass, so changes made
while no process was watching are indexed. The leader's settings win: a
standby's `--debounce-ms` applies only once it leads. CLI watch mode reports each
role change as a `{"event":"watch-role","role":"leader"|"standby"}` line on
stderr. Explicit `index` calls do not take the lock.

The watcher uses native `notify` filesystem events, ignores file reads and
unrelated files, and notices JSONL creation, modification, replacement, and
deletion. It reloads configuration on each pass and watches for config/model
marker changes. Missing roots are watched through their nearest existing parent.
Removing an authoritative configured root reports an error rather than purging
its history; removing individual sources from an existing root is reconciled.
Failed indexing and pending shared publication retry after 30 seconds without
requiring another file change. A reconciliation pass every five minutes recovers
missed events. No models are downloaded automatically; every node retains SQLite.

Watchers use one local CPU embedding worker instead of the eight used by explicit
indexing. They keep the same embedding batches and vector format, trading bulk
throughput for lower memory use. The indexing model pool is reused during bursts
and released after 60–90 seconds without model work; the next update reloads it.
Idle release also applies to Apple Silicon's Core ML indexing pool. Allocators may
retain freed memory, so releasing the pool is not a process RSS limit. Search model
caching is separate from the indexing pool.

Each source read captures a fresh stamp and retries detected changes up to three
times, with 50 ms between attempts. Local and shared-only ingestion both use this
stable-read check. Stable malformed input fails without retries. If a source keeps
changing, indexing returns a retryable `source-changed` error recommending `index`;
an incomplete scan cannot authorize removal of missing histories. Local queued
results are checked again before writing. If the source changed while waiting,
the stale content is discarded and a new bounded stable read runs before any
content or checkpoint is replaced. A removed source still requires another
indexing pass.

Watch mode writes one JSON result per completed pass to stdout (newline-delimited
JSON); progress and typed errors go to stderr. Ctrl-C finishes the active pass
and exits. MCP does not watch history and rejects `--watch`, `--debounce-ms`, and
the retired `watch_start`, `watch_stop`, and `watch_status` tools. Run the CLI
watcher separately; all MCP sessions reading that database see its committed
updates without reconnecting. With no watcher, freshness advances only when an
explicit CLI or MCP `index` operation runs. No daemon is installed.

Long-lived MCP and CLI watch processes register locked records beside the
selected database in `<database>.runtimes`. Installation diagnostics list their
PIDs, running revisions, invocation paths and whether those executables still
match. This inventory covers registered processes for that database only; older
releases and other databases are outside its scope. A fresh MCP probe does not
verify an existing agent session.
An explicit MCP probe also registers its short-lived child; its record is removed
on exit, but an empty runtime directory may remain. It never indexes or downloads
models.
An unchanged executable from a different release is reported as `different_build`;
it must be reconnected explicitly because its own invocation path did not change.

After executable replacement, MCP rejects new mutations when they acquire the
session's mutation guard; CLI watchers check before each pass and once per second
while idle or standing by. Already admitted operations may finish. Reconnect MCP
or restart the CLI watcher after upgrading. This is an admission check, not
transaction fencing against an installer racing a check. Read-only MCP tools
remain available, but use the running release's schema and contract.

Run `mash status --shared`, or call the MCP `status` tool with `{"shared":true}`,
to inspect `shared_index.activity`: service-wide occupied/capacity counts and
live operations belonging to your authenticated group. Each operation reports
its node, kind, phase, elapsed time, phase counters, and time since its last
reported progress. Counters reset at phase boundaries and do not certify durable
commits. Long SQLite statements can run without intermediate updates; elapsed
time alone does not prove a stall. Detached server work remains visible after
its requester disconnects. Completed work disappears; server restart resets the
inventory. Older servers omit this field, which means unknown.
The `checking_search` phase checks collection health once and probes retrieval
after awaited document writes. Normal background optimization does not delay
publication: no collection-wide optimizer drain is required. Failed or paused
optimization and failed probes still return errors. Certification describes
applied search data, not an idle optimizer or a guarantee of query latency under
load. Older servers report `optimizing_views` while waiting for a drain.

When mutation slots are occupied, up to 32 authenticated requests wait in FIFO
order for at most five minutes before returning a retryable capacity error. Waiting
is reported as `waiting_for_capacity`, separately from receiving an upload or
waiting for the node's writer lock. The handler does not collect upload bodies
until admitted. Queue overflow is rejected immediately. Dropping a waiting
handler releases its queue position; disconnected sockets may remain waiting
until the server observes cancellation or the deadline expires. Once work is
admitted and detached, it retains its slot through completion. This bounds
waiting and active work independently; it does not increase publication capacity.

With `MASH_TRACE=1`, shared-operation start, phase transitions and end events go
to structured server logs, correlated by operation ID. Events include node IDs,
timings and enum phases, never history contents or credentials. Mutation outcome
events distinguish success from failure. Status is a point-in-time observation;
it does not queue work, increase concurrency or retry mutations.

Use that same status response
to inspect `shared_index.nodes` for every publishing node in the configured
group. Each node includes its revision, conversation/message counts, and UTC
timestamps:

- `last_synced_at`: last successfully committed indexing/publication check,
  including a check with no new data.
- `last_updated_at`: last committed data update or deletion. A check with no
  changed data preserves this timestamp.

The API records these timestamps in the node's shared silver head, inside the
SQLite transaction that applies the upload.
Failed uploads leave them unchanged. They describe received
index data, not the newest source message's timestamp or proof that a node is
currently online. Older nodes can have null timestamps until they publish with
this version. Use `mash status --shared` to include shared-index freshness;
ordinary status remains usable offline. Freshness inspection is
read-only.

`shared_index.unpublished_nodes` lists configured members of the authenticated
group that have no retained publication, in sorted order. An empty array means
every member configured on this API has published; an omitted field means that
membership scope was not supplied. Published nodes still require separate
lexical/dense readiness and provider-coverage checks. This reflects the API's
current credential configuration, not network discovery, liveness or proof of
complete source coverage; historical publications remain in `nodes` even if
their credentials are later removed.

## MCP

CLI and MCP are wrappers around the same typed application core in `app/core.rs`.
The core owns indexing, search, chat listing, view, status, forgetting, silver export/import, and model installation.
MCP calls it directly without constructing CLI arguments or invoking CLI handlers.
The watcher also uses typed core requests.

`app/core/models.rs` defines the in-memory requests and serialized responses.
Operation handlers live in `app/core/indexing.rs`, `search.rs`, `chats.rs`, `inspection.rs`
and `maintenance.rs`; each returns its concrete result type. The façade resolves
configuration and paths, dispatches the request and wraps the result once.
Indexing owns per-index provider routing, progress and watcher target selection;
inspection owns read-only status and canonical views. Parsing, SQLite and local
inference run on blocking workers, while shared HTTP operations remain asynchronous.

`app/config.rs` owns the typed node settings and status models. Its private
`document` module decodes strict TOML and validates provider routing, roots,
identifiers and endpoints; `loading` handles bounded file reads and platform
defaults. CLI, MCP and watcher operations use the same resolved settings.

`app/shared.rs` is the typed shared-index facade. Its private `client` module
owns pooled HTTP requests, authentication, deadlines and response decoding;
`publication` owns durable acknowledgements, routing ownership and streaming
uploads; `retrieval` owns concurrent local/shared candidates, fusion and contextual
ranking. The HTTP service and concrete database backends remain separate from
those node-side modules. These boundaries use existing concrete types rather
than adding transport or provider abstractions.

`app/shared/server.rs` owns Axum routing, authorization, typed HTTP responses
and the listener lifecycle. Its private `configuration` module loads bounded
strict TOML and resolves authorized node identities; `store` initializes Qdrant; `upload` spools bounded
NDJSON streams. Configuration and credentials are validated before connecting
to storage, and backend initialization completes before the listener opens.

Run `mash mcp` as a stdio MCP server. It accepts `--config`, `--db`, and
`--models-dir` at startup and exposes `search`, `list_chats`, `view`, `status`, `index`, `forget`,
`models_install`, `list_attachments`, `get_attachment`, `export_silver`, and `import_silver`. The server uses the same operations and typed JSON errors
as the CLI; tool failures set MCP `isError`.

All MCP tools present readable text alongside the
unchanged typed `structuredContent` JSON. Text includes copyable lookup arguments,
source attribution and partial errors; archived excerpts are quoted as evidence.
Clients may expose both channels, so this does not guarantee token savings.
`view` includes complete archived message bodies. Mutation and watcher tools
summarize their results and retry/next-action information; attachment retrieval
also retains native MCP image/resource content. Tool errors use readable text
and preserve typed error JSON and `isError`.
`search`, `list_chats`, and `status` accept `detail: "compact"` (default)
or `"extended"`. Compact search bounds message/excerpt text to 600 characters
(plus truncation markers), removes per-stage ranking scores and enrichment
provenance, and preserves result ordering, IDs, publishing node attribution,
copy anchors, pagination, partial-source errors and explicitly requested
diagnostics. Truncated text is marked with `content_truncated` or
`snippet_truncated`; call `view` to read complete messages. Extended returns the
existing full response, including retrieved message text, scores, provenance
and status manifests/configuration. `view` always returns complete message
text and accepts no detail selector. Extended text adds the full JSON response
after its readable summary. CLI output stays extended JSON.

`export_silver({"directory":"/path/to/new-export"})` saves local silver and
retained attachments offline, with a typed JSON manifest and checksums.
`import_silver({"directory":"/path/to/new-export","database":"/path/to/new.sqlite3"})`
imports to a fresh path and leaves views explicitly unready. It does not redirect
the running MCP session or overwrite existing history. See [silver exports](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/silver-exports.md)
for format, CLI commands, import/rebuild steps and data scope.

Attachment discovery uses a message ID from `view` or search:
`list_attachments({"message_id":"…"})`, then
`get_attachment({"message_id":"…","attachment_id":"…"})`.
For shared evidence, pass `shared` with its publishing node ID to both tools.
Retrieval returns metadata plus native MCP image content for PNG/JPEG/GIF/WebP,
or a base64 embedded resource for other files. Local `view` also includes attachment
metadata. No models, URL downloads, OCR, or arbitrary file reads are involved.

The initial adapters capture Claude image/document blocks, Codex raw and completed
[image inputs](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/user_input.rs),
and OpenCode file parts. Other providers' attachment formats are not
yet captured. Re-run `mash index` with this version to replay older interpretation
checkpoints. Attachment names/markers stay out of semantic dialogue projections.
Up to 32 attachments per message and 4 MiB per retained file are supported.

Embedded base64 files are retained automatically. Explicit local file references
require allowed roots in node TOML; missing or disallowed files retain metadata:

```toml
[attachments]
allowed_roots = ["/absolute/path/to/provider/attachments"]
share_files = false
```

Changing allowed roots triggers replay during the next explicit index operation.
Local copies live beside SQLite in `<database-path>.attachments`, keyed by content
hash, and work offline even after the provider file disappears. Back up this
directory with SQLite. UTF-8 files are retained and returned as credential-redacted
views; binary images/PDFs are not sanitized. Attachment bytes do not enter Qdrant.

For optional shared files, set `share_files = true` on the node and configure an
absolute `attachments_dir` in server TOML. Nodes upload only files linked to their
published messages, after canonical publication; failed uploads leave indexing's
shared result pending for retry. Metadata without uploaded bytes reports
`not_uploaded`. Storage is isolated by group/node, and every read checks current
published message membership. Removed attachments become inaccessible; orphaned
content-addressed files currently remain on disk until the cache is removed.

To stream an `index` tool call's progress, include a string or numeric
`params._meta.progressToken` in the `tools/call` request. The server sends
standard `notifications/progress` with the same token while the job runs,
including when it waits for another mutation. `message` contains the phase,
counts, elapsed time, and phase ETA. Structured progress fields are also available
under `params._meta["dev.jamesqo.mash/index-progress"]` in each notification.
The numeric `progress` is an increasing update counter across all phases;
`total` is omitted because the whole job's work is unknown. Phase-specific
totals and `eta_seconds` are in the structured metadata. Unknown ETAs are `null`.

Progress delivery uses a bounded queue and skips intermediate updates if a
client is slow; a disconnected client does not block indexing. The normal
tool result follows progress and retains the CLI's success/error structure.
Without a progress token, the tool returns only its final result. CLI progress
still goes to stderr; MCP stdout remains exclusively SDK protocol messages.

Example MCP client configuration:

```json
{
  "mcpServers": {
    "mash": {
      "command": "/absolute/path/to/mash",
      "args": ["mcp", "--config", "/absolute/path/to/config.toml"]
    }
  }
}
```

For shared operations, provide the configured node token environment variable to
`mash mcp` through your MCP client's environment mechanism. `search` accepts a
`shared` boolean, a `unit` of `chats` (default) or `messages`, and `limit`/`offset` paging with `next_offset` in the response; `view` accepts a `shared` publishing node ID. Search, view, and
status and `list_chats` are annotated read-only. Index and forget mutate data; `models_install`
is the only operation that downloads models. Shared node I/O uses async HTTP;
service-side Qdrant I/O uses async HTTP. SQLite, parsing, and local FastEmbed inference run on blocking
workers and are awaited by callers. MCP mutations serialize per session; status
and view may run concurrently, while model-heavy operations use a single permit
to bound memory and CPU use. Remote embeddings use the same node configuration
for CLI and MCP operations.

## Code organization

CLI and MCP translate inputs into typed application-core requests. Implementation
modules are grouped by responsibility:

- `app/silver/`: canonical models, typed provider records, history discovery,
  normalization and source reconciliation; `app/silver.rs` coordinates ingestion
  and checkpoints. Provider-owned history files are bronze inputs.
  `app/silver/dialogue.rs` interprets visible dialogue independently of retrieval;
  canonical messages retain full context alongside an optional `dialogue_text`.
- `app/silver/local.rs` and `app/views/local.rs`: local stage boundaries for
  durable canonical ingestion and rebuildable retrieval state. The application
  core wires those stages together.
- `app/views/shared.rs`: node-side embedding and publication-payload materialization
  from interpreted silver conversations; source scanning stays in the shared
  publication coordinator.
- `app/semantic/` and `app/search_*`: views model execution, embedding batches,
  Core ML, remote embeddings, hybrid retrieval and query/ranking policies.
- `app/shared/snapshot/`: backend-independent publication validation; `layers.rs`
  defines persisted silver history and views embeddings within one atomic revision.
- `app/views/qdrant/`: common typed REST reads/setup, gRPC document writes, collection schema,
  and dense/sparse retrieval. The engine takes an index scope, independently
  of Axum authentication; shared publication owns coordination around it.
  `Store::open` inspects existing collections without writes; `Store::connect`
  explicitly completes setup and then verifies the same reader contract.
- `app/views/documents.rs` and `app/views/retrieval.rs`: common document and
  retrieval contracts, also used by the shared HTTP protocol.
- `app/views/materialize.rs`: native silver-to-Qdrant text preparation,
  inference and bounded document writes. Callers own silver snapshots, mutation
  coordination and final certification.
- `app/tests/`: CLI contract tests grouped by behavior, with common setup in
  `support.rs`. CLI and MCP share marker fixtures from `app/test_support.rs`.

Keep assertions in individual tests. Share setup for repeated fixtures, and
split independent cases so each failure names the behavior that broke.

## Development

Agent work is tracked with [Beads](https://github.com/steveyegge/beads). Issue
history lives in a separate, private Dolt remote (`kojah/mash-beads`), not in this
repository. With `bd` and Dolt installed and access to that remote, run
`bd config set sync.remote git+https://github.com/kojah/mash-beads.git` and
`bd bootstrap` after cloning, then `bd prime` and `bd ready`. `bd dolt commit` and
`bd dolt push` persist tracking updates. The entire `.beads/` directory, including
configuration, stays local and is ignored by Git.

## Verification

[Correctness checks](docs/verification.md) describes generated Proptest
sequences against real SQLite, disposable-backend replay interruptions and
focused cargo-mutants audits. Property tests run with ordinary nextest tests;
mutation audits run explicitly with `make mutation-tools` and `make mutation-check`.
Run `make property-check` for generated SQLite tests. With disposable
`MASH_TEST_QDRANT_URL` and `MASH_TEST_QDRANT_GRPC_URL`, run `make property-backend-check` for generated interrupted
connector writes and `make mutation-connectors` to audit their regressions.

Bounded TLA+ checks for publication, connector receipts, tombstones and shared
view recovery live in [protocol/](protocol/README.md). Run `make protocol-tools`
once to install the pinned checker, then `make protocol-check`. These checks
include explicit expected counterexamples, including the historical replay
certification bug; they supplement implementation tests rather than certify
the Rust implementation.

Use Cranelift for development builds and tests. Install the nightly component
and use the Cargo aliases:

```bash
rustup component add rustc-codegen-cranelift-preview --toolchain nightly
cargo build-clif
cargo run-clif -- status
cargo test-clif
```

These aliases select Cranelift for mash and mash-query in the development profile. Dependencies
keep LLVM, so their existing build cache can be reused. `test-clif` requires
`cargo-nextest`. Use the usual commands below for standard LLVM verification
and `cargo build --release` for optimized builds. Cranelift is an experimental
nightly component and is available only on supported host platforms; see the
[Cranelift backend documentation](https://github.com/rust-lang/rustc_codegen_cranelift#usage).

```bash
cargo fmt --check
cargo nextest run
cargo clippy --all-targets -- -D warnings
cargo test --doc
```

The handwritten workspace includes the application and the pure `mash-query`
crate. These default commands cover both. For an isolated query edit cycle,
use `env CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif -p mash-query`.
See [crate boundaries](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/crate-boundaries.md) for responsibilities and
the integrated checks required when changing search behavior.

For repeated unit-test edits, keep one target directory and enable incremental
compilation explicitly:

```bash
env CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif --lib
# Add a Nextest expression to run the affected unit tests:
env CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif --lib -E 'test(claude_copies::)'
```

On Xenia, warm incremental rebuilds after a small test edit took 7.7–8.6 seconds,
compared with approximately 32–35 seconds without incremental compilation.
The first incremental build took 41.7 seconds and created additional disk cache.
The empty `RUSTC_WRAPPER` bypasses a locally configured compiler wrapper; the
installed wrapper failed on this incremental build. `--lib` selects unit tests
and does not run CLI integration contracts. Keep the full suite, real-model and
shared-backend checks as completion gates. See the
[measured workflow](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-09-30-test-turnaround.md) for scope and limits.

CI also runs the [labeled hybrid relevance matrix](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-01-shared-relevance.md)
with explicitly installed models against local SQLite and shared Qdrant. The
report retains historical measurements of the removed backends.
The shared tests require their disposable backend endpoint and
`MASH_TEST_MODELS_DIR`. To run these checks together with those variables set:

```bash
cargo test-clif --lib --run-ignored only -j 1 -E 'test(labeled)' --success-output immediate
```

The installed August 2026 nightly can hit a [Clippy compiler crash](https://github.com/rust-lang/rust/issues/161495)
when checking async code. On that toolchain, use
`RUSTFLAGS=-Znext-solver=no cargo clippy --all-targets -- -D warnings`.

The storage integration tests require disposable Qdrant 1.19+ in
`MASH_TEST_QDRANT_URL` and its gRPC endpoint in `MASH_TEST_QDRANT_GRPC_URL`.
Tests start real HTTP services and create/remove isolated
collections for their owned groups:

```bash
MASH_TEST_MODELS_DIR='/path/to/explicitly-installed-models' \
  MASH_TEST_QDRANT_URL='http://localhost:6333/' \
  MASH_TEST_QDRANT_GRPC_URL='http://localhost:6334/' \
  cargo nextest run --run-ignored only --no-tests fail -j 2 -E 'test(qdrant)'
```

The real-model integration tests are ignored by default and require
`MASH_TEST_MODELS_DIR` pointing at assets created by `mash models install`.
The shared-backend selections include Claude copy reconciliation with real
embeddings and require that directory too. Selected tests fail when prerequisites
are missing; `--no-tests fail` rejects an accidentally empty selection.

For an instrumented coverage report, install
[cargo-llvm-cov 0.9.1](https://github.com/taiki-e/cargo-llvm-cov/releases/tag/v0.9.1)
and the toolchain's `llvm-tools-preview` component. Set the model directory and
the Qdrant test URL above to installed models and disposable services.

Also set `MASH_TEST_SERVER_BINARY` to the instrumented `mash` binary produced
in the coverage target's `debug` directory. With an explicit `CARGO_TARGET_DIR`,
use `$CARGO_TARGET_DIR/llvm-cov-target/debug/mash`; without it, the default is
`target/llvm-cov-target/debug/mash`. An explicit `CARGO_LLVM_COV_TARGET_DIR` instead
uses that directory directly, as defined by [cargo-llvm-cov 0.9.1](https://github.com/taiki-e/cargo-llvm-cov/blob/v0.9.1/src/cargo.rs#L63).
The suite builds the binary before running tests, so it need not exist before
the coverage command starts. Restart and snapshot-restore contracts
require Docker and the cached Qdrant image; set `MASH_TEST_DOCKER_SG=1` on Xenia.
They own their containers and API processes and do not restart the supplied
backend test services. Use the instrumented binary; forced API termination can
prevent its final LLVM profile flush, so the contract's runtime checks do not
guarantee that all child-process execution appears in the coverage report.
Snapshot restoration stops all fixture writers and restores both group
collections onto a fresh database after deleting the original database and
volume. See the [backup scope](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/silver-gold-configuration.md#quiescent-backup-and-restore).

Then run:

```bash
rustup component add llvm-tools-preview
make coverage
```

This runs every test except the manual benchmarks, including the installed
model and all three shared-backend contracts. It writes LLVM JSON coverage to
`target/llvm-cov/coverage.json`; set `COVERAGE_OUTPUT` to choose another location.
The coverage task cleans workspace coverage artifacts before execution so older
binaries cannot contribute obsolete source mappings in a persistent target.
On nightly, use `CARGO_ARGS=--branch` to include
LLVM's experimental branch instrumentation. This does not measure MC/DC.
Generated SQL bindings, separate test files, standalone test helpers and the
test-only relevance evaluator and manual benchmark modules are excluded from
the report.
Tests embedded in production source files still contribute to those files'
coverage, so assess uncovered behavior as well as the aggregate percentage.
Coverage needs LLVM instrumentation; regular development continues to use
Cranelift. On the affected nightly, use `RUSTFLAGS=-Znext-solver=no make coverage`.

To compare inference thread counts and reranking batch sizes on your own corpus,
use an explicitly refreshed, isolated index and installed models:

```bash
MASH_TEST_MODELS_DIR=/path/to/installed/models \
MASH_TEST_SEARCH_DB=/path/to/refreshed/index.sqlite3 \
  cargo test-clif --release --run-ignored all -j 1 \
  -E 'test(benchmark_real_search)' --success-output immediate
```

These manual diagnostics open the index read-only, warm models, and emit JSON
timings for seven queries in message and chat modes. They compare ordered
message IDs and chat excerpt IDs against the default inference configuration;
the batch comparison also checks scores. Ranking changes fail the diagnostic.
Repeated settings help reveal timing noise. Measurements exclude CLI startup,
shared search, and do not establish relevance by themselves.
Use these alongside end-to-end measurements before changing production defaults.

CPU indexing reserves two workers for long embedding batches and allows short
batches to use all eight workers. It preserves the dynamically quantized model's
batch membership and vector generation. See the
[indexing memory measurements](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-09-30-index-memory.md) for the
memory/throughput tradeoff and the manual worker-budget diagnostic. This is an
inference workspace limit, not a total indexing memory limit.


## Deployment releases

Every push to `main` first runs CI, including generated-query checks, linting,
and SQLite, real embedding/reranking/hybrid-search, and Qdrant tests.
CI explicitly installs and smoke-tests local models on cache misses; model assets
are cached by platform, dependency lockfile and installation implementation.
The shared-storage gate includes Claude-copy reconciliation.
Ordinary pushes validate changes without deploying them. To release a stable
batch, manually run the **CI** workflow on `main` with its **deploy** input enabled.
Only a successful explicitly requested release run can build and
deploy the Linux x86-64 and Apple Silicon binaries from that same commit.
Deployments are serialized; a superseded commit is skipped after acquiring
the deployment lock. New pushes do not interrupt an active deployment.
Manual CI runs without that input only validate. Pull requests and manual runs
on other branches never deploy. For local fleet releases, use
`bash tools/fleet.sh preflight shared-api xenia` before building, then `deploy`
at the agreed release milestone; see [deployment/README.md](deployment/README.md).

Deployment atomically installs `mash` to `~/.local/bin` on Xenia,
dev-macbook, and personal-macbook over Tailscale. Atomic installation is per
machine; a network failure can leave the fleet partially updated, and the
workflow reports failure so the current CI deployment can be retried.
The macOS binary is
cross-compiled on the Linux runner with `cargo-zigbuild`; destination machines
need neither this repository nor a Rust toolchain.

Deployment requires `TS_OAUTH_CLIENT_ID`, `TS_OAUTH_SECRET`,
`DEPLOY_SSH_PRIVATE_KEY`, and `DEPLOY_SSH_KNOWN_HOSTS` repository secrets.
After the current-main check, a preflight verifies that all four are populated
before starting either platform build. Missing credentials fail deployment with
the missing secret names; credential values are never printed by this check.
Successful CI alone does not confirm that any machine was updated: verify the
deployment job and destination smoke checks too.

Binary deployment does not rebuild indexes. Use the
[redeployment checks](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/redeployment.md) to refresh only incompatible or
pending artifacts, while preserving required data migrations.

## License

See [LICENSE](LICENSE).
