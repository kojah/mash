# Topic methodology lab

Topic modeling is the first priority. The agreed sequence is research compilation,
approach selection, Python experiments with recorded results, and Rust
implementation only after selection. This directory is an offline lab, not a
runtime dependency or a new CLI command. See the
[research compilation](../../benchmarks/2026-10-01-topic-quality-research.md).

Topic modeling must be fast enough to avoid materially slowing ingestion and must
not use an LLM. Exclude generative summarization, rewriting, labels and taxonomy
synthesis, including local LLMs and paid topic-inference calls. Reuse existing
local embeddings first; additional non-generative encoder work needs measured
quality gains and bounded costs. LLM-based prior art is context, not a candidate
implementation. Independent source review remains an evaluation requirement.

The provisional performance gate is no more than 10% ingestion-throughput loss
versus topics disabled on matched workloads. Measure the full path, including
context processing, extra encoder calls, clustering, label maintenance and writes,
for initial indexing, new threads, appended turns, edits and deletions. Keep model
assets, provider inputs, routing/backends and hardware fixed. Report warm/cold
runs, repeated timings, p95 update latency, CPU and peak process RSS, and distinguish
amortized label refresh from immediate processing. Current clustering-only timings
do not satisfy this gate. Quality and performance both need evidence before Rust
method selection or promotion.

## Shortlist and decision protocol

| Boundary | First comparisons | Reason |
| --- | --- | --- |
| Unit | Whole-thread means, bounded user episodes, turn-pair/context units | Long sessions mix subjects; short approvals need context without importing unrelated assistant topics. |
| Discovery | BIRCH reference, offline HDBSCAN, average-link control; then GRINCH/online alternatives | Separate geometry failures from label failures and quantify online tradeoffs. |
| Labels | c-TF-IDF; contrastive training-background weights; representative phrases/MMR | Preserve technical and secondary subjects while reducing conversational labels. |
| Hierarchy | Independently reviewed semantic parents and overlapping leaves | Tree topology alone does not establish ancestor correctness. |

Freeze native source-group splits before fitting or label selection. Annotate
primary and secondary subjects, irrelevant/context-only text, episode boundaries,
and parent relationships independently of algorithm outputs. Review full sources
where available and mark incomplete evidence explicitly. Keep a development set
and a reserved set; do not tune on reserved labels. Report provider and rare-topic
breakdowns, not just a single coherence score.

Record assignment precision, secondary-topic recall, label specificity, intrusion
judgments, unassigned coverage, ancestor correctness and insertion-order stability.
Record wall time, peak process RSS, inference calls, and incremental update cost.
The [annotation protocol](annotation-guidelines.md) defines source/context scope,
episode granularity, overlap, ancestors, uncertainty and independent review for
the next dataset; it does not retroactively alter the initial pilot.
Automatic coherence is a diagnostic, not a replacement for source review.
Select an approach only after reviewing these tradeoffs against a frozen baseline;
no numeric quality acceptance threshold has yet been justified by annotations.
Then specify bounded online insertion, edit/delete, restart and model-version
behavior before porting to Rust or exposing topic MCP tools and filters.

Update the [living LaTeX paper](../../paper/README.md) as experiments proceed,
including procedures, setup, citations, results, analysis and limitations.

## Source-pinned boundary development pilot

`episodes.py` accepts private TOML annotations with `schema_version = 1`, the
embedding `generation`, and `[[cases]]` containing `thread`, `source_sha256` and
ordered `[[cases.boundaries]]` records (`left`, `right`, `judgment`, `rationale`).
Judgment is `same`, `change` or `uncertain`; ordinals identify adjacent nonempty
searchable user turns. Source SHA256 covers every such turn's native ID, ordinal
and full searchable text using compact UTF-8 JSON. Every boundary must be present,
including uncertain ones. Reserved groups are rejected for this development pilot.

