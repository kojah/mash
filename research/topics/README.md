# Topic methodology lab

Topic modeling is the first priority. The agreed sequence is research compilation,
approach selection, Python experiments with recorded results, and Rust
implementation only after selection. This directory is an offline lab, not a
runtime dependency or a new CLI command. See the
[research compilation](../../benchmarks/2026-10-01-topic-quality-research.md).

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
