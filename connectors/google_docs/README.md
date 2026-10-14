# Google Docs → Mash

A runnable, read-only connector. Selected Google Docs become searchable
`google-docs:document` documents, and their comments and replies become entries
in a `google-docs:comments` container per Doc (Mash documents cannot be
parents). It uses the [Python SDK](../../sdk/python/README.md), polls Drive and
talks to Google over plain HTTPS with the standard library.

## Google Cloud setup (once)

1. In [Google Cloud Console](https://console.cloud.google.com/), create or pick
   a project and enable the **Google Drive API** (APIs & Services → Library).
2. Configure the **OAuth consent screen** (Google Auth Platform → Branding and
   Audience). Choose **External** for a personal account, or **Internal** on a
   Workspace domain. Add the `.../auth/drive.readonly` scope under Data Access.
3. **Publish the app to "In production"** (Audience → Publish app). An
   External app left in **Testing** issues refresh tokens that expire after
   7 days, so the connector would need `auth` again every week. An unverified
   production app is fine for personal use: you see a one-time "Google hasn't
   verified this app" warning (Advanced → Go to app). Internal apps have no
   such expiry.
4. Create credentials: **Create client → Desktop app**, then download the
   JSON and keep it private, e.g. `~/.config/mash/google-docs-client.json`.

Even in production a refresh token stops working if you revoke access at
<https://myaccount.google.com/permissions>, leave it unused for six months, or
issue more than 100 tokens for one client and account (the oldest stop first). `sync` then exits with code 3 and
`authorization-required`; run `auth` again.

Only `drive.readonly` is requested. `drive.metadata.readonly` cannot export
text or read comments, `drive.file` cannot traverse folders, and
`documents.readonly` cannot list folders, drives or comments.

A service account (sharing folders with its email) would avoid the browser
consent for headless hosts, but is not supported yet: it needs JWT signing.

## Configure and run

```sh
uv sync --project connectors/google_docs --locked
```

Save a configuration outside Git, e.g. `~/.config/mash/google-docs.toml`. Paths
are relative to it, with `~` expansion:

```toml
account_id = "personal"   # Stable; the Mash source is google-docs:personal.
client_secret = "~/.config/mash/google-docs-client.json"
# token = "~/.config/mash/google-docs-token.json"  # The default; written 0600.
state_dir = "~/.local/share/mash/connectors/google-docs/personal"
# Allowlists; each empty list selects nothing. IDs come from Drive URLs.
folders = ["1AbCdEfGhIjKlMnOpQrStUvWxYz"]       # Recursive.
shared_drives = []                             # Every Doc in the drive.
documents = ["1ZyXwVuTsRqPoNmLkJiHgFeDcBa"]     # Single Docs.
# history_days = 90  # By Doc creation time; omit for all history.
poll_seconds = 300

[[destinations]]
type = "local"
database = "~/.local/share/mash/mash.sqlite3"
executable = "mash"
# config = "~/.config/mash/config.toml"

# [[destinations]]
# type = "shared"
# api_url = "https://mash.example"
# group_id = "personal"
# node_id = "laptop"
# token_env = "MASH_API_TOKEN"
```

```sh
alias gdocs='uv run --project connectors/google_docs --locked mash-google-docs --config ~/.config/mash/google-docs.toml'

gdocs auth          # Prints a consent URL; paste back the redirect address.
gdocs sync          # One poll and delivery.
gdocs sync --watch  # Keep polling in this process; Ctrl-C stops it.

mash index --rebuild-local-views
mash search 'your doc text' --source google-docs:personal --local
```

`auth` works over SSH: open the printed URL in any browser, approve, and the
browser then fails to load `http://127.0.0.1:8085/...`. Paste that address (or
just its `code`) back. The refresh token is stored with mode 0600, and the
connector refuses a token file other users can read. Access tokens stay in
memory. Tokens, client secrets and Doc text never appear in output: each poll
prints JSON counts and content-free issues. Exit codes: 1 delivery or Google
failure, 2 configuration or state, 3 authorization required, 9 partial poll.

## Capture behavior

- Only `application/vnd.google-apps.document` files are selected. Folders are
  traversed recursively, including folders inside shared drives.
- Docs are exported as Markdown (plain text if Drive refuses), up to Mash's
  1 MiB content limit (`truncated` metadata marks a cut). Titles, Doc URLs,
  creation and modification times are kept; owners, the last modifying user,
  parent folders and shared drive are metadata. Mash documents carry no author.
- Comments keep author, resolved state and the quoted passage, which also
  prefixes the entry text. Replies point at their comment; resolve or reopen
  actions are metadata.
- Each poll lists the selected scopes and re-exports only Docs whose Drive
  `version` changed since their last acknowledged delivery (tracked in
  `state_dir`, per destination set, so adding a destination backfills it). A changed Doc refetches all its comments, and a
  comment or reply deleted in Drive is deleted from Mash.
- A Doc moved to the trash is deleted from Mash. A Doc that disappears from the
  listing (unshared, moved out of scope, permission errors) is kept; narrowing
  scope or `history_days` never deletes. Failures are reported as issues.
- 429, 5xx and Drive rate-limit 403s are retried up to five attempts with
  backoff (honouring `Retry-After`); a failed Doc is retried on the next poll.

## Verify

```sh
uv run --project connectors/google_docs --locked ruff check connectors/google_docs
uv run --project connectors/google_docs --locked ruff format --check connectors/google_docs
uv run --project connectors/google_docs --locked ty check connectors/google_docs
MASH_CONNECTOR_MASH_BINARY=/absolute/path/to/mash \
  uv run --project connectors/google_docs --locked python -m unittest discover \
  -s connectors/google_docs/tests
```

Tests use an in-memory Google endpoint; the integration test delivers into a
disposable Mash database and finds a Doc and a comment with `mash search`/`view`.
