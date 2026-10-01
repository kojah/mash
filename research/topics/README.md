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
Automatic coherence is a diagnostic, not a replacement for source review.
Select an approach only after reviewing these tradeoffs against a frozen baseline;
no numeric quality acceptance threshold has yet been justified by annotations.
Then specify bounded online insertion, edit/delete, restart and model-version
behavior before porting to Rust or exposing topic MCP tools and filters.

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
