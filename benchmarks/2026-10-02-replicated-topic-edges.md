# Distinct-parent replication and capped graph weights: negative control

Bead `mash-c0d.19.2.35`. The local-window audit found that most admitted word
edges occurred inside only one source question. This prospective comparator
requires replicated local-pair support, then separately tests whether capping
each question's graph vote improves discovery. **All 432 settings fail the
unchanged proxy gate, including individual seeds.** This is not a selected
production policy.

## Fixed procedures

Consume pinned `windows-first.json` and its exact source/code pins. Reconstruct
the original 64 exposed questions, 629-word source-only unigram vocabulary,
raw/equal-title-body counts and 2,016 natural pairs/97 positives. Reserved
references and outcomes remain unused; the original vocabulary is transductive.

For each 16/32/64-token window policy, collect all local word pairs separately
per parent, then deduplicate within that parent. Repeated windows and different
source records from one question add no extra parent votes. Words in separate
windows do not fabricate a co-occurrence. Admit only edges already selected by
the previous p05/BH05 policy **and supported inside local windows by >=2 different
questions**. Cap pair expansion at two million events, failing rather than
truncating. Actual expansions are 8,685/14,026/20,313.

Two weight controls have the same admitted topology:

- `window-count`: preserve original window co-occurrence edge weight; isolate
  the replication requirement.
- `parent-count`: weight by distinct supporting question count; additionally
  cap each question at one vote.

Underlying probabilities, window DF and multiple-testing family remain
unchanged. This does **not** recalculate an independent-parent null or demonstrate
FDR control. The graph's reweighted counts are Infomap weights only.

Keep three seeds, ten trials, one thread, hard mass/50-step sparse mixture,
coverage scaling, relative .5 threshold, absolute .10/.20/.30 and budgets3/8.
Three windows x two filters x two weights x three seeds x two membership methods
x three thresholds x two budgets =432 configurations. Require all three seeds
to have precision >=.70 and >=20 links, then maximize worst recall, precision and
deterministic key. Preserve unsupported evidence and no-fallback selection.

## Results and coverage trade-off

| Window | Raw edges / covered words | BH edges / covered words | Raw topics: window weights (17/29/101) | Raw topics: parent weights |
| --- | --- | --- | --- | --- |
| 16 | 366 / 257 | 50 / 74 | 41 / 39 / 40 | 41 / 42 / 41 |
| 32 | 665 / 351 | 50 / 80 | 40 / 42 / 43 | 47 / 48 / 45 |
| 64 | 1,066 / 447 | 51 / 77 | 42 / 45 / 45 | 43 / 41 / 40 |

BH produces 30/32/30 topics for windows16/32/64 under either weighting, identical
across seeds. Restricting support reduces coverage; stable tiny communities
are not necessarily useful subjects. Requiring recurrence can exclude genuinely
useful rare or newly emerging topics as well as incidental code patterns.

Descriptive maxima among settings predicting >=20 links:

| Weight / membership | Configuration: window/filter/threshold/budget/seed | TP / FP | Precision / recall | Unassigned / memberships |
| --- | --- | --- | --- | --- |
| Window / hard | 32/p05/.30/3/101 | 5 / 21 | .192 / .052 | 41 / 24 |
| Window / mixture | 32/p05/.30/3/101 | 7 / 19 | .269 / .072 | 4 / 62 |
| Parent / hard | 32/p05/.30/3/101 | 5 / 16 | .238 / .052 | 42 / 23 |
| Parent / mixture | 16/p05/.20/3/17 | 5 / 23 | .179 / .052 | 4 / 63 |

These exposed maxima are not selected winners, held-out improvements or
semantic precision. The selector is null. Capping graph votes does not make this
representation adequate; mixtures often fill coverage without improving precision.
Do not apply this strict support filter to production based on repeatability.

## Validation and cost limits

Two fresh processes repeat all non-timing fields. Independent parent-set
reconstruction verifies all **592,518** support entries, expansion counts and
all 12 graph masks/weights. Independent membership/pair audits verify **870,912**
decisions, confusion counts, abstentions and budgets. All 36 model partitions,
covered-word sets, probability-mass/coverage invariants and monotonic likelihoods
are checked. Six raw models (all windows/weights, seed17) are independently
replayed using dense EM: every likelihood, count conservation and final scores/
word distributions agree. This is not independent full replay of all 36 fits.
All **282** lab tests pass with the optional graph extra; two integration tests
skip explicitly without it.

Total after imports is 5.738/5.831s for all 432 settings. The first run's support
construction is .044/.258/.049s for windows16/32/64. It reuses frozen window
probability tests, so comparison with previous end-to-end experiment times is
not an ingestion speed-up. No additional inference occurs; earlier bounded-unit
inference remains charged. CPU/RSS, p95, edit/delete/restart and the <=10% complete
ingestion gate remain unverified. No online semantic hierarchy or Rust parity
is demonstrated.

Outside-Git artifacts under `topic-public-stackoverflow-20261002`:

- `replicated-first.json`: `b999fdbccd1577b1282acdab9c483e81de8f70828c827aa8c0326db6040b9c70`
- `replicated-second.json`: `f974988e8ac84a284e555dae136438f107e5ce574883857a428063d03f0dac9c`
- `verify-replicated.py`: independent support/weights/pair/model/dense-EM audit.

Repeated negative results motivate revisiting source representation and the
evaluation evidence before expanding graph parameter grids. Preserve code and
prose separately in a prospective representation control, rather than silently
deleting implementation detail or hand-tuning stopwords. Independent agent-chat
primary/secondary/ancestor judgments and guided-search evaluation remain required;
community-tag overlap cannot establish those properties.