The controls are windows of three turns, adjacent cosine below 0.55 and current
segment-centroid cosine below 0.55, plus disjoint two-turn block means at the same
threshold. Blocks use one future user turn, never share input across the boundary,
and abstain on missing vectors or numerical cancellation. This needs bounded
lookahead/provisional decisions for an online implementation; it does not re-embed
context or resolve references. Nonempty unembedded turns remain in sequence;
cosine predictors abstain on adjacent boundaries and reset rather than silently
bridging missing evidence. Uncertain judgments are excluded from scoring;
abstention does not earn positive recall. This is user-only linear presentation,
not branch-aware dialogue segmentation or independently validated context labels.

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/episodes.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --annotations "$PRIVATE_ANNOTATIONS" --output "$PRIVATE_REPORT"
```

The first development pilot is recorded in the paper: four purposively bounded
threads, 29 boundaries, six uncertain. One coding assistant reviewed user texts
before these predictions, with prior corpus exposure; this is not blind human
ground truth. All controls need a broader independently annotated test set before
method selection or threshold tuning can establish quality.

The block comparison increased false splits from 14 to 16 while retaining all six
judged changes. The negative result is in the paper; thresholds/annotations were
not adjusted to improve it. The lab's signed-byte decoding was checked against
Rust, with exact reconstructed norm parity on all 6,714 stored user vectors.

## Re-embedded text context control

`text_context.py` uses explicitly supplied installed MiniLM ONNX/tokenizer assets,
without model downloads. It re-embeds each user turn individually as an inference
control, then compares disjoint blocks of up to two user turns joined by two
newlines. Singleton CPU inference uses one thread, attended-token mean pooling and
Rust-compatible symmetric signed-byte rounding. Inputs over 512 tokens are
rejected, never silently truncated. A bounded 1,024-input cache avoids repeating
identical work within the run. Every context records its component message IDs.

```bash
PYTHONDONTWRITEBYTECODE=1 HF_HUB_OFFLINE=1 OMP_NUM_THREADS=1 \
OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/text_context.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --annotations "$PRIVATE_ANNOTATIONS" --snapshot "$INSTALLED_MODEL_SNAPSHOT" \
  --output "$PRIVATE_REPORT"
```

The installed snapshot must contain `onnx/model_quantized.onnx` and
`tokenizer.json`. Reports pin their SHA256s, code/lock hashes, runtime versions,
source/annotation fingerprints, stored-versus-re-embedded drift, inference/token
cost, elapsed time and peak process RSS. These are private reports; source texts
are not committed. This is an offline development comparison of linear user
contexts with one-user lookahead, not native branch resolution or assistant-context
selection. Re-embedded nonempty unembedded turns can also differ from stored-vector
coverage; report that when extending the cohort.

On the existing four-thread pilot, re-embedded single turns yield six detected
changes and 15 false splits; concatenated blocks yield six and 17. This does not
improve boundaries at the fixed 0.55 threshold. The re-embedded singleton control
differs from stored vectors and is essential to interpretation. Results, costs
and limitations are recorded in the living paper.

## Expanded development source review

`review_packets.py` samples two canonical development groups from each provider ×
length stratum: short (3–10 nonempty searchable user records), medium (11–40).
It excludes prior annotation source groups and reserved groups, caps each turn at
4,096 characters and each thread at 40,000 UTF-8 bytes, and ranks candidates by
SHA256 of the fixed seed plus canonical group identity. Selection never reads
candidate model outputs. Insufficient strata fail explicitly. This bounded
sampling is deliberately not a representative corpus estimate.

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/review_packets.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --exclude-annotations "$PRIOR_ANNOTATIONS" --output "$PRIVATE_PACKETS"
```

Packets contain the complete selected searchable user sequence, native IDs,
ordinals, source hashes and stored-vector availability. The display guard redacts
standalone credential-like values without changing the source database or source
hashes. It can redact ordinary opaque identifiers and is not a complete secret
scanner. Review remains private; do not print entire packets or treat redacted
values as topic evidence. Preserve original source identity for annotation checks.

