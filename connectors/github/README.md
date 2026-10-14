# GitHub → Mash

A runnable, read-only connector for issues and pull requests in allowlisted
repositories. Each issue or PR becomes a `github:issue` or `github:pull-request`
container. Its description, comments, reviews and inline review comments are
entries in it. It uses the [Python SDK](../../sdk/python/README.md) and
[githubkit](https://github.com/yanyongyu/githubkit) for typed, async REST calls
with rate-limit retries. This package supersedes the
[`github.py` blueprint](../github.py).

## Token (once)

Create a [fine-grained personal access token](https://github.com/settings/personal-access-tokens/new)
with **Repository access** limited to the repositories you will select and these
read-only **Repository permissions**:

- **Issues: Read-only**
- **Pull requests: Read-only**
- **Metadata: Read-only** (always included)

Organization repositories may need an owner's approval for the token. Export it
in the variable named by `token_env` (default `GITHUB_TOKEN`). If that variable
is unset, the connector uses `gh auth token` from the GitHub CLI. That token
usually has broader scopes; the connector only sends GET requests. Tokens are
never written to state or output.

## Configure and run

```sh
uv sync --project connectors/github --locked
```

Copy [example.toml](example.toml) outside Git, e.g. to
`~/.config/mash/github.toml`, and set the account ID, repositories and
destination. Paths are relative to the configuration file, with `~` expansion.

```sh
alias gh-mash='uv run --project connectors/github --locked mash-github --config ~/.config/mash/github.toml'

gh-mash --check      # Offline setup report; no ingestion, state or network.
gh-mash sync         # One poll and delivery.
gh-mash sync --watch # Keep polling in this process; Ctrl-C stops it.

mash index --rebuild-local-views
mash search 'your issue text' --source github:personal --local
```

Each poll prints JSON counts and content-free issues, never text or tokens.
Exit codes: 1 delivery failure, 2 configuration or state (including a failed
`--check`), 3 no token, 9 partial poll (a repository failed; its cursor stays
put and the next poll resumes). `--check` follows the
[prerequisite declaration](src/mash_github/prerequisites.toml). It makes no
network requests, so GitHub's acceptance of the token stays `unverified` until
`sync` reads the repositories.

## Capture behavior

- `repositories` is an `owner/name` allowlist; an empty list selects nothing.
  `item_kinds` selects issues, PRs or both. PRs are read through the issues API,
  so each has one ID (`issue:<GitHub issue ID>`), stable across renames.
- Live updates are polled; webhooks would need a public endpoint. Each poll
  lists issues and PRs updated since a per-repository cursor kept in
  `state_dir`. New comments, reviews and review comments update their issue, so
  unchanged items are not fetched again. A listed item's discussion is
  refetched for the history window and synced as a complete snapshot; the SDK
  sends only what changed. Comments deleted on GitHub are deleted from Mash the
  next time their item is listed; a deletion alone may not update the item.
- `history_days` uses creation time. An old issue with a recent comment is
  still listed; only entries created in the window are ingested, and its
  container carries the current title and state. Items with nothing in the
  window are counted as `time_excluded_items`. Narrowing the window or the
  allowlist never deletes earlier history. Changing destinations, kinds or the
  window restarts the cursor from the window start.
- Review comments keep their inline reply links; their path, line, side, diff
  hunk and review ID are metadata. Reviews keep their state; an approval with no
  text reads `(approved)`. Pending reviews are skipped. Containers carry the
  repository, number, state, labels and, for PRs, draft and merged flags.
- Closing an issue updates its container; it is never a deletion. Deleted
  issues are not detected.
- githubkit retries a rate-limited request once, after the server's
  `Retry-After` or the limit reset, and 5xx responses three times. Other
  failures are reported per repository.

## Verify

```sh
uv run --project connectors/github --locked ruff check connectors/github
uv run --project connectors/github --locked ruff format --check connectors/github
uv run --project connectors/github --locked ty check connectors/github
MASH_CONNECTOR_MASH_BINARY=/absolute/path/to/mash \
  uv run --project connectors/github --locked python -m unittest discover \
  -s connectors/github/tests
```

Tests use an in-memory GitHub API behind githubkit's HTTP transport; the
integration test delivers into a disposable Mash database and finds an issue
and a review comment with `mash search`/`view`.
