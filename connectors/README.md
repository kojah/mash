# External connector API blueprints

These Slack, Discord, and GitHub Issues/PR connectors describe the **desired API**.
They are not runnable integrations yet: `api/mash_sdk/__init__.pyi` is a typing
specification, and each platform client is an injected protocol. The SDK in
`sdk/python` implements the models, local and shared delivery, `sync` for polled
sources and `Connector.run` (backfill diffs, event capture and debounce), and
Mash indexes delivered records for search. Platform adapters still need
implementation; `sdk/python/examples/markdown_folder.py` is a complete, runnable
connector to start from.
Sketched future connectors live in blueprints/: Obsidian and Apple Notes share the note-to-document mapping in blueprints/notes.py; their source readers are sketched in blueprints/obsidian.py and blueprints/apple_notes.py. blueprints/imessage.py sketches iMessage mapping and blueprints/gold.py sketches the custom-gold API.
`blueprints/imessage.py` sketches container/entry mapping from a separate read-only SQLite
reader. Native database readers and local attachment transport are not implemented.

The platform adapter handles authentication, permissions, rate limits, pagination,
source text rendering, and event capture. Mash's SDK should handle durable pending
changes, canonical fingerprints, debounce, retries, checkpoints, and receipts.
Mash never imports or executes connector code in its server process.

## Configuration

Every connector takes a `Scope` and one or more explicit destinations:
`example.toml` sketches the matching configuration-file shape (not wired up yet).

```python
scope = Scope(
    ids=(
        "C0123456789",
        "C9876543210",
    ),  # Slack/Discord channel IDs; GitHub owner/repo names
    history_days=90,  # None means all available history
    quiet_seconds=2,
    max_wait_seconds=10,
)
destinations = [
    Destination.local(database=Path("mash.sqlite3")),
    # Optional; the same settings as Mash's [shared] configuration:
    Destination.shared(
        api_url="https://mash.example",
        group_id="personal",
        node_id="laptop",
        token_env="MASH_API_TOKEN",
    ),
]
```

An empty allowlist selects nothing. The same selection applies to backfill and
live events, with defensive checks even if the source sends unrelated events.
The history window uses message creation time, not edit time. On GitHub, old
issues can still contain recent comments and must remain discoverable. Discord
thread channels are separate chats and must be explicitly selected; native
parent-channel/workspace/repository information is chat metadata.
Note connectors select vault folders or stable notebook IDs. Notes with unknown
creation time are excluded from a bounded creation-time window; use unbounded
history to include them. Obsidian readers must not invent creation time from mtime.

Rolling windows restrict what is newly ingested; they do not automatically purge
previously imported history. Changing a scope must not silently delete excluded
data. A snapshot's `complete` flag applies only to its `created_since` scope;
partial pages, permission failures, and time-excluded records cannot become
omission-based deletions. Large snapshots need a paged begin/page/end protocol
before these blueprints are production-ready.

## Data and delivery contract

- Every record is an `Object` with a core `role` (container, segment, entry or
  document), a namespaced `kind` label and an optional `parent`.
- Channels/issues are containers; their text and authors live on entries.
- Notes are standalone documents with text, title, metadata, and attachments.
  They do not require synthetic containers or entries. Documents must be searchable via
  gold and usable as direct evidence references alongside messages.
- iMessage conversations are containers, with group participants in metadata and
  sender identities on messages. Missing rows in a pruned local database are
  not automatically treated as deletions.
- `github.py` covers both issues and PRs. A PR description, general discussion,
  reviews, and inline review replies share one container. Review replies retain native
  parent links; file/diff location, review state and merge/branch details are metadata.
  Configure `item_kinds` to select issues, PRs, or both. The native adapter must
  reconcile the same PR appearing through the issues and pull-request APIs.
- Stable source IDs survive restarts and credential rotation. Native message
  IDs are preserved or deterministically namespaced. Slack timestamps stay strings.
- Message reply links represent real same-chat relationships. Cross-chat
  references and arbitrary source properties use bounded opaque metadata.
- Partial source updates fetch complete current records before an upsert.
- Closing an issue, leaving a scope, losing access, or receiving an incomplete
  response is not a deletion. Explicit confirmed deletes are separate changes.
- The SDK must persist deliveries before upstream acknowledgement, persist batch
  IDs across retries, and only advance destination checkpoints on a matching ACK.
  Requests may be transmitted more than once. Once-only logical application needs
  transactional server receipts; it is not promised by this blueprint.
- `Delivery.acknowledge_capture` is a source-adapter callback after capture is
  durable, not Mash's ingestion ACK. Webhooks, Gateway resume, and polling have
  different native delivery semantics; some adapters have no upstream event ACK.
- Event capture must be active before backfill to avoid a bootstrap gap; source
  cursors/resume tokens need durable handling. Delivery payloads must be serializable.
- Silver contains canonical data only. Search and custom search artifacts belong
  to gold. `gold.py` sketches an external projector with explicit message evidence;
  gold input revisions and projection receipts are separate from silver ingestion ACKs.

The Rust connector contract owns the wire schema. Generated Python validation
models should derive from that schema; the temporary `.pyi` is an API discussion
aid, not a second authoritative schema.

## Blueprint checks

```sh
uv tool run --from ruff ruff check connectors
uv tool run --from ruff ruff format --check connectors
uv tool run --from ty --with pydantic ty check connectors --extra-search-path connectors/api
uv run --no-project python -m unittest connectors.test_scope
```

These checks validate Python syntax/types and selection rules. They do not
exercise platform APIs, SDK delivery, or server ingestion.