The expanded review adds eight conversations/84 records. It freezes source-only
admission and major/secondary subject judgments before model scoring, alongside
76 boundary judgments (40 known, 36 uncertain). The source review sidecar pins
packet/guideline hashes and supporting ordinals. One coding assistant reviewed
the searchable user text, with prior corpus familiarity; full assistant/native
branch context and independent human review remain incomplete. Detailed aggregate
procedures, counts and unchanged-parameter baseline results are in the paper.

## Preceding conversational context

`preceding_context.py` produces private source-pinned review packets for every
visible user anchor in a frozen development annotation file. Claude records follow
the snapshot's native parent ordinals; linear records follow preceding stored
order. Missing, forward, mixed or ambiguous ancestry fails rather than falling
back to a nearby assistant message. No future or sibling records are borrowed.
The packet walks at most 64 raw records to the previous visible user or root.
It displays only searchable user/assistant prose, with 16 KiB per-record and
32 KiB total display bounds, explicit omissions and the existing display guard.
Tool/developer text is not displayed; hashes of traversed canonical/searchable
records still make context edits detectable. Outputs must be outside Git, cannot
overwrite existing files, and stdout contains only aggregate counts.

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/preceding_context.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --annotations "$PRIVATE_ANNOTATIONS" --output "$PRIVATE_CONTEXT_PACKETS"
```

This verifies traversal of stored edges, not independent native-file fidelity or
semantic referent resolution. Reaching a previous user record does not prove that
the referent is resolved: that record may itself be an approval or generated frame.
Linear histories may contain inherited or compacted context. The source guard is
not comprehensive secret scanning, so do not print entire packets. Keep existing
user-only judgments frozen; any full-context judgments need their own evidence
scope. No topic/search input or production admission behavior changes here.

For chained replies, `--maximum-users 3` follows up to three preceding searchable
user records along the same causal path. The 64-record traversal and display
budgets apply to the entire walk, not separately to each interval. The default
one-user context pins remain unchanged. Three-user pins include the user limit;
neither mode treats a reached user bound as semantic reference resolution.

The expanded development capture contains 84 contexts: 56 reach the three-user
bound, 22 reach a root and six exhaust the raw-record budget. There are no display
omissions; the largest display is 11,727 UTF-8 bytes. A fresh capture reproduces
the packet exactly. This is stored-projection context: the earlier native audit
verified anchors and Claude anchor edges, not every preceding message body.
Separate source-pinned reference judgments are still required before contextual
embedding experiments. Do not turn reviewer-written interpretations into model
inputs or change the frozen user-only judgments.

## Source-backed reference judgments

`reference_review.py` validates a separate private annotation sidecar against the
exact chained-context and original source-review bytes. It requires every one of
the originally referential anchors exactly once, preserves anchor/source hashes,
uses a closed resolution enum and requires source evidence for resolved cases.
Evidence must match intact, unredacted display text and an identity on the
captured causal path. Sibling/future/absent evidence, omissions, edited hashes,
duplicates and incomplete annotation sets fail validation. The command prints
only aggregate counts; it does not generate representations or topic labels.

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/reference_review.py \
  --contexts "$PRIVATE_CHAINED_CONTEXT" --source-review "$PRIVATE_SOURCE_REVIEW" \
  --annotations "$PRIVATE_REFERENCE_REVIEW"
```

Focused source review classifies the 29 referential anchors as 20 resolved tasks,
three resolved metadata references, four interaction prompts and two unresolved
references. A reported number denotes a count of unresolved findings rather than
an issue identifier; named old sessions are selectors rather than evidence that
their topics are currently discussed. Corrections must not inherit rejected
entities or all assistant proposals. This provides evaluation targets for later
context selection, not measured topic improvement. The single coding-assistant
reviewer had prior source, method and prediction exposure; these are exploratory
judgments, not independent human truth. Preceding bodies use stored projections,
not comprehensive native verification. Original user-only judgments remain frozen.

