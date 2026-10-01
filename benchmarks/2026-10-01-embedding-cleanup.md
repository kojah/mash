# Does conversational filler removal improve local embeddings?

Initial controlled results do **not** support enabling filler removal for
document embeddings. The installed local model already retrieves the selected
real requests without cleanup. Synthetic tests show small ranking regressions,
not gains. This is a bounded pilot, not an exhaustive search-quality evaluation.

## Method

The ignored `semantic::evaluation::cleanup::benchmark_paired_embedding_cleanup`
test accepts a strict, versioned TOML corpus of explicitly paired raw/cleaned
documents and positive target-query judgments. It requires installed local
models and a fresh explicit output path, caps documents/queries/input sizes,
rejects unknown fields and missing/duplicate relevance IDs, and permits only
nonempty contiguous retained text. It writes typed private reports with corpus
fingerprint, model generation, quantized-vector cosines, inference times, first
target rank and recall@5. It never changes a database, canonical message,
search projection, config or production embedding policy.

Queries keep fixed singleton vectors. Documents are inferred twice using the
same order and positions: once unchanged, once cleaned. Both singleton inference
and the existing document-batch scheduler are measured; this distinguishes text
effects from the previously observed batch-neighbor sensitivity. Candidates use
exact cosine ranking on the same per-vector quantized representation used by
mash. No lexical fusion, reranker, filters, thread grouping or abstention is used.

Installed CPU model generation:
`ca9c7766f955402eb34db3c324da24a512e1118de44e3d9aa9a0faeeac0da5f4`.
Runs used Cranelift test binaries on Linux, with no downloads.

Three corpora were frozen before their inference runs:

- **Light synthetic:** the 17 public documents from
  `docs/fixtures/search-relevance.toml`, with whole `Hi!`, `Sure, sounds good!`
  or `Thanks!` leading clauses. Eight unchanged controls cover Hello World,
  Go reflect Kind, parser field names, paths, negation and an option approval.
  There are 25 documents and 21 positive queries, including technical identity
  distinctions from the existing fixture. Filtering/negative queries are omitted.
- **Heavy synthetic:** identical documents/judgments, replacing the injected
  prefix with `Hi! Thanks for helping. Sure, sounds good! Let’s keep going.`.
  Cleanup removes exactly that known prefix, an idealized oracle rather than a
  tested general-purpose automatic cleaner.
- **Reviewed requests:** all 17 qualifying short user messages in the frozen
  SQLite corpus that have a complete greeting/acknowledgment first line followed
  by substantive content: 14 Codex and three Claude Code. Each complete message
  was inspected before authoring its target-query description. Only that first
  line and surrounding whitespace are removed. A seeded sample of 50 unchanged
  short user messages per provider supplies background candidates, giving
  117 documents and 17 target queries. Technical content and negation remain
  intact. These are source-finding judgments, not exhaustive semantic relevance
  judgments for every background message.

## Retrieval results

| Corpus | Document inference | First-rank targets, raw → cleaned | MRR, raw → cleaned | Rank improvements / regressions |
|---|---|---|---|---|
| Light synthetic | Singleton | 20/21 → 19/21 | 0.9643 → 0.9405 | 0 / 1 |
| Light synthetic | Document batch | 19/21 → 18/21 | 0.9405 → 0.9087 | 0 / 1 |
| Heavy synthetic | Singleton | 20/21 → 19/21 | 0.9683 → 0.9405 | 0 / 2 |
| Heavy synthetic | Document batch | 20/21 → 18/21 | 0.9683 → 0.9087 | 0 / 3 |
| Reviewed requests | Singleton | 17/17 → 17/17 | 1.0000 → 1.0000 | 0 / 0 |
| Reviewed requests | Document batch | 17/17 → 17/17 | 1.0000 → 1.0000 | 0 / 0 |

Mean recall@5 is 1.0 for every row. Synthetic regressions concern near-tied PR,
issue and URL identities. Their occurrence does not imply full mash search
regresses: lexical/entity handling and reranking are deliberately absent here.
The real-request queries already have perfect target ranks, leaving no measured
headroom in this selected pilot.

Median raw/cleaned cosine is about 0.912–0.918 for synthetic documents and
0.953–0.957 for real requests. Considerable vector movement does not translate
into better target ranks. Unchanged control vectors are identical under
singleton inference (within floating-point cosine rounding), but their minimum
cosine is 0.982–0.989 under document batching. Cleanup changes batch context as
well as words; singleton results are the cleaner isolation of text effects.

Real-request document inference took 266 → 181 ms for singleton and
346 → 323 ms for document batches in these one-shot runs. These timings exclude
model loading and are sensitive to warm-up, execution order and host load. They
do not establish a throughput gain or justify production preprocessing.

## Decision and remaining work

Keep production embedding inputs unchanged. Complete boilerplate exclusions,
topic-word weighting and source-aware context recovery remain separate trials.
An option approval cannot safely be treated as disposable filler merely because
it lacks subject nouns; the accompanying
[topic-context experiment](2026-10-01-topic-option-context.md) tests that case.

The open `mash-c0d.19.2.17` task still requires a larger independently judged
real-history corpus, held-out relevance and negative queries, long-input/token
budget cases, retrieval margins and full hybrid/reranked message/thread search.
Any adopted projection needs explicit versioned generation/cache identity and
reindex behavior. This pilot evaluates whole-clause removal only; it supplies no
evidence for POS pruning, IDF-based token deletion or generative rewriting.

Private corpora and reports are under
`/home/james/scratch/mash-artifacts/embedding-cleanup-20261001`. Corpus fingerprints
for light, heavy and reviewed inputs are respectively:

- `ec7c54a24079af419ecaf58c542b24d607ca076db25c7b52c4afc6bd4272ec22`
- `ff6798f480e3b46bf58f8e74c0760aa3c2c6896a900ae42fad203fb9a994ab58`
- `7e1aa4410c9a89a5f52b8d770e2b660ab490362c06528638619062a70c23ce7e`

Run a reviewed paired corpus with explicitly installed models:

```sh
MASH_TEST_MODELS_DIR=/path/to/installed/models \
MASH_TEST_CLEANUP_CORPUS=/path/to/reviewed-pairs.toml \
MASH_TEST_CLEANUP_REPORT=/path/to/new-report.json \
cargo test-clif --lib --run-ignored only -E 'test(benchmark_paired_embedding_cleanup)'
```

Each TOML document has `id`, `raw` and `cleaned`; each query has `text` and a
nonempty `relevant` ID array; the top-level `version` is 1. Private histories must
stay outside Git. All three manual trials passed. The 97 focused topic tests and
all-target/all-feature Clippy also pass with the evaluation harness.
