# Complete-ancestry turn replication

Issue mash-7w0. This is a development-only experiment, separate from production
indexing and search. The original visible-only experiment remains historical.

## Frozen replication procedure

Keep the original 48 development threads, 5,498 message identities, texts,
vectors and sparse references unchanged. Do not inspect or score the 24 reserved
threads. Capture complete local silver histories in one read-only transaction
and capture the corresponding provider files privately. Resolve omitted parents
through canonical ancestry to the nearest eligible visible ancestor. Preserve
explicit roots and branches. Hidden assistant activity counts as starting a
response; hidden user tool results do not. Reject absent/future parents, changed
source text, omitted visible messages, duplicate coverage and native mismatches.

Verify Claude UUID identities and parents against captured native JSONL. Codex
uses chronological canonical order: native text/hash presence is a supplementary
check, not independent per-occurrence duplicate reconciliation. Revalidate all
capture hashes and source identities when running or summarizing results.

Use the same installed quantized MiniLM, tokenizer, batch size eight and single
inference thread, four representation recipes, twelve representation-screen
settings and 27 unchanged algorithm settings as
[the original protocol](turn-embedding-experiment.md). No new thresholds,
models, LLM calls or independently invented labels. Record capture SHA-256 in
both the report and gold artifact. Cache reuse is permitted only by exact model
generation and input-text hash. Publish in a new private artifact directory.

All original-message pair scores remain comparable. Cross-turn scores must be
recomputed for every baseline using the repaired boundaries: the cross-turn
subset has changed, so old and new cross-turn scores are not directly comparable.
The original quality gates still apply; agent-authored sparse labels and a manual
agent audit do not constitute independent human validation.

## Boundary audit

Captured 39,689 canonical records: 11,443 Claude and 28,246 Codex. All 5,498
visible frozen messages match current silver exactly. Claude native verification
checks all 11,443 canonical identities and parents with zero mismatches.

| Provider | Original turns | Complete turns | Original orphans | Remaining orphans |
|---|---:|---:|---:|---:|
| Claude Code | 1,725 | 347 | 1,378 | 1 |
| Codex | 681 | 681 | 15 | 15 |
| Total | 2,406 | 1,028 | 1,393 | 16 |

Ancestry alone yields 343 Claude turns. Recognizing omitted assistant response
activity splits four more prompt bundles, yielding 347.

Audit selection is deterministic: two paired bundles in every provider/length
stratum (12), all remaining orphans (16), and all four boundaries added by hidden
assistant activity. Review source context and canonical parent witnesses. The
32-case review is by the development agent, not an independent human annotator.

The remaining Claude orphan follows an explicit native root reset; attaching it
to the immediately preceding prompt would cross that root. Codex orphans have
no eligible captured user prompt, often because setup/worker instructions were
excluded. Preserve these as assistant-only runs.

Important representation limitations remain. Visible interruption markers and
local-command envelopes can appear among prompts; image-only messages can lack
useful textual content. One inspected Codex turn contains 160 assistant segments
after one prompt, spanning autonomous work on several subjects. A correctly
delineated turn is therefore not necessarily an atomic topic. Keep source text
unchanged for this replication and report these qualifications rather than
silently adding cleaning heuristics after viewing results.

Private captures and the reproducible audit are under
`/home/james/scratch/mash-artifacts/turn-ancestry-audit-20261005`.

## Completed replication

The corrected cross-turn reference subset has **8 related, 48 unrelated and 3
ambiguous pairs**, with positives in only seven threads. Previously it had
57/59/37, with positives in 24 threads. Most formerly cross-turn positives were
continuations of the same actual prompt/answer turn. All original-message
reference pairs and the baseline all-pair scores remain unchanged.

| Representation screen, average-link .35 | Cross-turn positive-thread macro F1 | Cross-turn micro P / R / F1 |
|---|---:|---|
| Prompt-only | .400 | .500 / .500 / .500 |
| Joint truncation | .852 | .250 / 1.000 / .400 |
| Prompt-conditioned passages | .852 | .276 / 1.000 / .432 |
| Pooled message vectors | .805 | .258 / 1.000 / .410 |

Joint truncation wins the frozen screen's tie by recipe registration order;
this is not evidence that truncation is generally better than complete passages.
The fixed 27-setting retry selects BIRCH radius .60 / membership cosine .35 by
the registered positive-thread macro F1 criterion (.924). Its eight true matches
and 16 false matches give micro precision .333, recall 1.000, F1 .500. That
criterion ignores false matches in negative-only threads; micro counts expose
the problem. Affinity propagation .25 failed to converge on one thread, so that
setting is excluded. No fit timed out; no thresholds were added.

