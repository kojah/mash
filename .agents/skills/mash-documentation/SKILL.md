---
name: mash-documentation
description: Write and maintain Mash's README and documentation, including documentation updates accompanying user-visible behavior changes. Keep the README welcoming to first-time users and technical reference in focused docs.
---

# Mash documentation

The README is the front door for someone who has never used Mash. It should
help them understand the benefit, get set up, and find their first conversation.
Detailed documentation should be easy to find without making that first visit
feel like reading an implementation manual.

## Choose the right home

Read the current `README.md` and `docs/README.md` before choosing where an update
belongs. Paths in this skill are relative to the repository root.

- **README:** purpose, supported agents, the shortest working setup, a few
  useful examples, dashboard entry point, essential privacy information, and
  links onward. Keep required dependencies visible. Aim for roughly 150 lines;
  if it grows beyond about 200, reconsider what belongs in a linked guide.
  These are review cues, not a reason to omit a prerequisite.
- **`docs/`:** user guides only: setup, search, agents, dashboard, refresh,
  sharing, backups, configuration, and troubleshooting. Lead with the task and
  a working example. Link new user guides from `docs/README.md`.
- **`docs/internal/`:** architecture, storage, protocol contracts, tuning,
  development, and maintenance. These files are not published on the website
  or included in its search. Do not link them from public guides just to retain
  technical detail. Preserve useful engineering information here when cutting it
  from user documentation.
- **Specialized references:** keep deployment procedures in
  `deployment/README.md`, protocol modeling in `protocol/README.md`, with links
  from the internal documentation index. Private operational logs,
  machine-specific rollout notes, and captures stay outside Git.

Give each detailed contract one maintained home. Keep internal contracts out of
the README; do not copy the full flags, schema versions, limits, or recovery
procedure into several pages. A new feature usually needs an existing guide updated,
not another README section.

Lead user guides with a working task, expected result, and next step. Keep
protocol fields and implementation details in unpublished internal notes. Check the common
journeys: install, find/read a chat, connect an agent, browse/export a transcript,
refresh/include older history, share across machines, back up/restore/upgrade,
and diagnose missing results. Keep the website sidebar and docs index aligned;
the website builds from `docs/`, so never edit its generated content copies.

## Describe current behavior

Check relevant CLI definitions, configuration, implementation or tests before
carrying forward a factual claim. For behavior changes, search affected docs
for the old term or instruction and update the existing explanation rather
than appending a contradictory paragraph.

For installation instructions, distinguish packaging templates from published
packages. Check the actual release and registry endpoints before claiming an
installer works. When a channel is pending, say so and provide a working path.
Keep the user-requested platform install options discoverable in the README;
compact sections or expandable examples can keep them from overwhelming setup.

Remove obsolete release narration such as “this release now…” and instructions
for retired paths when they no longer help a current user. Preserve necessary
compatibility and recovery guidance in `docs/upgrades.md`. In particular, do
not imply that ordinary indexing migrates incompatible schemas or that
export/import accepts every historical archive format.

Label historical measurements and pinned design references as historical; do
not present their timings or contracts as current guarantees. Prefer current
local documentation for operational instructions. Keep useful technical detail
when moving it, and preserve concurrent edits to that material.

## Write for the reader

Lead with the task and the command or outcome the reader needs. Introduce
SQLite, Qdrant, silver, views, transport protocols, and internal module names
only where they help the reader set up or use Mash. Keep developer information
out of public guides. Keep
compiler workarounds, parser limits, benchmark results, and generated-code
instructions out of the README quick start.

Examples should form a usable sequence: prerequisites before commands, clear
placeholders, and optional steps labeled as optional. Explain consequential
effects such as rewriting configuration, publishing history, or preserving
sensitive exports at the point where a reader might take that action.

## Check the result

Review the README from a first-time user's perspective and the detailed guide
from the perspective of someone following its commands. Verify relative links
and section anchors after moves, balanced code fences, and `git diff --check`
including newly added files. Search for stale copies of the changed guidance.
Keep discoverable links for sections referenced by CLI messages or other docs.

For prose-only changes, link/format checks and targeted source verification
are sufficient; do not run broad application builds merely to validate a move.
If examples or behavior change, check those specific commands without running
destructive examples or touching live data just to test documentation.