## Native source fidelity audit

`native_audit.py` checks the same frozen development anchors against their current
native files without printing or storing source bodies in its report. It
reconstructs the application's BLAKE3 message IDs from Claude thread/UUID pairs
or explicit Codex raw-message and completed turn/item IDs. It requires exact role
and canonical-text SHA256 equality; a text-only match is insufficient. Unsupported
canonical tool/thinking forms and conflicting replay identities are explicit
failures. The completed-message subset ignores image blocks according to the
typed ingestion schema; it does not guess image contents or reconcile mirrors.

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/native_audit.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --annotations "$PRIVATE_ANNOTATIONS" --output "$PRIVATE_NATIVE_AUDIT"
```

Each source is limited to 64 MiB, 100,000 records and 8 MiB per record. Re-reading
exact bytes catches changes during capture. Reports pin whole-file/record hashes,
source IDs, annotation/code/lock hashes and recognized origin-field presence.
Claude anchor parent links are checked through at most 64 native metadata edges
to the closest represented message or root. Missing or conflicting edges are
reported. Codex linear ancestry is explicitly unverified. Current native bytes
need not equal the historical whole file used to create the snapshot; individual
matched records are the evidence. Native origin metadata does not establish
substantive intent, and source fidelity does not establish semantic topic quality.

All 84 expanded-review anchors match native identity, role and canonical text;
one requires a completed Codex attachment event. All 40 Claude anchor edges match
(36 parents, four roots). The report is byte-identical on a fresh repeat. The
existing admission and boundary judgments remain unchanged. Full preceding-chain
content verification, source-supported referent resolution and production
eligibility rules remain subsequent work, tracked in Beads and the paper.

## Lexical boundary ranking control

`lexical_boundaries.py` compares adjacent dense cosine, disjoint two-turn dense
means, adjacent word TF-IDF cosine, disjoint two-turn lexical means and a fixed
equal dense/lexical adjacent combination. Scores are dissimilarities; higher
means a more likely change. It does not tune a decision threshold or emit topic
tags. The lexical-cohesion motivation is related to TextTiling, but these controls
do not reproduce its token-sequence, smoothing, depth-score or paragraph policy.

```bash
PYTHONDONTWRITEBYTECODE=1 OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/lexical_boundaries.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --annotations "$PRIVATE_ANNOTATIONS" --source-review "$PRIVATE_SOURCE_REVIEW" \
  --output "$PRIVATE_LEXICAL_REPORT"
```

Frozen source-reviewed admission is an **oracle diagnostic**, not a production
classifier: only content/referential records enter TF-IDF fitting. All original
boundary positions remain; an inadmissible block member or missing/cancelling
vector causes abstention rather than bridging framing. The sidecar must match
every source ID, ordinal and text hash. TF-IDF is fitted transductively to the 68
admitted development records with unigrams/bigrams, lowercase, sublinear TF,
smooth IDF, L2 normalization, no stopword list, an explicit technical-token regex
and an 8,192-feature cap. This is not a safe incremental IDF-coordinate policy.

Reports give non-interpolated average precision, ROC AUC, prevalence and coverage
on known boundaries, plus common-support and provider breakdowns. Uncertain
judgments are excluded; abstentions are counted explicitly. Common support avoids
comparing rankings over different scored subsets. Two-turn blocks use one future
user turn and are user-only linear representations, not native-branch context.
No LLM or model inference is invoked. Source/annotation/code/lock/feature hashes
are pinned; vocabulary/source text is withheld. Fit/scoring costs and full-process
RSS do not measure complete ingestion overhead. Results and limitations are in
the living paper; no production method is selected from this small diagnostic.

## Group-aware dense cutoff calibration

`calibrate_boundaries.py` tests one fixed operating rule: maximize training
balanced accuracy (mean change recall and continuation specificity), breaking exact
ties toward the higher dissimilarity cutoff/fewer splits. Candidates are training
score midpoints and endpoints only. A split requires dissimilarity strictly above
the selected cutoff. Every canonical development group is excluded in turn from
all training scores/labels, including any alias thread; insufficient training
classes cause abstention. Uncertain judgments and missing scores never choose a
cutoff. The comparator uses dissimilarity >0.45 (cosine <0.55).

```bash
PYTHONDONTWRITEBYTECODE=1 OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/calibrate_boundaries.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --annotations "$PRIVATE_ANNOTATIONS" --source-review "$PRIVATE_SOURCE_REVIEW" \
  --output "$PRIVATE_CALIBRATION_REPORT"