| Algorithm family: best registered macro setting on joint truncation | Cross macro F1 | Cross micro P / R / F1 |
|---|---:|---|
| Leader .50 | .805 | .267 / 1.000 / .421 |
| Average-link .35 | .852 | .250 / 1.000 / .400 |
| BIRCH .60 / .35 | .924 | .333 / 1.000 / .500 |
| HDBSCAN size 3 / .50 | .286 | .222 / .250 / .235 |
| Complete-link .35 | .829 | .292 / .875 / .438 |
| DP-means .65 | .757 | .211 / 1.000 / .348 |
| OPTICS .03 | .767 | .205 / 1.000 / .340 |
| Spectral k=4 | .686 | .231 / .750 / .353 |
| Affinity .50 | .686 | .353 / .750 / .480 |

The chronological eight-message baseline has cross-turn micro P/R/F1
.556/.625/.588. A **post-hoc micro ranking**, not the registered selection,
finds complete-link .65 best: six true matches, five false matches, two misses;
P/R/F1 .545/.750/.632. Its .044 micro F1 improvement over chronology is small
and based on only eight positives. It does not meet precision .85 or the
required .10 gain. Do not promote or continue parameter tuning on these labels.

Turn grouping alone now has all-pair macro P/R/F1 .832/.910/.850 and micro
.824/.928/.873. As in the earlier experiment, repeat the **post-hoc bundle
union** on the registered BIRCH winner, separately from selection. All 48 plain
refits reproduce the registered counts; union introduces no cross-turn matches.
The union's all-pair macro P/R/F1 is .771/.979/.847 and micro .745/1.000/.854.
It is slightly worse than turn grouping alone and fails precision .85. The
standalone BIRCH view's all-pair macro F1 is .358 because the distinct-turn
support rule excludes many within-turn relationships; it is not a replacement
for the structural turn view.

**Decision:** ancestry repair is validated for this cohort; clustering remains
experimental. Stop this sweep. The next useful step is independently authored
references containing substantially more genuine cross-turn positives and hard
negatives, with a selection metric that accounts for negative-only threads.
Keep the reserved cohort closed until that protocol is frozen. Do not infer
production retrieval quality or choose a default representation from this run.

## Cost and provenance

| Recipe | Inputs | New vectors | Build seconds | Original character coverage |
|---|---:|---:|---:|---:|
| Prompt-only | 1,136 | 1,105 | 49.23 | 14.97% |
| Joint truncation | 1,028 | 974 | 66.46 | 62.40% |
| Prompt-conditioned passages | 2,796 | 1,544 | 179.08 | 99.97% |
| Pooled control | 1,028 | 0 | .10 | No new text inference |

Costs are incremental in recipe execution order, with exact-text cache reuse.
Joint truncation discarded 228,351 tokenizer tokens. Passage inputs used 679,309
tokens including special tokens, repeated prompts and overlap. The 665 uncovered
characters are the same whitespace gaps as before. The fixed 64-message probe
has mean cosine .99227, minimum .98484 against stored vectors; numerical Rust
parity is not established.

The full run took 635.32 seconds, including complete-input verification,
embeddings, message controls and isolated fits. Artifact size is 40.50 MiB;
cache verification/reuse took .071 seconds with zero new inference and identical
vectors. These are single-run Python cohort measurements, not cold production
latency estimates. Timing includes SQLite cache transactions and host I/O.

Private report, summary, post-hoc overlay and exact pre-run protocol bytes are
in `/home/james/scratch/mash-artifacts/turn-ancestry-replication-20261005`.
The private `turn-ancestry-replication-analysis.py` reproduces the bundle union
from cached vectors and checks plain refit parity; it performs no inference.

- Corpus SHA-256: `a4f84e869808f6ae243a973de125cfd25ccd4738e86f63e359f6758c41554773`
- References SHA-256: `dc66aa568ab442e84abc810778e0c0a2abb4e102b0a66bbff4fa9f183d212732`
- Capture SHA-256: `92ab77282386e567febd413b857b49b5edb0248086c62a4960522cb8618639db`
- Pre-run protocol SHA-256: `67852db2a62e9ffb3042bdd4b74ed3ba3d8cc789cc2f068c82ee0e2490baed78`
- Report SHA-256: `2a475bf8d1472b9fd42934f00de95bbdb5433555c329de33de646ef62274e7c0`

The protocol digest covers the original turn protocol, a NUL separator and the
pre-results ancestry procedure, frozen in `protocol.txt`; this living document
now includes results. The 32-case review is tied to audit SHA-256
`3a1f60df5655bfb1d37dd23fc3eba03e6a7cd51b4764cf83586b5c90285cb48f`.
