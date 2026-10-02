# Human-boundary calibration: simple cosine remains too imprecise

Bead `mash-c0d.19.2.20.2`. Training-only cutoff selection transfers to author
development data, but precision is about one third. These controls establish
a reproducible boundary baseline; they do not select production episode extraction
or a topic method.

## Prospective setup and separation

Use the [verified TIAGE training preflight](2026-10-02-tiage-boundary-preflight.md),
all 300 original training dialogues and 4,392 nonroot boundaries. Response labels
refer to the preceding/current transition; roots remain excluded sentinels.
TIAGE supplies independent open-domain human annotations, with annotator
lookahead; coding-topic/ancestor quality needs separate evidence.
[Author guidelines](https://arxiv.org/html/2109.04562v1).

Freeze rules in Beads before inference: complete original singleton MiniLM turn
directions; adjacent cosine and normalized mean of the preceding at most two
directions versus the incoming response. No future context, annotation features,
rewriting, truncation or new model downloads. Context mean norm ≤1e-12 abstains,
and calibration compares matched support. Before model initialization, verify
installed model/tokenizer hashes, at most 5,000 unique source texts, 2–256 tokens
and at most 64 KiB per text; unsupported sources fail the complete experiment.

Use existing `Encoder` attention-unpadded float32 mean pooling followed by
round-away-from-zero int8 scaling and float64 normalization. This research
direction stage differs from the unquantized complete-question experiment and
is not cross-platform Rust inference parity. Preserve every original turn;
deduplicate only exact inference input strings, in first-occurrence source order.
Clear the existing 1,024-entry inference cache at capacity while retaining the
complete bounded vector lookup. One model initialization per fresh process;
no mixed padded batches or paid/generative calls.

For `cosine < cutoff`, enumerate every distinct supported training score and
`nextafter(max,+inf)`, including none/all-change endpoints. Preserve complete
candidate confusion counts. Maximize exact rational micro-F1, then prefer fewer
predicted changes, lexical method ID and smaller cutoff. Seal both per-method
cutoffs and the global winner **before** downloading development JSON. The
separate evaluation module verifies exact first/repeat calibration hashes,
arrays and code pins before permitting that download; it never recalibrates.
Test and weak-supervision data remain undownloaded.

Installed Xenova all-MiniLM-L6-v2 model SHA256
`afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1`,
tokenizer `da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0`.
Python 3.11.15, NumPy 2.4.6, sklearn 1.9.1, ONNX Runtime 1.30.0,
tokenizers .23.2, one CPU intra/inter-op thread; same Xenia environment as
earlier semantic controls. Inference/preprocessing/assets remain pinned.

## Outcomes

| Fixed rule | Training cutoff | Train F1 | Dev precision | Dev recall | Dev F1 | Dev AP | Dev AUC |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Adjacent, frozen global winner | .2196514282107352 | .4354 | .3416 | .5756 | .4287 | .3549 | .6873 |
| Previous-two mean, diagnostic | .2896496928416871 | .4290 | .3123 | .6367 | .4190 | .3484 | .6713 |
| All change | — | .3360 | .2151 | 1.0000 | .3540 | — | — |
| No change | — | .0000 | undefined | .0000 | .0000 | — | — |

Training selected adjacent: TP 511, FP 949, FN 376, TN 2,556. Previous-two gives
583/1,248/304/2,257. All original 4,392 boundaries are supported; zero cancellation
abstentions. 4,692 turns require 4,613 unique model calls, retaining 79 exact-text
reuses; maximum 27 input tokens, four cache clears, zero internal cache hits.
There are 4,386/4,393 candidate cutoffs, including endpoints. Training AP/AUC
is .3661/.7088 adjacent and .3606/.6985 previous-two. No cutoff is chosen using
development scores or labels.

Development contains all 100 original dialogues, 1,546 turns and 1,446 boundaries
(311 change, 1,135 continuation). All are supported, no abstentions; 1,532 unique
model calls, maximum 26 tokens, one cache clear. Frozen adjacent produces
179/345/132/790, previous-two 198/436/113/699. There are no exact training-dialogue
duplicates; 19 unique turn strings and 30 turn occurrences overlap training.
Prespecified dialogue-disjoint and duplicate-inclusive slices therefore coincide;
shared turn strings remain included, not silently removed. Pretraining overlap
is unknown and no zero-shot claim follows.

Across 1,000 fixed seed-20261002 dialogue-clustered draws, adjacent development
F1 2.5/50/97.5 percentiles are .3882/.4284/.4702; previous-two .3823/.4195/.4556.
Paired F1-minus-all-change ranges are [.0393,.1106] and [.0327,.0959]. These are
descriptive resampling ranges, not calibrated significance or coding-topic
generalization. Training-selected intervals also retain selection optimism.
Higher F1 than the trivial all-change rule does not make roughly two false cuts
per correct cut acceptable for reliable episode extraction. Adding context by
averaging does not improve this control. Do not repair cutoffs on exposed
development outcomes or conclude every possible context model fails.

## Verification and measured costs

Fresh full-inference repeats match all decoded nontiming fields and three
array hashes for each phase. Frozen calibration verification gates development
capture. Ten new tests cover prefix-only scoring, cancellation, invalid inputs,
generated tied-score threshold cases, exact endpoints/near-threshold strict
ordering, conservative/lexical tie rules, sealed-before-network behavior and
paired resampling. The full enabled research suite passes 416 tests.

Independent auditors reconstruct all 4,692/1,546 source turn alignments,
4,392/1,446 boundaries, exact-text reuse, original training/development overlap,
frozen-vector cosine geometry, 8,779 brute-force training candidates and 2,892
fixed development predictions. They check confusion counts, frozen winners,
AP/AUC via independent pair/tie calculations and complete bootstrap arrays.
Maximum geometry differences are 1.55e-15/1.33e-15, ranking differences
1.11e-16/1.67e-16. This is source/wrapper/frozen-vector arithmetic evidence,
not independent model inference or Rust parity.

| Scoped after-import cost, seconds | Train first | Train repeat | Dev first | Dev repeat |
| --- | ---: | ---: | ---: | ---: |
| Full experiment, excludes final JSON | 6.854 | 35.849 | 12.624 | 5.710 |
| Process CPU | 6.764 | 13.740 | 4.735 | 3.635 |
| Complete encoding loop | 6.372 | 34.506 | 10.717 | 4.944 |
| ONNX calls only | 5.624 | 28.438 | 8.701 | 4.142 |
| Preflight | .227 | .522 | .907 | .428 |
| Model initialization | .092 | .196 | .780 | .220 |

Training calibration plus bootstrap takes .069/.216 s. These timings vary widely
on the shared host and include monitoring interference; no controlled speed
comparison, cold-start guarantee or production latency follows. No model weights
are trained. Costs do not measure complete ingestion, persistence or ≤10% overhead.

| Memory KiB | Train first | Train repeat | Dev first | Dev repeat |
| --- | ---: | ---: | ---: | ---: |
| Rusage peak | 263,604 | 264,468 | 230,548 | 227,468 |
| External smaps RSS maximum | 264,108 | 271,136 | 231,636 | 228,900 |
| External PSS maximum | 256,596 | 263,102 | 223,296 | 221,160 |
| Samples / vanished reads | 318 / 1 | 873 / 6 | 425 / 2 | 242 / 1 |

Requested sleep is 20 ms; median/max observed gaps are 24.94/48.08,
42.16/227.82, 38.56/125.35 and 30.07/63.96 ms respectively. All four monitored
processes exit zero. Sampled RSS/PSS maxima are lower bounds; status counters
approximate, with VmHWM 264,108/270,880/231,632/228,900 KiB. Target-only scope
includes imports, assets, cache/vector/score state and final JSON, excludes
children/observer, and is not deployed worker memory. No peak subtraction occurs.

## Reproduction pins and decision

Run `tiage_calibration.py` with `--source`, pinned `--preflight`, explicit installed
`--snapshot` and fresh `--output-directory`; repeat adds `--compare FIRST_RESULTS`.
Wrap each process with `memory_audit.py` and one-thread environment settings.
`tiage_development.py --calibration FIRST --calibration-repeat REPEAT
--capture-development FRESH_SOURCE` gates the one annotated development download.
Its separate evaluation invocation adds `--training-source`, `--development-source`,
`--snapshot`, fresh `--output-directory` and optional repeat `--compare`.

Captures stay outside Git:

- Training first/repeat result hashes:
  `652173df490d2a93882ad81165dfb9affd6642acff898da839d9004da4e53ea0`,
  `2f7aca3a331b46dae8f023cc5700139fcf86e72703435950fcfe711ba3a1f0bb`.
- Development first/repeat result hashes:
  `dd5af8f64223b2d717d17ddc11c75bb9c5b523743feb00268593d82bff56e9ea`,
  `c522dd40157acbd5eec3d4fe570c0b659e9b0d14c2cac365dc3fb22b8b9f93ab`.
- Development source SHA256
  `721032dcfbf546e8607c8d9d59089db731aafb1fe6569473c64cea0c030e1a3e`,
  author blob `c1f3c9374ed2a27979b13126b46998526543a302`, 96,155 bytes.
- `topic-tiage-calibration-audit-20261002.py` SHA256
  `d62fe4f46275fb050e2745192b99975b56bbe36dd1d5b41a63f03b0eff423d84`
  and its verified `topic-tiage-calibration-audit-first-20261002.json`.
- `topic-tiage-development-audit-20261002.py` SHA256
  `4b5a310c4d9877cce451bde03ad3ee0ac282693357fd973ff872d1d7ad1f7326`
  and its verified `topic-tiage-development-audit-first-20261002.json`.
- Result directories `topic-tiage-calibration-{first,second}-20261002` and
  `topic-tiage-development-{first,second}-20261002`; monitors use the same
  names with `-memory-` before the phase suffix. Author sources are separate
  `topic-tiage-author-training-20261002` and `topic-tiage-author-development-20261002`.

Keep this weak but reproducible baseline. Further episode work needs a justified
representation/coherence model and frozen training-selection protocol, not an
exposed development cutoff sweep. Test data remain reserved. Coding-provider
units, labels, overlapping topic assignments, meaningful ancestry, lifecycle,
guided search, complete ingestion and Rust parity remain unproven. Production
search and topic behavior are unchanged.
