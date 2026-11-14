---
name: status
description: Check in plain language whether Mash is healthy and up to date. Use when the person runs /mash:status, or asks whether Mash is working, indexed, current or connected, or why a recent chat is missing from search.
---

# Check that Mash is working

The person wants to know whether Mash is ready and current, and what to do
if it is not. Report in plain words; do not paste JSON unless asked. Hosts
may prefix the tool names.

## Check

1. Call the `status` tool, with `shared: true` when a shared index is
   configured. It covers the index, the search service, the models and each
   machine's last sync.
2. Run `mash status --installation` in a terminal for the agent
   connections: the tool alone does not report whether each agent has the
   Mash tools and the plugin.

If the tool cannot be called at all, say that Mash's tools are not reachable
from this agent and give the `mash status --installation` result alone.

## Report

Lead with one line: working, or the one thing that is wrong. Then only what
applies:

- **Search:** ready or not; whether meaning-based search is available or
  only word search.
- **Index:** how many chats, and how current.
- **Agents:** for each connected agent, fully connected, tools only, or not
  connected.
- **Other machines:** when each last synced, and any that have not
  published.
- **Next step:** the single action that fixes the first problem, taken from
  the `next_action` and `recommended_action` fields.

When the person asked why a recent chat is missing: Mash refreshes the index
in the background at most every ten minutes and does not follow the current
conversation, so the last few minutes are normally absent. Say that before
treating it as a fault.

## Fixing things

Offer the fix and wait for a yes before running it:

- index out of date: the `index` tool, then check again
- an agent's plugin is stale or missing: the command `next_action` gives

Never download models or install or start the search service on your own:
`mash models install` and `mash qdrant install` are the person's to run.
Give the command and say what it does. Never forget, export or import as
part of a check.
