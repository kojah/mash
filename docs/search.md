# Searching and filtering history

[Documentation](README.md) · [Project home](../README.md)

Search evidence includes `lookup.view` and `lookup.list_attachments`: copy these
argument objects into the corresponding MCP tool. They preserve local/shared
origin, including this node's own data returned by a shared-only search. Both
compact and extended results keep them.

For scoped history questions, use MCP `status` with an optional `coverage`
object containing `provider`, `node`, `after`, and `before`, or run
`mash status --coverage --provider claude-code --node xenia --shared`.
Coverage time selectors use message timestamps in Unix seconds; observed bounds
are Unix milliseconds. The report separates recorded provider scan state from
scoped retained-row counts. It never certifies absence or complete history:
historical scan bounds and source freshness are not recorded by these facts.
Shared aggregate certificates do not prove provider/time coverage; scoped shared
counts and provider scan details are `null` when unreported. Shared status failures
preserve local facts with `shared_error`.

The current models support English queries. Unsupported scripts and confidently
detected non-English queries return a usage error asking for an English query,
before loading models or opening an index. Short names and ambiguous technical
terms remain searchable; language detection cannot reliably classify every short
or mixed-language query.

`--sort recent` (MCP `"sort":"recent"`) orders the results that qualify newest
first, after relevance has chosen them; a chat sorts by its latest matching
message, and undated results come last. With paging, relevance chooses each
page and recency orders only that page, so pages never repeat or skip results. The default, `relevance`, keeps the
relevance order. Exact-mention search already breaks ties by recency.

CLI search accepts repeatable `--provider`, `--node`, `--role`, `--chat`,
`--project` and `--repository`. Repeated values for one field are alternatives;
different fields are combined with AND. `--node` filters publishing identity and
does not connect directly to another node. A standalone local installation uses
the node label `local`. `--days` is a rolling message-age window. `--after` is
inclusive and `--before` exclusive; both accept `YYYY-MM-DD` (UTC midnight) or
RFC3339 timestamps with an explicit offset.

MCP exposes the same selection fields: `provider`, `chat`, `node`,
`role`, `project` and `repository` are arrays, including for a single
value.
`after` and `before` use Unix seconds. Omitted or empty shortcut arrays impose
no restriction; an empty list inside a structured predicate, such as
`{"chat": []}`, matches nothing. Unknown chat IDs match nothing.
Use canonical `chat_id` values returned by chat search to drill down
into messages from promising chats:

```bash
mash search 'deployment' --unit chats
mash search 'rollback procedure' --unit messages \
  --chat CHAT_A --chat CHAT_B \
  --provider codex --provider claude-code
```

The corresponding MCP drill-down request is:

```json
{
  "query": "rollback procedure",
  "unit": "messages",
  "chat": ["CHAT_A", "CHAT_B"],
  "provider": ["codex", "claude-code"]
}
```

```bash
mash search 'database migration' --provider codex --provider claude-code \
  --role user --role assistant --after 2026-09-01 --before 2026-10-01
mash search 'deployment' --repository https://github.com/kojah/mash.git \
  --node dev-macbook --node xenia
```

For Boolean combinations, use `--filter` with a structured JSON expression, or
the MCP search tool's `filter` argument. This is a query expression; configuration
files remain TOML. Each expression contains exactly one operator:

- `all`: AND of expressions; an empty list matches everything.
- `any`: OR of expressions; an empty list matches nothing.
- `not`: negation of one expression.
- `node`, `provider`, `role`, `chat`, `project`, `repository`, `source`: lists of exact, case-sensitive values.
- `after`, `before`: inclusive/exclusive Unix timestamps **in seconds**.

An MCP request can select either repository while excluding tool messages:

```json
{
  "query": "database migration",
  "filter": {
    "all": [
      {"provider": ["codex", "claude-code"]},
      {"any": [
        {"repository": ["https://github.com/kojah/mash.git"]},
        {"project": ["/home/james/scratch/mash"]}
      ]},
      {"not": {"role": ["tool"]}}
    ]
  }
}
```

The CLI accepts the same expression through `--filter '...JSON...'`. Convenience
flags and the structured expression are combined with AND. Filters apply before
lexical/semantic candidate limits in local and shared Qdrant, then the
combined results are reranked. Raw SQL, regular expressions, and glob patterns
are not supported. Limits are eight levels of nesting, 128 expressions, and 100
values per field list.

Project means the chat's first recorded working directory. Repository means
the chat's logged Git repository URL, or the locally available Git origin for
that directory at ingestion time. They are chat metadata; the first
directory applies to the whole chat. Unavailable metadata remains unknown,
so a positive filter does not match it and a negated filter does. Date filters do
not match messages without timestamps. Incompatible schemas are rejected untouched; follow the
[upgrade guide](../docs/upgrades.md) to preserve retained history.
