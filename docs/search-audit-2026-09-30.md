# Search quality audit: 2026-09-30

## Method

The initial diagnostic matrix tested 22 query classes against Xenia's SQLite
index, local-only and local-plus-shared (44 requests), before and after the
literal-query/reference changes. Queries covered topics, URLs, identifiers,
multiple concepts, questions, paraphrases, deployment, progress, metadata,
typos, hyphenated names, quoted phrases, negation, short queries, follow-ups,
no-match phrases, nonsense identifiers, Japanese, punctuation, stopwords, and
PR numbers. Searches used a 14-day window, user-role messages, five threads,
and `--no-worker`. These timings include model loading.

A separate 24-request filter matrix covered providers, roles, conversations,
nodes, projects, repositories, dates, and nested Boolean expressions in both
modes. Two project-and-NOT-project cases returned no results. Live MCP checks
exercised URLs, PR numbers, and punctuation errors.

This is a diagnostic sample, not an exhaustive relevance benchmark or a
measured precision/recall score. Private history captures remain outside Git.

## Defects and fixes

| Query / boundary | Observed failure | Implemented correction |
| --- | --- | --- |
| Complete PR URL | SQLite FTS syntax error | Quote literal tokens; preserve quoted phrases |
| Natural question ending in `?` | SQLite FTS syntax error | Same literal compilation |
| `voyage-code-4` | Hyphens interpreted as FTS syntax | Same literal compilation |
| `!!!`, `the and of` | Error or irrelevant topic results | Typed usage error before inference |
| `PR 180` | PR 183 ranked first; incidental numeric 180 leaked | Match explicit PR/issue identifiers; retain exact references outside reranking budget |
| `purple platypus quantum waffle` | Five irrelevant threads | Joint cosine/reranker evidence gate in both result modes |
| `qzxv981723zznohistory` | Five irrelevant threads | Require literal mixed alphanumeric identifiers |
| Model-warmth paraphrase | Unrelated agent-framework discussion ranked above caching | Bounded nearby-message context for reranking |
| `search without SSH` | Parallel-search question selected as evidence | Contextual reranking selects the SSH-removal request |
| Automatic review/compaction transcripts | Injected history appeared as user evidence | Exclude dedicated injected-message prefixes and browser context |
| Japanese query | Unrelated English greetings | Reject unsupported scripts before inference |
| Old shared metadata | Project-filtered hits returned absent project metadata | One-time, resumable Redis snapshot metadata repair from canonical records |

The targeted local follow-up checks returned zero results for both nonsense
queries, ranked the actual caching discussion first for the warm-model query,
selected the explicit SSH-removal message, and returned both correct PR 180
discussion threads. The full URL also returned both threads.

The evidence gate is a model-specific heuristic, not a calibrated probability:
lexical and exact-reference evidence remain eligible; semantic evidence requires
positive reranking evidence or sufficient cosine similarity together with a
non-extreme negative reranker score. Useful negative-score paraphrases are
explicitly preserved. A labeled evaluation should grow alongside model/corpus
changes (Beads `mash-jh7`).

## Boundaries and refresh

Reranking reads at most four nearby messages for each of 30 thread candidates
(or 10 message candidates), with fixed character budgets. Shared context calls
have bounded concurrency. Canonical `view` output is unchanged; the internal
HTTP view request can select searchable projections. Context errors preserve
local fallback and surface through shared-search status.

The language guard recognizes unsupported scripts. Accented Latin text and
technical identifiers remain accepted; it does not reliably identify every
non-English language written in Latin characters.

SQLite schema 17 invalidates old checkpoints/readiness so users explicitly run
`mash index` to refresh projections and embeddings. Search never performs this
migration or ingestion itself. Shared checkpoint fingerprints likewise change.
Existing remote nodes need an explicit refresh to remove old injected records
from their indexed candidate sets.

Redis repairs conversation payloads from canonical snapshots without inference,
revision changes, or freshness changes. Completion is acknowledged only after
all records are repaired and only for the unchanged snapshot. Interrupted
repairs resume; concurrent publication cannot recreate retired message hashes.
Missing upstream metadata remains missing rather than being guessed.

## Regression validation

178 tests passed with real local models and disposable Redis Search and
PostgreSQL/pgvector services; two performance benchmarks were intentionally
excluded. Tests cover literal FTS queries, reference precision/recall, abstention,
unsupported scripts, projection exclusions, migration readiness, projected
context over both HTTP backends, and idempotent Redis metadata repair preserving
freshness. Formatting, Clippy, doctests, and reproducible query generation are
required before committing.

The final refreshed-corpus matrix ran all 44 requests against schema 17 and an
updated temporary HTTP server on the existing shared Redis data. Each mode had
19 successful searches and three intended typed errors (unsupported script,
punctuation, and stopwords). Both no-match cases returned zero; both PR 180
threads were retained. No shared transport/context errors occurred.

The private explicit refresh took 501 seconds: ingestion 2.3 seconds, derived
search refresh 4.2 seconds, and changed-record embedding 493 seconds. It retained
102,666 searchable messages. Production node deployment and provider expansion
are tracked separately in Beads.
