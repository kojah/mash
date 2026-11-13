# Search reference

[Documentation](../README.md) · [Project home](../../README.md)

Search evidence includes `lookup.view` and `lookup.list_attachments`: copy these
argument objects into the corresponding MCP tool. They preserve local/shared
origin, including this node's own data returned by a shared-only search. Both
compact and extended results keep them.

`list_chats` rows carry the same `lookup.view` object for the chat's first
message (by display position); it is absent only for a chat with no retained
messages. `view` accepts message IDs only. Chat IDs and message IDs are separate
namespaces, so `view` never reinterprets an ID: an ID that matches no message is
a `not-found` error (exit 13), and when it is a local chat ID the message names
that chat's first message ID.

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

`exact-mention` intent, the default, takes a query of several unquoted terms as
words: each (or an accepted prefix or typo alternative) is required in one
message, ranked by BM25, then recency. It loads no models, needs no dense
views, and merges this machine with the shared nodes by score. A single term or
a query wrapped in double quotes is instead verified as one literal, so
identifiers such as `voyage-code-4` are not split. Ranking and vector overrides are rejected for both. The shared
API applies the same rule to `exact-mention` requests, so a node older than
this change rejects unquoted multiword answers as nonliteral until upgraded.

Semantic (related-discussion) search supports English queries. Exact-mention
search does not use this model-language check. Unsupported scripts and confidently
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
- `node`, `provider`, `role`, `chat`, `project`, `repository`: lists of exact, case-sensitive values.
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
[upgrade guide](../upgrades.md) to preserve retained history.

Chat results fold copies. A message copied into forked chats (same provider,
role, text and message time) earns one chat result; the chat with the smallest
ID stands for the forks so lexical and semantic candidates fuse, and the other
copies are listed under the match's `copies`. A fork still appears by itself
for messages only it holds. Message results keep every canonical message.

## Tune semantic ranking

Optional `[views.query]` settings select the default `unit` (`"chats"` or
`"messages"`) and `limit` (1–200). Omitting them preserves chats and 20 results.
CLI/MCP search parameters override either setting without rebuilding the index.
`[views.query.fusion]` controls hybrid candidate fusion independently of views
builds: `rank_constant` defaults to 60 (1–1000), and `lexical_weight` and
`semantic_weight` each default to 1 (1–100). Higher weights favor that candidate
stream before contextual reranking. `recency_weight` (default 50, 0–100) favours
newer messages in the final order: recency is ranked like another candidate
stream and combined with the relevance order, at that percentage of
relevance's weight, after reranking and before paging. A newer
message can overtake comparably relevant older ones, but a strong old match
still outranks a weak new one; undated messages get no recency credit and 0
ranks by relevance alone. These query-only settings apply to local,
combined and fallback searches, take effect on the next request, and require no
index rebuild. Exact-mention search is independent of fusion settings.
Set `[views.query] ranking = "fusion"` to keep fused hybrid ordering and skip
contextual reranking and its context fetches. The default is
`"contextual-rerank"`. This query-only policy applies across local, shared and
combined searches through the same CLI/MCP core, using the same typed requests.
It changes no stored vectors or silver. Dense query embeddings still require
the installed local embedding model. Fusion skips loading the reranker;
local installation inventory validation remains required. Status reports `query_models` as
`"available"` or `"unavailable"` for current TOML query defaults,
separately from physical `models_installed`. Switching ranking reloads the
matching cached backend.
Use diagnostics to compare execution and results before choosing a default.
For one search, use `--ranking fusion` or MCP `"ranking":"fusion"`; the other
choice is `contextual-rerank`. Omission inherits current TOML. Explicit ranking
overrides require related-discussion intent, do not persist, and never rebuild
the index.
For one hybrid request, CLI `search` accepts `--fusion-rank-constant`,
`--lexical-weight`, `--semantic-weight` and `--recency-weight`; MCP `search` accepts a partial
`fusion` object with the corresponding underscore-named fields. Omitted fields
inherit current TOML defaults. Overrides are validated before model work,
apply to local/shared/fallback ranking, and are rejected for exact-mention.
For resolved policy and retrieval outcomes, use CLI `search --diagnostics` or
MCP `search` with `"diagnostics":true`. The optional report includes selected
artifacts, completed retrieval sources, fallback, result count and core time.
It does not run indexing or change search results.
See [silver enrichment and views configuration](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/silver-gold-configuration.md)
for the layer contract and current configuration scope.
