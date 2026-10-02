# Lexical evidence granularity with matched source support

P0 bead `mash-c0d.19.2.20.8.3.3.3.9` remains in progress. This stage evaluates
representation under fixed batch average linkage; it does not select a
production clustering algorithm or topic taxonomy.
[Frozen protocol](../research/topics/LEXICAL_GRANULARITY_PROTOCOL.md).

## Method

Same162 QMSum training meetings,84,434 inherited ready sources,77,892
singleton-known sources and8,196 original physical blocks as the dense
granularity control. All88,336 originals remain accounted for, including3,902
unsupported originals outside inherited geometry. Unknown/mixed sources enter
geometry but do not become clean conditional gold. Every arm excludes positive
pairs from the SAME original physical block, retaining all/far/recurring
denominators7,978,214/7,923,483/84,323.

Reuse the immutable exact-marker source count ledger, original18,316-term
vocabulary and already fitted full-training-snapshot smoothed IDF. Pool counts
before sublinear TF, frozen IDF and L2 normalization. No new inference, LLM,
download, learned projection, stoplist/weight change, cutoff/window grid or
held-out read. The full-snapshot vocabulary/IDF is an initial-batch diagnostic,
not causal online fitting or an implemented shared feature-generation service.

Arms: original message counts; count sum within the exact previously audited
semantic five-turn/512-model-token/gap-stop boundaries; original16-turn/
512-token physical block count pooling. This isolates lexical evidence while
keeping the context window unchanged; it is not the earlier1024-lexical-token
retrieval experiment. Empty lexical rows stay in geometry and source accounting.
All use cosine average linkage in original order. Original lexical-block
topology/merge tables replay exactly and metric totals match within frozen
tolerance. No gold annotations choose merges, cuts or source content.

## Results

| Evidence | Broad lexical/dense purity | Far lexical/dense | Recurring lexical/dense | Zero lexical units |
|---|---:|---:|---:|---:|
| Raw message | .404743 / .402046 | .404963 / .402285 | .546292 / .546808 | 273 |
| Causal context | .493283 / .433803 | .491564 / .432418 | .607982 / .561856 | 5 |
| Physical block | .554138 / .475705 | .553862 / .475791 | .622208 / .561768 | 0 |

Lexical pooling/context improve conditional hierarchy structure over dense
views; individual lexical messages remain weak. Physical pooling remains the
most promising representation among these arms. This reproduces the original
lexical-block reference as a control, not a new independent dataset gain.
No significance, equivalence, coding-generalization or guided-search claim.
Original source words carry useful distinctions, but stronger hierarchy scores
do not establish good labels or semantic ancestors. Carry pooled lexical
evidence into label/membership evaluation rather than another granularity grid.
The previous failed coherence cut is not rehabilitated by these scores.

## Captures and independent audit

Artifacts outside Git under `/home/james/scratch/mash-artifacts`:

- First `topic-qmsum-lexical-granularity-first-20261002.json` SHA256
  `4d32e6b60f167d17b1e076c395601d0eafe5a251e8a82c99355ab78a1d6af068`.
- Repeat `topic-qmsum-lexical-granularity-second-20261002.json` SHA256
  `00bc58334782dbd6d7804f8694ac23c1ef274b77533c45d485ae923aa0616f5e`.
  Every non-timing field matches exactly; begin/end source pins unchanged.
- `topic-qmsum-lexical-granularity-audit-20261002.json` SHA256
  `4d8a93157d974bd0551c47180c1159e4dc65cbe259a106f00f69c267bde3dc25`.
  Scalar auditor reconstructs source word pooling, TF/IDF/L2, original native
  topic/span memberships, LCA class fractions, pair support, merge mean heights,
  global average minima, zero/NNZ counts and summaries. It checks486 trees,
  176,578 merges,4,723,068 scalar feature coordinates and47,958,060 positive
  pair-scope contributions repeated across arms/scopes. Max numeric discrepancy
  2.91e-11 in aggregate sums within existing relative/absolute1e-12 tolerance;
  max chosen-minus-global-minimum gap7.77e-16, below frozen1e-12. This checks
  minimum validity, not identical tie choices under scalar reassociation.
  Auditor SHA256
  `f8bc9beb3796221ef229240055afddf5200700200894c7079eaa1920fb8cc841`;
  reusable source-pair oracle SHA256
  `4c45bfd96f56238b6f6169104f69078aab6e10d4e28b4b7e06190dfdbda6a943`.
  Shared original count/IDF/vocabulary ledger and NumPy/SciPy sparse Gram;
  original tokenizer and dense-context boundaries were audited previously.

New fixtures preserve exact words and gaps, retain empty-row ownership, reject
invalid count/identity data, check conditional unknown-source fractions and
reject corrupted metrics/populations. Full enabled lab suite547 tests passes.
An initial handcrafted audit fixture mistakenly indexed compacted memberships
with original ordinals; fixed the fixture to retain its full ten-source ledger.
Neither corpus code nor results changed.

## Costs and limitations

First/repeat after-import wall30.173/29.858s, CPU24.746/24.990s,
setup14.492/14.668s. Total raw/context/block feature transforms across162
meetings124.498/207.737/160.513ms first and124.041/198.478/162.507ms repeat;
Gram+batch-linkage totals1.794/2.784/.135s and1.699/2.566/.138s. Context
counts are pooled once per meeting; the pool timer also includes original
block counts, not independent context-only cost. Labels/persistence/production
assignment are excluded, so these do not pass an ingestion-overhead gate.
Target setup loads/validates large raw-vector and source ledgers; raw vectors
are validation inputs, not candidate features. Main timer excludes imports,
final JSON/repeat comparison; external monitor includes them.

Rusage peak2,203,128/2,202,292KiB; external RSS/PSS maxima
2,204,148/2,192,493 and2,203,500/2,195,423KiB; approximate status HWM
2,204,144/2,203,500KiB. Samples861/906, vanished1/1;
median/max gaps36.661/90.380 and34.797/79.963ms. One target excludes observer/
children; intrusive sampled lower bounds, approximate kernel status and
possible retained launcher rusage prohibit production-worker attribution or
peak subtraction. Scientific runs sequential, uncontrolled host; no speed-win
claim. These batch controls are not deployed code.

Source-supported labels/multitag memberships, independent coding/semantic
ancestor/guided-search checks, shared global generations, bounded online
updates, edits/deletes/restart, complete-ingestion<=10% overhead and selected
Python/Rust executable reference/differential CI remain open. No production
method is selected from these meeting proxies.
