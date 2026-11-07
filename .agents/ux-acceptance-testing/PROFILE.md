# CLI UX acceptance profile

## Product

- Name: `mash`
- Binary: a release build of the current checkout, packaged by the session
  adapter as a simulated GitHub release; testers install it from
  `~/Downloads/install.sh` or start with it already in `~/.local/bin/mash`.
- Repository revision command: `git rev-parse HEAD` (the adapter appends
  `-dirty` for uncommitted tracked changes)

Record-tool journeys may use a freshly built development binary supplied with
`--binary`; record the build type and do not use its timings as release performance
measurements. These journeys exercise the stdio MCP interface. Python 3 is
available inside the sandbox for an MCP client; the fixture agent CLIs cannot
execute tool calls. Select `record-and-recall` or `record-correction` explicitly
with `select --family NAME`. Both start installed, before setup. No Qdrant or
model service is available in these offline sessions; preserve any setup or
indexing failure as evidence and distinguish it from record-tool behavior.

## Journey selection

- Selector: `python3 tools/ux-session.py select [--seed N]`. It returns `seed`,
  `family`, `variation`, `goal`, `stop`, `excluded`, and the starting point
  (`preinstall`, `presetup`) to pass to `start`. Pass the same seed to both
  testers of a pair.
- Recent-family source: `- Journey family:` lines in the three most recently
  modified journals under the journal directory.
- Families describe outcomes, not commands. They live in `FAMILIES` in
  `tools/ux-session.py`.

## Session adapter

- Start: `python3 tools/ux-session.py start SESSION_ID [--preinstall] [--presetup]`
  from the repository root. It builds the release binary unless
  `--binary PATH` names one already built from this revision, with the matching
  `mash-server` binary beside it. The simulated archive contains both. `--preinstall`
  runs the installer and `--presetup` also runs `mash init`, as the selector
  asks.
- Stop: `python3 tools/ux-session.py stop SESSION_ID`
- Required reported fields: `session`, `enter`, `binary_sha256`, `revision`,
  `roots.home`, every `isolation` value `true`, and `limits`.
- Allowed session ID pattern: `^ux-[a-z0-9-]{3,40}$`

Testers run every command as `SESSION/enter 'shell command'`. It runs in a
bubblewrap sandbox with all namespaces unshared (no network), read-only `/usr`
and `/etc`, a private `/tmp`, and only the session home mounted at
`/home/user`. Each command has a 600 second timeout and an 8 GiB
virtual-memory limit. The home persists between commands. `start` verifies
that the network is blocked, that the repository, host home and `mash` are not
visible before installation, and that `HOME` is `/home/user`; it deletes the
session and fails if any check fails. `stop` removes only a directory carrying
the adapter's marker for that exact session ID.

## Disposable product state

- Working roots: `/home/user` inside the sandbox
  (`/home/james/scratch/mash-artifacts/ux-sessions/SESSION_ID/home` on the host).
- Configuration/state roots: `/home/user/.config/mash`,
  `/home/user/.local/share/mash`, and whatever else mash creates under
  `/home/user`.
- Test fixtures:
  - Claude Code, Codex and OpenCode histories in their default locations:
    six chats across `/home/user/work/{ops,billing,site}`, dated 1 to 12 days
    before `start`. One Claude Code chat contains a fake API key.
  - `~/Downloads/install.sh` is the repository's installer. A stand-in `curl`
    serves `https://github.com/kojah/mash/releases/...` from
    `~/.release-mirror` and reports every other host as unresolvable.
  - Stand-in `claude` and `codex` CLIs: `mcp get|add|list|remove` edit the
    real user settings (`~/.claude.json` `mcpServers`, `~/.codex/config.toml`
    `[mcp_servers.*]`) as the real CLIs do. Plugin commands fail as they would
    offline, since plugins come from a GitHub marketplace. They cannot run
    agents. Tell testers this before exploration: it is the environment, not
    the product.
  - `~/bin/agent-say 'text'` appends a new user message to a Claude Code chat,
    standing in for using the agent.
  - There is no network: `mash models install` cannot download.
- Forbidden host or production state: `/home/james` (including real agent
  histories and `~/.local/share/mash`), the repository checkout, the shared
  fleet and its credentials, and Docker.

## Journals and discovery

- Journal directory: `/home/james/scratch/mash-artifacts/ux-journals/`
  (outside Git; one subdirectory per run, named by date and batch).
- After exploration: testers may read the repository README, `mash --help`
  output already captured, and source under `app/` to explain observations;
  search Beads with `bd search` for existing issues and record duplicates.
- Performance add-on: `.agents/skills/mash-performance-profiling/SKILL.md`.
- External issue updates: none. Accepted findings go back to the coordinating
  agent, which files Beads issues only when the user asks.

## Handoff

- Formal acceptance tests: the coordinating agent, as CI smoke scripts under
  `.github/scripts/` or `app/tests/`.
- Unit/integration tests: the coordinating agent, following AGENTS.md.
- Product changes: the coordinating agent, after the user picks findings.
