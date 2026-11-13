# Include older chats and search by meaning

## Add older history

Mash starts with the last 90 days. To include older conversations:

```sh
mash index --all-history
```

To refresh just one agent's history:

```sh
mash index --provider codex
```

Your original history files are never changed. Keep the local search service
running and check the command's final result for errors.

For continuous updates, see [keep history up to date](synchronization.md).
To change the age limit permanently, see [configuration](configuration.md).

### Copilot in VS Code remote windows

Copilot Chat sessions from a Remote-SSH, container or Codespaces window are
stored on the computer running VS Code, so index them there; their project is
the folder on the remote machine. Copilot CLI sessions started from the chat
view of a remote window run on the remote machine and are stored in its
`~/.copilot/session-state`, so index them with Mash on that machine.

If Copilot is still writing a session while Mash reads it, Mash keeps that
chat's previously indexed messages and reads the session again on the next
refresh.

## Enable semantic search

Word and phrase searches work without models. Semantic search lets you ask a
question even when you do not remember the original wording.

First, download the optional local models:

```sh
mash models install
```

This downloads about 177 MB from `https://huggingface.co` into
`~/.local/share/mash/models` on Linux or
`~/Library/Application Support/dev.kojah.mash/models` on macOS (Apple
silicon also downloads a Core ML embedding model). Set
`HF_ENDPOINT` to download from a Hugging Face mirror instead, or pass
`--models-dir PATH` to use another directory.

In the configuration created by `mash init`, update these settings:

```toml
[views.build]
representations = "lexical-and-dense"

[views.query]
intent = "related-discussion"
```

Edit the existing tables rather than adding duplicates, and keep your other
settings. Then refresh and try a question:

```sh
mash index
mash search 'why did we choose a separate database' --intent related-discussion
```

The first pass takes longer because it prepares your existing history for
semantic search. Subsequent passes reuse unchanged work. Semantic queries
currently need English.

You can still search for exact words:

```sh
mash search 'auth_token' --intent exact-mention
```

If something fails, see [troubleshooting](troubleshooting.md).
