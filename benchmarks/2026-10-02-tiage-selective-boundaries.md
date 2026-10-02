# Selective boundaries: no supported high-precision operating point

Bead `mash-c0d.19.2.20.4`. Research only; production topics remain unselected.

The [six-feature classifier](2026-10-02-tiage-boundary-classifier.md) has modest
F1 gains but low precision. Selective classification studies risk/coverage
trade-offs ([Geifman and El-Yaniv, 2017](https://arxiv.org/abs/1705.08500)).
Our positive-boundary acceptance diagnostic is simpler: it does not implement
their statistical risk guarantee or accept confident no-change classifications.

## Frozen procedure

Before outcomes, freeze one target: observed training OOF precision >=.80 and
at least 20 accepted training boundaries. Select maximum accepted count, then
exact rational precision, then larger cutoff. Use **strict probability > cutoff**
and preserve complete equal-score blocks and no/all-change endpoints. If no
candidate qualifies, emit `unsupported`, null winner, and no accepted development
boundary. Never fall back to the F1 cutoff. Unselected means **unresolved**, not
evidence of no topic change. Confusion counts include missed gold boundaries to
measure recall, not to assert that unselected cases are negative classifications.

Reuse verified original sources and frozen classifier first/repeat probabilities;
check original boundary identities, every original array and classifier code pin.
All 4,392 training and 1,446 development boundaries remain. No new fitting,
inference, downloads, paid/generative calls or test access. Development was
previously exposed; any supported evaluation would be exploratory. Do not tune
the target or cutoff using development outcomes.

Reliability uses ten fixed bins `[0,.1), ... [.9,1]`: `floor(10*p)` capped at 9.
Record counts, changes, mean probability and observed frequency, with null
statistics for empty bins. Brier is mean squared probability error; log loss
clips to float64 epsilon and `1-epsilon`; ECE is the count-weighted absolute
bin gap. These diagnostics neither retrain nor calibrate probabilities.

## Results and implication

**None of the 4,393 training cutoff candidates passes the gate.** Even the
largest observed precision among candidates accepting >=20 is only .60:
15 true / 10 false boundaries among 25 accepted, recall .01691. That maximizer
is a retrospective training diagnostic, not a substitute policy, and is not
applied to development. The unsupported policy accepts 0 boundaries and leaves
all 4,392/1,446 train/development boundaries unresolved.

| Probability diagnostic | Training OOF | Development |
| --- | ---: | ---: |
| Brier | .145088 | .154947 |
| Log loss | .453145 | .479500 |
| Fixed-bin ECE | .006070 | .014925 |

Small average calibration gaps do not establish discrimination or high-precision
coverage. Development has 9/2 examples in [.6,.7)/[.7,.8), and none >=.8.
Observed change rates there are .444/.5 versus mean predicted .630/.759; these
tiny bins do not support stable high-confidence conclusions. The bulk of the
corpus has low predicted change probabilities. Training OOF versus final-model
prediction shift, correlated dialogue boundaries, cutoff selection and unknown
pretraining overlap further prevent an observed .80 target from being a
population guarantee. No confidence intervals or risk guarantees are claimed.

This rejects the present classifier as a reliable automatic topic splitter at
the frozen gate. It does not prove that whole-dialogue pooling is adequate,
that there are no real topic changes, or that all possible coherence methods
fail. Do not lower the target or substitute tiny perfect-precision prefixes.
Further work needs a more informative representation and independent technical
session evidence; label quality, overlap, semantic ancestors, guided search,
complete ingestion and Python/Rust parity remain open.

## Reproducibility and audit

Python 3.11.15, single-thread environment. Fresh repeat matches every nontiming
field, all cutoff/count/risk/coverage records, bin diagnostics and null winner
exactly. Five tests cover unsupported fallback, explicit unresolved semantics,
exact .80 eligibility, minimum support, tied-score coverage, strict boundaries,
bin endpoints/empty bins and malformed inputs. Full enabled lab: **426 tests**
pass, no skips with author reference and graph extras.

An independent auditor, importing no lab functions, reads original JSON and
checks all 5,838 boundary identities, parent probability-array hashes, every
candidate with brute strict comparisons and exact Fraction eligibility/ties,
accepted IDs, per-dialogue counts, bin statistics, scalar Brier/log loss/ECE.
Maximum reliability drift is `1.11e-16`; zero eligible candidates verified.
This checks source/formula fidelity, not independent inference or Rust parity.

Artifacts under `/home/james/scratch/mash-artifacts`:

- `topic-tiage-selective-first-20261002/results.json` SHA256
  `e2cff92839a7b6ed00a9da9bd05d0c8cfb2e0d8e96f58a342592b5f6138dcf02`.
- `topic-tiage-selective-second-20261002/results.json` SHA256
  `25436c0529e329a78b6c7461aeb8b6aa5b4ea8c25d728ba38af8ba12fa05e61a`.
- `topic-tiage-selective-audit-20261002.py` SHA256
  `14cabac12f9117ede60c17013d8b48fe8de088a928dad7951bd9d88a402fb682`;
  output `topic-tiage-selective-audit-first-20261002.json`.
- `topic-tiage-selective-memory-{first,second}-20261002.json`.

Reproduce `research/topics/tiage_selective.py` with `--classifier`,
`--classifier-repeat`, `--training-source`, `--development-source`, fresh
outside-Git `--output-directory`; repeat `--compare FIRST/results.json`.
No source text, credentials or captured probabilities enter Git.

## Scoped cost

After-import first/repeat wall .2020/.1706 s, process CPU .1996/.1642 s;
preflight .1483/.1336 s, training policy calibration .0347/.0241 s. Total includes
source verification, policy and reliability/count evaluation, not final JSON
serialization. The script deliberately imports no classifier training module
and performs no fit or encoder call; imported verification dependencies still
contribute memory/startup. This is not the earlier embedding/fitting cost.

Rusage peaks 164460/164052 KiB. Externally sampled target RSS 164472/163320 KiB,
PSS 156814/155683 KiB, status VmHWM 164472/164052 KiB; 62/60 samples, 1/0
vanished reads, median/max gaps 23.81/29.89 and 23.64/27.14 ms. Target includes
imports, frozen classifier JSON/arrays, sources and policy records; excludes
children and observer. Sampled peaks are lower bounds, kernel status approximate,
monitoring perturbs runtime, rusage may preserve launcher peaks; no subtraction.
Not production worker memory, ingestion throughput or the <=10% ingestion gate.
