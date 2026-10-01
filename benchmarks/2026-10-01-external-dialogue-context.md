# Frozen external dialogue context comparison

Development experiment, 2026-10-01, Bead `mash-c0d.19.2.30` (still in progress).
No production method is selected. This complements exposed coding-session
diagnostics with an external task-oriented service proxy.

## Source and frozen procedure

[MultiWOZ 2.2](https://aclanthology.org/2020.nlp4convai-1.13/), upstream
`fe0c8e65cfcd8462bd33c86e35f21addc84ca82b`; README/schema/development
`dialogues_001.json` only. Exact SHA256 pins are in `multiwoz.py` and
[the source note](../research/topics/EXTERNAL_EVALUATION.md). No train/test or
separate action-annotation file was downloaded. Upstream discloses model-assisted
action correction with manual verification; this is not independent human
coding-topic gold. Do not execute upstream scripts.

The typed loader validates 512 dialogues, 7,690 alternating turns, 1,401 exact
value spans in Unicode and UTF-8, and 95 copies. Eight copied-value lists differ
from source-slot alias lists and remain untouched. There are 248 undeclared
`find_taxi` intents against declared `book_taxi`: preserve unknown values, exclude
affected turns from primary label evaluation, and never silently alias them.
Every user turn contains all eight service frames, so frame presence is unusable
as an active-topic label. State slots can be cumulative.

Before model outputs, freeze 32 canonical whole-role/utterance-content groups
using seed `mash-topic-multiwoz-v1-20261001`. No service stratification, annotation
features, output-driven tuning or resampling. All 512 source groups are unique.
Controls:

- Current: exact original USER utterance.
- Mean: normalize the sum of separately normalized current USER and immediately
  preceding SYSTEM vectors, each with equal weight.
- Joined: exact preceding SYSTEM, two newlines, exact current USER.

Contextual controls abstain at the root. Window policy is 2–512 tokens, no padding
or truncation; all selected inputs fit (maximum 80 tokens). Preflight the entire
fixed selection against the 1,024-unique-input encoder cap before inference.
Utterance-only typed input construction has no state/schema/intent arguments.

The evaluation proxy is the set of schema-declared non-NONE active services.
Cross-dialogue pairs with nonempty supported sets are positive if those sets
overlap, negative if disjoint. Unknown intents and no-active states are excluded;
same-dialogue pairs are excluded to avoid adjacency/source shortcuts. Labels enter
only evaluation, never the model input. AP and ROC AUC rank cosine scores; no
threshold selection or topic clustering is performed.

## Results

238 USER anchors; 14 unknown-intent anchors; 43 with no declared active service
(these categories can overlap); 191 known nonempty proxies. Supported active-service
counts are restaurant 47, hotel 57, train 54, attraction 48. This sample does not
evaluate taxi/hospital/bus/police topic quality.

Common model support is 206 anchors, excluding 32 roots. Common evaluation:
12,341 pairs, 3,448 positive (27.94%) and 8,893 negative.

| Control | Common AP | Common ROC AUC | Own pair count | Own AP |
| --- | ---: | ---: | ---: | ---: |
| Current | 0.464235 | 0.645505 | 17,606 | 0.494045 |
| Mean | 0.534925 | 0.710532 | 12,341 | 0.534925 |
| Joined | 0.555472 | 0.720130 | 12,341 | 0.555472 |

The primary matched-support comparison favors context on this service proxy.
Current own-support differs in anchors/pairs and is not a matched comparator.
Pair observations share dialogues and are statistically dependent; no confidence
interval, significance, semantic-topic or granular-label improvement is claimed.

## Reproducibility and cost

Installed Xenova all-MiniLM-L6-v2 snapshot
`751bff37182d3f1213fa05d7196b954e230abad9`; quantized ONNX SHA256
`afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1`,
tokenizer SHA256
`da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0`.
ORT 1.30.0/tokenizers 0.23.2, singleton CPU, one intra/inter-op thread,
384-dimensional mean pooling followed by symmetric int8 rounding and normalized
float64 direction. Python/Rust inference parity is not established.

650 unique local encoding calls, 206 cache hits per fresh run; no downloads,
generative calls or paid inference. Inference 1,419.98/1,294.29 ms,
tokenization 86.12/74.63 ms, experiment wall time after imports
2,503.06/2,264.26 ms. Total includes source validation, model initialization,
preflight, vector/pair/metric construction, but excludes capture serialization
and process imports. These noisy descriptive costs are not ingestion throughput,
latency or externally measured memory gates.

Private artifacts under
`/home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001`:

- `context-controls-final-first.json`, SHA256
  `62eff98049bd418f359500c5c2d7b9c4bab2f879d48b639dcb3e6dd8e8f74b0c`.
- `context-controls-final-second.json`, SHA256
  `9b66c60ae87d2b1210bce36d5cef56f8a923710884e914bde4e133fbacc3ac1e`.

The initial capture pair is retained under `context-controls-{first,second}.json`
with hashes `5018575ebe3c6d132a988dfb214c6c56d45ed889250e7064f348a95568544ac1`
and `740b196c2169f1d648f6ba1b5538e3ce6fbce715ee4c58ba132e359a74839db3`.
After adding explicit rejection of missing nonroot context, the final pair uses
fresh paths and new code pins. Its vectors, pair scores and metrics agree with
the initial pair; historical captures are not rewritten.

Complete capture equality excluding only explicit timings passes: exact vectors,
all own/common pair scores, input hashes, token counts, supports/abstentions,
source/code/asset pins and model call counts. This is Python repeatability,
not Python-to-Rust parity. Full corpus/utterances and captures stay outside Git.

```bash
env PYTHONDONTWRITEBYTECODE=1 OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
  /home/james/scratch/mash-artifacts/topic-python-env/bin/python \
  research/topics/external_context.py \
  --root /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001 \
  --snapshot /home/james/scratch/mash-artifacts/ci-contracts-2ba4629/models/.mash-assets-oio3h5/models--Xenova--all-MiniLM-L6-v2/snapshots/751bff37182d3f1213fa05d7196b954e230abad9 \
  --output /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001/context-controls-new.json \
  --compare /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001/context-controls-final-first.json
```

The runner refuses to overwrite captures. Code changes intentionally break pinned
repeat equality; preserve original run identities rather than laundering drift.
Run the original inference/repeat command with code at commit `1ba0bca`; the later
intent-annotation extension changes the loader's code pin. The inference captures
are preserved and reused by the [follow-up](2026-10-01-external-intent-specificity.md).
Synthetic tests cover source corruption, metadata separation, Unicode offsets,
copy differences, unknown values, alias consistency, contextual adjacency,
cross-dialogue proxy support and exact repeat drift. All 165 lab tests pass.

## Next decision

Keep bounded dialogue context in the candidate set, but do not select a production
method on broad service separation. Evaluate episode segmentation, multiple granular
aspects, labels and guided-search usefulness on independently judged coding sessions;
report per-domain behavior and group-level uncertainty. Cumulative intent semantics,
annotation mismatch and unknown model pretraining overlap remain limitations.
Reserve test data until method selection and require complete-ingestion performance
and Python/Rust differential gates before shipping.
