# Keep history up to date

Mash searches a saved index, not your agents' history files. A new chat becomes
searchable after the next indexing run; searching or opening the dashboard does
not refresh the index.

## Refresh now

```bash
mash index
mash status
```

`mash index` reads new or changed history, skips the rest, and shows progress as
it works. Check its final result: an incomplete scan or a failed shared
publication leaves work pending, and running `mash index` again retries it.
`mash status` then reports whether search is ready.

To refresh without running a command each time, use the watcher below.

## Keep refreshing while you work

Run this in a separate terminal and leave it running:

```bash
mash index --watch
```

It indexes once, then refreshes each time history changes settle. Ctrl-C lets
the active pass finish and stops watching; closing the terminal also stops it.

Your configured providers, age limit, and local/shared settings apply. To include
older history as you work:

```bash
mash index --watch --all-history
```

For a permanent age setting, edit [your configuration](configuration.md).
`--watch` cannot be combined with `--full` or `--reread`.

When you upgrade Mash, the watcher finishes its active pass and then starts the
new `mash` with the same options, so a terminal or service keeps watching
without a restart. If it cannot start the new executable, it exits with an
error; start it again yourself.

## What the agent plugin does automatically

The Mash plugin that `mash init` installs for Claude Code and Codex starts a
low-priority catch-up index at session start, at most once every ten minutes.
The agent does not wait for it. If another indexing run is active, that attempt
exits without changes.

This catches up between sessions. It does not pick up chats during a
long-running conversation; use the watcher for that. To disable the catch-up,
set `MASH_AUTO_INDEX=off` in the agent's environment.

## Understand freshness warnings

“Live indexing is inactive” means no watcher is running. This is normal after
`mash init` or a one-time `mash index`. It does not mean that setup failed or
that the index is out of date; results come from the last indexing run.

A search may also warn that provider coverage is incomplete or unverified. It
still returns the history already indexed.

- Run `mash index` to refresh now, and check its result for errors.
- Keep a watcher running if you need ongoing updates. A running watcher is not
  proof that every source was read successfully.
- If the warning follows an upgrade or mentions a schema mismatch, use the
  [upgrade guide](upgrades.md); ordinary indexing cannot migrate the database.

## In the dashboard

The dashboard refreshes its chat list periodically but keeps a transcript you're
reading stable until you request an update. See [dashboard refresh behavior](dashboard.md).

## Across machines

Each machine must index and publish its own history. Run `mash status --shared`
to inspect publication state. If a shared publication is pending, restore the
connection and run `mash index` again.

See [multi-machine setup](shared-index.md).
