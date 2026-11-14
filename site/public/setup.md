# Set up Mash on this machine

You are a coding agent, and the person you are working with asked you to
install Mash. Mash is a local command-line tool and MCP server that indexes
their past chats with coding agents (Claude Code, Codex, GitHub Copilot,
Cursor, OpenCode, Antigravity) and lets them, and you, search those chats.
Everything stays on this machine unless they later choose to share.

Work through the steps in order. Every `mash` command prints JSON on stdout;
read it to decide what to do next. Stop and tell the person when a step says to
ask them, or when a command fails in a way this page does not cover. Full
guides: https://getmash.io/installation/ and
https://getmash.io/troubleshooting/.

## Before you start: ask once

Tell the person what setup will do, and get a yes before you continue:

- Install the `mash` and `mash-server` programs for their user (no
  administrator rights).
- Download and run Qdrant, the local search service: about 30 MB, checksum
  verified, reachable only from this machine, started again at each login.
- Read their agents' chat history files. Mash never changes those files.
- Register Mash with the Claude Code and Codex command-line tools it finds, by
  running those tools' own commands. This edits the agents' settings files;
  the result lists each file and the command that undoes it.

Then ask one question: do they also want search by meaning, which finds a
discussion even when the wording differs? It downloads about 177 MB of models
from huggingface.co. Without it, Mash searches by words and phrases, which
needs no models. They can add it later.

## 1. Install Mash

Skip this step if `mash --version` already prints a version.

Check whether a release is published:

```sh
curl -fsSL -o /dev/null -w '%{http_code}\n' https://github.com/mashium/mash/releases/latest/download/SHA256SUMS
```

If it prints `200`, install the prebuilt release.

macOS (Apple silicon) or Linux (x86-64 or ARM64), including WSL:

```sh
curl -fsSL https://raw.githubusercontent.com/mashium/mash/main/install.sh | sh
```

Windows x64, in PowerShell:

```powershell
irm https://raw.githubusercontent.com/mashium/mash/main/install.ps1 | iex
```

Both install to the user's `.local/bin` directory and verify the release
checksum. If that directory is not on `PATH`, call Mash by its full path for
the rest of this page, and tell the person to add the directory to `PATH`.

If the check prints anything else, or the machine is an Intel Mac, build from
source. This needs a stable Rust toolchain and takes several minutes; if
`cargo` is missing, ask the person before installing Rust.

```sh
cargo +stable install --locked --git https://github.com/mashium/mash mash
```

Confirm:

```sh
mash --version
```

## 2. Start the local search service

```sh
mash qdrant install
```

Success is a JSON result with `"outcome"` of `installed`, `current`,
`repaired` or `upgraded`. Rerunning it is safe. If the person already runs
their own Qdrant 1.20 or later on `127.0.0.1:6333`, skip this step.

## 3. Optional: search by meaning

Only if the person said yes to the model download:

```sh
mash models install
```

Run it before step 4, so that setup turns search by meaning on.

## 4. Connect their histories

```sh
mash init --yes
```

This finds supported agents' histories in their usual places, indexes the last
90 days, and registers Mash with the Claude Code and Codex command-line tools
it finds. It checks Qdrant first and changes nothing if Qdrant does not
answer.

Read the result:

- `agents` lists each agent that was connected, the settings `files` that were
  edited and the `undo` commands. Report these to the person. An empty list
  means no agent was connected; `next` says why.
- `index_error` means setup was saved but the first index failed. Fix the
  cause it names, then run `mash index`.
- `written: false` on a rerun means the existing configuration was kept. That
  is not a failure.

To index everything instead of the last 90 days, which can take much longer on
a large history:

```sh
mash index --all-history
```

## 5. Check it works

```sh
mash status
```

Then search for a distinctive word the person suggests, or one from this
conversation:

```sh
mash search migration
```

A result lists matching chats with a `snippet` for each matching message. An
empty `results` list with no error means nothing matched; try another word
before concluding that setup failed.

## 6. Tell the person

Report, in a few lines:

- what was installed and where;
- which agents were connected, which files were edited, and the undo commands;
- how many chats and messages were indexed;
- that they must restart each connected agent, including you, before its Mash
  tools appear;
- that `mash index` picks up new chats, and `mash dash` opens a local dashboard
  at http://127.0.0.1:8790.

## If something fails

Each error is JSON with a `kind` and a message that names the fix.

| `kind` or symptom                         | What to do                                                                           |
| ----------------------------------------- | ------------------------------------------------------------------------------------ |
| `mash: command not found`                 | Use the full path from step 1, or add the install directory to `PATH`.               |
| `qdrant-unavailable`                      | Run `mash qdrant install`, then repeat the command that failed.                      |
| `database-missing`                        | Nothing is indexed yet. Run `mash init --yes`, or `mash index` if setup already ran. |
| `model` or `search-not-ready` on a search | Search by words instead: add `--intent exact-mention`.                               |
| `index-busy`                              | Another Mash process is indexing. Wait, then retry.                                  |
| Anything else                             | Show the person the message and https://getmash.io/troubleshooting/. Do not guess.   |

Do not edit Mash's configuration file, delete its data directory or change an
agent's settings by hand to work around a failure. Ask the person first.
