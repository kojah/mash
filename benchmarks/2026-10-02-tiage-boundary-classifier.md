# Bounded lexical/dense boundary classifier

Bead: `mash-c0d.19.2.20.3`. Research only; no production method selected.

TextTiling motivates lexical cohesion ([Hearst, 1997](https://aclanthology.org/J97-1003/));
utterance-pair coherence provides a related dialogue-segmentation approach
([Xing and Carenini, 2021](https://aclanthology.org/2021.sigdial-1.18/)).
This small supervised control implements neither paper's algorithm. It asks
whether generic lexical overlap complements the frozen cosine boundary scores
without further model inference. It is not an unsupervised topic-tree method.

## Frozen procedure

Before outcomes, record the protocol in Beads. Reuse complete original TIAGE
train/development sources and independently audited singleton int8-derived
normalized embeddings from the [previous study](2026-10-02-tiage-boundary-calibration.md).
Verify both prior result pins, array hashes, original source identity, every
boundary row, and recomputed source geometry. All 4,392 training and 1,446
development boundaries remain supported. No test data, new encoder calls,
model downloads, generative inference or paid requests.

Six features, in fixed order:

1. Current/previous turn cosine.
2. Current/normalized mean of at most two previous turn directions cosine.
3. Current/previous token-set Jaccard.
4. Current/union of at most two previous token sets Jaccard.
5. Log1p current token count.
6. Log1p previous token count.

Lexical tokens use lowercase Unicode `(?u)\b\w\w+\b`; empty unions score zero.
Counts include repetitions. No phrase rules, stoplists, learned vocabulary,
future turns, labels, IDs, positions, provider or role features. Dialogue IDs
only define folds. The public data do not supply agent/user roles.

Fit `StandardScaler` and logistic regression (`C=1`, `liblinear`, `max_iter=1000`,
seed 20261002) separately inside each of five deterministic `GroupKFold` training
folds. Select a cutoff from all training out-of-fold probabilities using exact
rational micro-F1, then fewer predicted changes, then smaller signed cutoff.
The existing strict-less-than calibrator consumes negative probabilities:
the deployed prediction rule would be **probability > 0.22878543025787945**.
Evaluate all 4,393 distinct cutoff candidates, including no/all-change endpoints.
Refit on all training rows and apply the fixed cutoff to development.
Scalers never use held-fold or development rows. Convergence warnings fail.

Development was already exposed by the earlier cosine study: this comparison is
exploratory, not a fresh held-out selection result. There are no exact complete
dialogue duplicates across splits; 19 unique turn texts / 30 development turn
occurrences are shared and retained. Model pretraining overlap is unknown.

## Results

| Evaluation | TP / FP / FN / TN | Precision | Recall | F1 | AP | AUC |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Training OOF, selected cutoff | 546 / 1056 / 341 / 2449 | .3408 | .6156 | .4387 | .3833 | .7191 |
| Development classifier | 196 / 364 / 115 / 771 | .3500 | .6302 | .4501 | .3756 | .6993 |
| Development frozen adjacent cosine | 179 / 345 / 132 / 790 | .3416 | .5756 | .4287 | .3549 | .6873 |
| Development all-change | 311 / 1135 / 0 / 0 | .2151 | 1 | .3540 | — | — |

Development F1 gains .0213, with 17 more true positives and 19 more false
positives. Precision remains only .35. The final standardized coefficients are
`[-.4929734, -.4881712, -.1716913, .3409670, .1426741, .0133414]`, intercept
`-1.598254`, five solver iterations. Correlated features make these coefficients
unsuitable as causal explanations. No ablation isolates the lexical contribution.

One thousand paired dialogue resamples (seed 20261002) give F1-gain
2.5/50/97.5 percentiles `[-.00196, .02105, .04592]`. This descriptive range
includes zero and omits cutoff selection, model/pretraining and development
exposure uncertainty. It is not a calibrated significance claim. Better boundary
metrics alone do not prove coherent coding-session episodes, topic labels,
semantic ancestor inheritance or guided-search gains.

## Reproducibility and audit

Python 3.11.15 / scikit-learn 1.9.1, single-thread environment. Fresh runs match
every nontiming field, all five array hashes, fold allocations, scaler/model
states, cutoff candidates and predictions exactly. Five targeted tests cover
causal context, Unicode/repeated tokens, empty-token cohesion, malformed input,
fold-local scaling, constant features and missing classes. Full enabled lab:
**421 tests pass**, no skips with the author reference and graph extras.

Artifacts under `/home/james/scratch/mash-artifacts`:

- `topic-tiage-classifier-first-20261002/results.json` SHA256
  `ad576d5be58021915fa1c8270f03bd15e93e0a9230b59d7185728556ef63cfdd`.
- `topic-tiage-classifier-second-20261002/results.json` SHA256
  `497d7f64efee187a9e10c791ed645d2ca520083e46ee66def1ce130e569fe728`.
- `topic-tiage-classifier-audit-20261002.py` SHA256
  `bc99a88981cafbab73853fcd7c6167bbd1f6a4f8b560e5ae7ccf29355efb7d1b`;
  output `topic-tiage-classifier-audit-first-20261002.json`.
- `topic-tiage-classifier-memory-{first,second}-20261002.json`.

The separate auditor directly reads original JSON, verifies all 5,838 boundary
identities, reconstructs lexical features and cosine component reductions,
checks all five fold memberships and scaler means/variances, refits six models,
checks scalar sigmoid probabilities, brute-counts all 4,393 cutoff candidates,
and recomputes per-dialogue confusion/AP/AUC and paired resampling. Maximum
feature/probability/ranking differences: `1.55e-15 / 2.22e-16 / 3.33e-16`.
This is independent source/formula verification using the same sklearn solver,
not independent inference, solver implementation or Rust parity.

Reproduce with `research/topics/boundary_classifier.py`, explicit
`--calibration`, `--calibration-repeat`, `--development`, `--development-repeat`,
`--training-source`, `--development-source`, fresh outside-Git
`--output-directory`, and repeat `--compare FIRST/results.json`. Sources and
frozen assets stay outside Git. Do not reinterpret existing development
results as test data or tune features/cutoffs against them without recording
the resulting exploratory status.

## Cost and decision

After-import total wall first/repeat .4085/.3882 s, process CPU .4035/.3801 s;
preflight .3300/.3082 s, all five fold fits plus calibration and final fit/predict
.0466/.0455 s. Remaining work includes saving arrays and evaluation/bootstrap;
final JSON serialization is outside that timer. No inference cost is hidden:
these runs reuse already-created embeddings, whose earlier cost remains real.

Rusage peaks 182108/181516 KiB; external sampled target RSS 182224/181916 KiB,
PSS 170657/172024 KiB, status VmHWM 182168/181836 KiB. There were 66/71 samples,
1/0 vanished reads, median/max observation gaps 24.16/30.01 and 24.23/36.86 ms.
Target includes Python imports, captured arrays, feature matrices and models;
children and observer are excluded. Sampled peaks are lower bounds, kernel
status counters approximate, monitoring perturbs timing, rusage may preserve
launcher high-water marks; no subtraction. These are not production worker
memory, full-ingestion throughput or the provisional <=10% ingestion gate.

**Do not promote.** This is a cheap modest baseline improvement with substantial
false boundaries and uncertain paired gain. Next assess generalization on
independent dialogue/coding-session boundaries and distinguish useful atomic
topic evidence from merely detecting a response shift. Topic/label quality,
complete ingestion, Python/Rust parity and MCP exploration remain open.
