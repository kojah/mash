# Include older chats and search by meaning

## Add older history

Mash starts with the last 90 days. To include every older conversation:

```sh
mash index --all-history
```

To go back a fixed number of days instead:

```sh
mash index --since-days 365
```

To refresh just one agent's history:

```sh
mash index --provider codex
```

The provider names are `claude-code`, `codex`, `opencode`, `copilot`, `cursor`
and `antigravity`.

Indexing needs the local search service (Qdrant) running and never changes your
original history files. Check the command's final result for errors.

These flags apply to one run. To change the age limit permanently, see
[configuration](configuration.md). For continuous updates, see
[keep history up to date](synchronization.md).

### Copilot in VS Code remote windows

Index each kind of session on the machine that stores it:

- **Copilot Chat** sessions from a Remote-SSH, container or Codespaces window
  are stored on the computer running VS Code. Their project is the folder on
  the remote machine.
- **Copilot CLI** sessions started from the chat view of a remote window run on
  the remote machine and are stored in its `~/.copilot/session-state`.

If Copilot is still writing a session while Mash reads it, Mash keeps that
chat's previously indexed messages and reads the session again on the next
refresh.

## Enable semantic search

Word and phrase searches work without models. Semantic search lets you ask a
question even when you do not remember the original wording. It takes three
steps: download the models, change two settings, and index again.

1. Download the optional local models:

   ```sh
   mash models install
   ```

   This downloads about 177 MB from `https://huggingface.co` into
   `~/.mash/models` (Apple silicon also downloads a Core ML embedding model).
   Set `HF_ENDPOINT` to download from a Hugging Face mirror instead, or pass
   `--models-dir PATH` to use another directory.

2. In the [configuration](configuration.md) created by `mash init`, set these
   values. Edit the existing tables rather than adding duplicates, and keep
   your other settings:

   ```toml
   [views.build]
   representations = "lexical-and-dense"

   [views.query]
   intent = "related-discussion"
   ```

3. Refresh and try a question:

   ```sh
   mash index
   mash search 'why did we choose a separate database' --intent related-discussion
   ```

This first pass takes longer because it prepares your existing history for
semantic search; later passes reuse unchanged work. Semantic queries currently
need English.

Word search still works, and is faster because it loads no models:

```sh
mash search 'auth_token' --intent exact-mention
```

To keep it as what plain `mash search` does, leave `intent` as `exact-mention`
above and pass `--intent related-discussion` when you want meaning.

If something fails, see [troubleshooting](troubleshooting.md).
