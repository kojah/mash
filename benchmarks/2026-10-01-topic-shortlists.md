# Diagnose topic-label candidate loss before changing IDF

Work on `mash-c0d.19.2.10`, following
[visible Markdown label text](2026-10-01-topic-visible-links.md). Rust test-only;
production indexing, search, CLI and MCP are unchanged.

## Existing weighting and diagnosis

Labels already use class-based TF-IDF. Each supporting thread contributes a term
once to each retained topic; the class weight is:

```text
(class_count / class_total) * ln(1 + average_class_total / global_count)
```

This rewards distinctive terms, but distinctiveness alone does not identify
useful domain names. In the reviewed full-forward pipeline topic, 403 supporting
terms compete for 64 slots. Deadlock and runtime each have support from two
threads, yet neither reaches the TF-IDF shortlist. Rare long phrases take those
slots; semantic ranking cannot select an excluded term.

Add a bounded, typed private label diagnostic using the same scoring function
as normal ranking. It records every shortlisted candidate's TF-IDF, cosine and
combined score, selected labels, and explicit probe states: absent from support,
excluded from shortlist, or shortlisted with rank and selection status.

The ignored `topics::evaluation::review::benchmark_label_candidates` requires a
frozen SQLite snapshot, explicit compatible installed models, private output
directory, selected-thread TOML and `MASH_TEST_TOPIC_LABEL_PROBES` TOML containing
`terms`. The selection remains bounded to 64 threads/16 KiB; probes to 32
distinct nonempty terms of at most 192 bytes/16 KiB. Capture at most 64 topics,
64 candidates per topic and 8,192 supporting terms per topic. Require complete
supporting-word capture (`MASH_TEST_TOPIC_HIERARCHY`); fail rather than inventing
absence when that evidence is missing. IDs and candidate text never enter stdout.

The diagnostic covers representative radius 0.4 in both orders, with optional
reserved-model mode. It loads no new assets and adds no inference for candidate
inspection. Shared corpus preparation and shared score calculation avoid a
second interpretation of the pipeline.

## Balanced shortlist experiment: rejected for adoption

Opt-in `MASH_TEST_TOPIC_BALANCED_LABELS=1` reserves half the fixed candidate slots
for single words, ordered by supporting-thread count and then TF-IDF. Remaining
slots prefer TF-IDF phrases without contained phrase variants; spare capacity is
filled from the original list. Reports serialize typed `label_shortlist` policy.
Any present flag value selects the experiment; unset it for the TF-IDF control.
The default remains unchanged. This policy is retained only to reproduce the
rejected experiment, not recommended as a production configuration.

The full-forward diagnostic now shortlists deadlock and runtime, but they rank
29 and 28 respectively (zero-based). Their cosine scores are about 0.268 and
0.216; runtime falls below the existing 0.25 acceptance threshold. Deadlock's
combined score is about 0.268, too low for the eight selected labels. Generic
planning wording remains. Phrase preselection also loses the stronger recall-audit
phrase while adding harness-scan wording. The named-session topic gains a greeting
label. More candidate coverage is not sufficient evidence of better label quality.

Do not increase IDF or promote this shortlist on those results. The current
evidence requires a better phrase shortlist and a trustworthy semantic comparison
before changing weights. Overall domain/secondary-subject recall and the full
source-based quality gate remain open.

## Score variation requires controlled investigation

Identical phrases have different cosine scores between the two candidate-pool
runs: differences reach about 0.020 in the inspected topic. A greeting moves from
0.237 to 0.253 and crosses the acceptance threshold. Six captured topic-label
lists also differ between the focused diagnostic and the complete benchmark,
whose ancestor labeling changes the earlier label-vector cache contents and
subsequent missing-phrase batches.

The source pipeline uses the quantized MiniLM model and fixed eight-text CPU
inference batches. These observations do not isolate quantization, padding,
pool scheduling, cache order or a mapping defect as the cause. Record the
diagnostic/benchmark discrepancies explicitly; do not silently treat their
label lists as equal. `mash-c0d.19.2.11` will control phrase inputs, neighbors,
batch sizes, repeated calls and raw-versus-stored vector stages, and evaluate
stable bounded label inference with relevance and cost measurements.

## Validation and cost

548 regular tests pass with 108 skipped, plus all 77 topic-related tests,
all-target/all-feature Clippy, doctests, formatting and diff checks. Fixtures
cover candidate inspection including low-similarity terms, score consistency,
strict probe bounds, rare-phrase saturation, phrase diversity and small vocabularies.

Same 789-thread snapshot/generation/canonical split: all 16 TF-IDF controls
reproduce preceding visible-link reports except timings and new policy metadata.
All 16 balanced reports preserve structural fields, memberships, ancestors and
grounded source evidence exactly, excluding label content/model accounting/timings.
Distinct thread/provider and forest audits pass. Eight focused label reviews
validate candidate score arithmetic, bounds and probe states; six diagnostic-versus-
benchmark label discrepancies remain explicitly recorded.

Primary radius 0.4 full candidate vocabularies shrink from 8,977/9,086 to
7,775/7,728; reserved from 8,285/8,317 to 7,036/6,998. Label word sets change for
149/147 full and 144/141 reserved active topics. Named-topic coverage is unchanged.
None of those aggregate counts establishes relevance.

Eight-run full diagnostics take 32.77 s control and 30.46 s balanced; reserved
33.40 s and 25.32 s. Peak RSS is roughly 1.07–1.09 GiB. Host contention varies;
these pairs do not establish a production speedup, indexing overhead or search
latency. Private artifacts are in `mash-artifacts/topic-shortlist-20261001` and
`mash-artifacts/topic-balanced-20261001`; real identifiers and raw histories stay
outside Git. Runtime processing is Rust; Python audits artifacts only.