```

This uses existing dense vectors and the frozen oracle-admission diagnostic only;
no lexical vocabulary/IDF is fitted and no inference is invoked. Report each
training/test group, cutoff, available training classes, out-of-group predictions,
provider metrics and abstentions. Do not replace fold results with a cutoff fitted
to all labels. Leave-group-out development results are still exploratory because
the cohort and single-assistant annotations have informed method choice. This is
cutoff selection, not probability calibration, semantic-topic validation or the
complete-ingestion performance gate. The operating rule is fixed before this
experiment's predictions; any later alternative needs a separately recorded trial.

On the expanded development review, out-of-group calibration reduces false splits
from 26 to eight but detects only five of twelve changes, missing seven in Codex.
This tradeoff is rejected; no cutoff is promoted or retuned after the result.
The paper records fold concentration, source-label limits and the next unit/context
review. Reserved groups remain untouched.

## Initial controls

`run.py` reads an existing consistent SQLite snapshot in read-only mode. It uses
stored compatible 384-dimensional quantized **user** embeddings, normalizes each
vector and averages unique searchable user texts per thread. Canonical duplicate
threads are represented once. A frozen Rust report supplies generation and source
group identities only: its cleaned topic vectors are **not** reused. Groups whose
first identity byte modulo five is zero form the reserved partition.

These are raw geometry controls. They retain conversational scaffolding and do
not perform inference, source cleaning, episode segmentation, overlapping topic
assignment, or semantic parent construction. BIRCH is scikit-learn's reference,
not a reproduction of Rust routing. HDBSCAN and average linkage are offline
comparators. HDBSCAN's `-1` fit labels remain noise. A common cosine-centroid
assignment diagnostic at 0.7 is neither HDBSCAN prediction nor calibrated
confidence. The label-only unigram/bigram c-TF-IDF control uses training texts;
stop words never rewrite embedding inputs. Initial parameters are controls, not
validated defaults or a selected winner.

Each fresh process emits a private JSON report with native member IDs, terms,
input/reference SHA256 fingerprints, versions, parameters, timing and RSS.
Keep reports and source material outside Git. Fingerprints include the exact
reference report bytes; use the same artifact across a comparison. Timing for
fit and labels excludes loading and interpreter startup; peak RSS includes them.

## Reproduction

Install the locked lab environment outside the repository:

```bash
UV_CACHE_DIR=/home/james/scratch/mash-artifacts/uv-cache \
UV_PROJECT_ENVIRONMENT=/home/james/scratch/mash-artifacts/topic-python-env \
uv sync --locked --project research/topics
```

Use explicit frozen input paths and a fresh output path:

```bash
OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/run.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --method birch --holdout --output "$PRIVATE_REPORT"
```

Compare `birch`, `hdbscan` and `average`, with/without `--holdout`, and with/without
`--reverse`: twelve separate-process controls. Keep CPU thread limits fixed.
The loader fails on generation/dimension mismatch, missing canonical identities,
invalid vectors or a live WAL. It never creates or modifies the source database.

The first twelve controls and their limitations are recorded in the
[results](../../benchmarks/2026-10-01-python-topic-controls.md). They do not select
a winner; independently reviewed dialogue units and topic labels come next.

Run lab input-boundary tests:

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python \
  -m unittest discover -s research/topics -p 'test_*.py'
```
