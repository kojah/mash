# Useful commands

| I want to… | Command |
| --- | --- |
| Set up Mash | `mash init` |
| Set up without interactive prompts | `mash init --yes` |
| Find a conversation | `mash search 'a word you remember'` |
| Find a chat by title | `mash list-chats --title-contains 'part of the title'` |
| Read a matching message | `mash view MESSAGE_ID --context 3` |
| Open the dashboard | `mash dash` |
| Refresh recent history | `mash index` |
| Include older history | `mash index --all-history` |
| Keep watching for changes | `mash index --watch` |
| Check setup | `mash status --installation` |
| Check search readiness | `mash status` |
| Download optional search models | `mash models install` |
| Back up my history | `mash export /path/to/new-backup` |

Replace `MESSAGE_ID` with a message ID returned by search.
Terminal results are JSON; use the [dashboard](dashboard.md) or a
[connected agent](agents.md) if you prefer reading conversations directly.
`mash init` uses an interactive wizard when stdin and stderr are terminals;
its prompts go to stderr and its final result remains JSON on stdout.

Run `mash --help` or `mash COMMAND --help` for available options.
Help and version output are JSON too. To display the help text with line breaks:

```sh
mash --help | python3 -c 'import json,sys; print(json.load(sys.stdin)["help"])'
```

In PowerShell, use `(mash --help | ConvertFrom-Json).help`.
Use `--config PATH`, `--db PATH`, or `--models-dir PATH` when your files
are outside their default locations.

See [search filters](search.md), [backup and restore](upgrades.md),
[removing a chat](privacy.md#remove-a-chat), and
[multi-machine setup](shared-index.md) for complete examples.
