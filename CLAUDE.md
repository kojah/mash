@AGENTS.md

Use the project-configured Codebase Memory MCP server for structural code discovery,
following the graph freshness and coverage rules in AGENTS.md.

Use Beads (`bd`) for agent work tracking. Run `bd prime` at session start and follow
the tracking and completion policy in AGENTS.md. Claude Code's local SessionStart
hook loads Beads context automatically; run `bd setup claude` to install it on a
fresh clone.
