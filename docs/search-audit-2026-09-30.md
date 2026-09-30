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
| Refreshed-corpus caching query | Relevant message was fourth within its thread and fell outside the old three-anchor budget | Select promising threads before ranking up to six distinct evidence anchors |
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

Thread selection first ranks at most 20 grouped documents, retaining up to six
candidate messages per thread. Individual evidence is then reranked for five to
ten promising threads (up to 60 messages); message mode evaluates ten candidates.
Each evidence document includes at most two nearest searchable messages with fixed character
budgets. Shared context calls have bounded concurrency and a one-second aggregate
deadline; completed context survives stalled requests. Interactive HTTP reads
have a two-second timeout, separate from the streaming publication budget.
Canonical `view` output
is unchanged; the internal
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

182 tests passed with real local models and disposable Redis Search and
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
threads were retained. No shared transport/context errors occurred. The matrix
was repeated after length-bucket batching and shorter context changes; caching
and SSH-removal evidence remained first in both modes. A short-window regression
also verifies that late query terms remain inside 160-character inputs.

A separate warm-worker sample used seven distinct queries twice per mode,
excluding the first request from each mode. Its 13 warm requests had median
latencies of 479 ms local-only and 696 ms local-plus-shared, with maxima of
677 ms and 864 ms respectively. Local startup was 2,180 ms; the first combined
request was 535 ms with the worker already warm. These are Xenia measurements
using a loopback shared HTTP service, not cross-machine network latency.
Length-bucket batches of four and smaller nearby-message windows reduced the
earlier roughly one-second warm inference cost while retaining the audited
evidence. No query-result cache is needed for this improvement.

The private explicit refresh took 501 seconds: ingestion 2.3 seconds, derived
search refresh 4.2 seconds, and changed-record embedding 493 seconds. It retained
102,666 searchable messages. Production node deployment and provider expansion
are tracked separately in Beads.

## Repeatable synthetic relevance matrix

`docs/fixtures/search-relevance.toml` contains 17 synthetic threads, 20 native
Claude messages and 16 explicit query judgments. The typed core ingests the
fixture and searches every query in message and thread modes using explicitly
installed real models. No private histories are included. Run it with:

```bash
MASH_TEST_MODELS_DIR=/path/to/explicitly/installed/models \
  cargo test-clif --run-ignored only -E 'test(labeled_synthetic)' \
  --success-output immediate
```

The fixture covers paraphrases, fourth-message thread evidence, multiple concepts,
hyphenated identifiers, quoted phrases, exact PR/issue URLs and numbers, unrelated
hard negatives, no-match queries, injected-context exclusion and conversation
filters. Every positive query must have a relevant first result; explicitly
forbidden results and nonempty abstention cases fail the test. Precision and
recall are printed alongside each request's results and elapsed time.

The first run exposed three defects: full reference URLs admitted other
repositories, quoted phrases admitted separated words through semantic search,
and `codex_internal_context` blocks remained searchable. URL constraints now
retain the host, repository, reference kind and number; bare PR and issue queries
also retain their distinct kinds. Quoted phrases require contiguous literal
tokens, with case-insensitive matching. Codex context blocks are excluded while
ordinary backtick-quoted examples remain searchable.

After these corrections, all 32 searches passed. On this small local fixture:

| Mode | Mean precision among up to 5 returned results | Recall@5 | MRR@5 |
| --- | ---: | ---: | ---: |
| Messages | 0.872 | 1.000 | 1.000 |
| Threads | 0.859 | 1.000 | 1.000 |

Metrics average the 13 positive query cases; the three abstention cases are
checked separately. They use audit-authored synthetic labels, awaiting independent
human review. Message relevance here is judged by conversation membership; this
does not measure every excerpt's relevance. These are local fixture measurements,
not production-corpus relevance or latency, and do not independently exercise the
shared transports. Broader human labeling stays open in `mash-jh7`.

SQLite schema 30 and source fingerprint `search-text-v4` require an explicit
`mash index` refresh so cached source records lose injected context. The migration
preserves canonical history and existing vectors while clearing old checkpoints
and invalidating search readiness. Searches never perform this migration or
ingestion themselves. Shared-only nodes also refresh their changed fingerprints;
canonical content remains available through `view`.

Final validation passed 326 tests with installed local models and disposable
Redis/PostgreSQL services; three manual benchmarks were excluded. Formatting,
Clippy with warnings denied and doctests passed. Migration tests also exercise
legacy backfills: invalidation runs after those backfills so old embedding
coverage cannot restore readiness for an outdated text projection.
