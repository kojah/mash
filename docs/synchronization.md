# Keep history up to date

Mash searches an index of your chats. Searching does not scan the provider's
history files, and starting the dashboard does not refresh them.

## Refresh now

```bash
mash index
mash status
```

Indexing skips unchanged input and processes new or changed history. It shows progress as it works. Check the final result: an
incomplete scan or failed shared publication can leave work pending.

## Keep refreshing while you work

Run this in a separate terminal:

```bash
mash index --watch
```

It indexes once, then refreshes after history changes settle. Leave the process
running; Ctrl-C lets the active pass finish and stops watching. Closing the terminal stops ongoing updates.

Existing provider, age, and local/shared settings still apply. To include older
history as you work:

```bash
mash index --watch --all-history
```

For a permanent age setting, edit [your configuration](configuration.md).
Do not combine `--watch` with `--full`.

## What the agent plugin does automatically

The history plugin starts a low-priority catch-up index at session start, at
most once every ten minutes. The agent does not wait for it. If another writer
is busy, that attempt exits without changes.

This is useful for catching up between sessions. Use the watcher when you want
updates during a long-running conversation. Set `MASH_AUTO_INDEX=off` in the
agent's environment to disable the session-start catch-up.

## Understand freshness warnings

A search may warn that no watcher is active or that provider coverage is
incomplete or unverified. It can still return useful retained history.

- Run `mash index` to refresh now.
- Check the indexing result for errors.
- Keep a watcher running if you need ongoing updates.

A running watcher is not proof that every source was read successfully.
If the warning follows an upgrade or mentions a schema mismatch, use the
[upgrade guide](upgrades.md); ordinary indexing cannot migrate the database.

## Across machines

Each machine must index and publish its own history. Run `mash status --shared`
to inspect publication state. If a shared publication is pending, restore the
connection and retry `mash index`.

See [multi-machine setup](shared-index.md).

The dashboard refreshes its chat list periodically but keeps a transcript you're
reading stable until you request an update. See [dashboard refresh behavior](dashboard.md).
