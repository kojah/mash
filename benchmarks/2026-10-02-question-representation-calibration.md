# Representation-specific programming-topic calibration: no viable setting

None of the 108 preregistered view/linkage/cut candidates meets the unchanged
calibration gate. This is a useful negative result: transferred cutoffs are not
the only failure. On this grid, calibrating to the programming representations
also fails to produce enough high-precision shared-tag relationships. Do not
relax the gate, tune on previously exposed reserved results, or select a Rust
method from these outcomes.

## Prospective procedure

Before outcomes, record the policy in `mash-c0d.19.2.25`. Use only the original
64 exposed `calibration-source` questions and their natural 2,016 pairs, with
97 shared-tag positives and 1,919 negatives. Original reserved-source references
and results are not used for fitting or selection. This is development
calibration, not a new held-out evaluation. Community tags are incomplete
technology/task proxies and never enter representation features.

Reuse exact source-pinned title, whole-question lexical and complete-unit captures.
No new inference, asset loading, model download or index mutation. Views:

- Title vectors, 64 × 384.
- Normalized mean of distinct unit directions, 64 × 384.
- Whole-question background-only TF-IDF, 64 × 1,725.
- Title/lexical and mean/lexical 75/25 fusion, each 64 × 2,109.
- Native unit union, 247 distinct unit points × 384, with question membership
  equal to the union of its units' direct native clusters.

Fusion concatenates `sqrt(.75) * dense` and `sqrt(.25) * lexical`, preserving
the previously defined weighted cosine rather than averaging coordinates from
different spaces. Refit the exact 72-question disjoint lexical basis to verify
the original vocabulary, IDF, sparse entries and source hashes; it is not a new
feature policy. Reconstruct unit means from saved directions exactly. Require
the original three capture hashes and unchanged source-code contracts.

Use the exact GRINCH-style prototype with average-cosine and normalized-centroid
linkage, cuts .1 through .9, forward/reverse/seed-17 shuffled arrivals. Six views,
two linkages and nine cuts yield 108 candidates and 324 partitions across 36
fitted trees. Each candidate must have precision ≥.70 and at least 20 predicted
positive question pairs **in every arrival order**. Among eligible settings,
maximize worst-order recall, then worst-order precision, then lexical candidate
key. Retain the analogous forward-only comparator. Both selectors explicitly
return no viable setting if the requirements are unmet; no fallback winner.

## Results

Every robust and forward-only winner is absent. The best forward precision
among grid candidates predicting at least 20 links, for each view/linkage, is:

| View | Average: precision / links / cut | Centroid: precision / links / cut |
| --- | ---: | ---: |
| Title | .174 / 46 / .2 | .208 / 24 / .3 |
| Unit mean | .238 / 21 / .3 | .167 / 42 / .3 |
| Lexical | .276 / 29 / .2 | .048 / 2,016 / .1 |
| Title 75 / lexical 25 | .200 / 30 / .2 | .130 / 200 / .2 |
| Mean 75 / lexical 25 | .200 / 20 / .3 | .182 / 66 / .3 |
| Unit union | .238 / 21 / .3 | .241 / 29 / .3 |

The lexical-average setting with the highest listed precision has only 8 true
links and 21 false links. Forward candidates reaching the precision target emit
only one or two links, below the minimum 20. Empty positive predictions have
undefined precision and cannot pass. Arrival robustness is not the sole reason
for rejection: no forward candidate passes either.

A disclosed post-hoc, reference-only feasibility check finds eight questions
sharing the most frequent tag. Grouping those eight and leaving the other 56
as singletons would yield 28 true links and no false links under this proxy.
Thus the precision/link-count target is not mathematically impossible for a
partition of this cohort. This is an oracle sanity check, not an algorithm,
feature, label seed or selected candidate; tags were never fed into clustering.

The result is limited to the declared grid, source cohort, encoder/lexical
features and tree prototype. It does not prove that no intermediate threshold,
representation, clustering method or meaningful semantic relation could work.
Shared tags are neither complete semantic equivalence nor independent
primary/secondary/ancestor annotations for coding sessions. The earlier naming
gains remain label salience gains, not useful grouping evidence.

## Verification, cost and next action

Two fresh processes match every non-timing field exactly. Runtime after imports
is 3.999/4.602 seconds for complete calibration/source checks, tree fitting and
cut evaluation. This is not ingestion throughput, memory, update latency or a
production gate. The prior unit representation's 407 extra calls and about
4.7 seconds of model inference remain charged even though this stage reuses them.

Independent reconstruction checks all 324 complete point partitions, direct
question membership unions, 653,184 pair predictions and confusion counts,
precision/recall/F1, eligibility, and absence of both forward and robust winners.
All 259 lab tests pass; new tests cover unsupported precision/too-few-links,
all-order eligibility, worst-order selection/ties and weighted-cosine fusion.
Tree validation runs for every fitted tree. This is our Python prototype, not
author-reference GRINCH equivalence or Rust differential CI.

Artifacts under `topic-public-stackoverflow-20261002`, outside Git:

- `question-calibration-first.json`: `1bab8d1c9755258fde74cf52ffa8391364a167e7a8982a17ccaf70b9904ae964`.
- `question-calibration-second.json`: `4d4919e9a6919aa9d1e02dda9351f497a257b2f51441079e76f77c49aa882f40`.
- `question-calibration-tests.log`, `question-calibration-independent-verification.log`
  and the retained independent verification script preserve checks.

Invoke `question_calibration.py --root <original-capture-directory> --output
<fresh-outside-Git-path>`, then repeat with `--compare <first-output>`.
The capture includes all candidate partitions/memberships/metrics, view input
hashes/shapes, tree statistics, source-code hashes and dependency versions.

No setting warrants fresh confirmatory inference/evaluation from this calibration
run, and none is promoted. Next research should compare substantially different
source-grounded multi-topic representations/discovery against these controls,
with independent semantic judgments and genuinely new groups. Do not continue
lowering cutoffs on exposed references and call that progress toward a validated
method. Quality, lifecycle, guided-search, full-ingestion overhead and Rust parity
remain open in `.22`, `.25`, `.26`, `.32` and `.33`. The distinct source-supported
overlapping lexical-discovery hypothesis is tracked as untested in `.35`;
research primary sources and freeze its experiment before any implementation.
