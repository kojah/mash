# Output-preserving lexical topic-label ranking

Profiling the [fixed topic-label control](2026-10-02-question-unit-topics.md)
identified full-vocabulary sorting as the main label-work bottleneck. Filtering
zero scores and selecting only the requested top-k terms preserves every frozen
label and score while reducing the matched count/scoring/ranking stage by about
7.5 times. This changes Python research label maintenance, not production search
latency, semantic quality or the full ingestion pipeline.

## Profile before changing code

Run the original `question_label_balance.py` with `cProfile` at commit
`d06475dce0dc93596541f0b21f0a313646030967`.
The profile repeats the original balance capture's non-timing outcomes exactly.
Total profiled execution is 21.652 seconds, including imports; experiment
execution is 18.007 seconds. Its 4,008 `ranked_terms` calls consume 15.405 seconds
cumulatively and evaluate 32,833,536 sort keys. Profiling adds overhead; do not
compare these times directly against an unprofiled optimized run.

## Change and frozen semantics

`ranked_terms` keeps the same validation and score/lexical tie rules. Select
positive-score indices first, then use `heapq.nsmallest` with `(-score, term)`
to obtain the requested ordinary top-k result. Zero scores never produced labels
before. Avoid constructing containment sets on the ordinary path. Diversity
pruning still sorts all positive candidates: arbitrary numbers of contained
terms can be rejected, so truncating candidates to k would change that behavior.
No score formula, vocabulary, source boundary, background DF, count weight,
membership or label budget changes.

`label_ranking_benchmark.py` retrieves only the original `ranked_terms` function
AST from the pinned Git commit, verifying its full source hash against the frozen
capture. It creates no compatibility copy and never executes corpus instructions.
Require the original topic/balance/unit capture hashes and validate every unrelated
code pin. Rebuild original source features, counts and count hashes; compare all
topic and parent labels, including floating scores, to the original capture.
The older experiment runners keep their strict code-drift guards. Use their
original commit to reproduce their original contract; use the new audit for this
explicit implementation comparison rather than relaxing those guards.

The 40 workloads cover all 24 raw-count topic partitions plus 16 fractional
title/body controls, with 10,024 rank calls per method pass. Use one process and
two trials in baseline/optimized/optimized/baseline order, keeping inputs and
count reconstruction identical. Time count reconstruction/hash checks, scoring,
term selection and per-parent maximum-score aggregation. Aggregate wall/CPU
times also include exact output comparison and output hashing; per-workload
timers stop before those two checks. Imports, source-feature construction,
source-support review, final JSON writing, inference and full ingestion are
outside that timed scope.

## Matched unprofiled results

| Trial | Committed full sort wall / CPU | Positive top-k wall / CPU | Wall speedup |
| --- | ---: | ---: | ---: |
| 0 | 26.798 / 25.400 s | 3.599 / 3.571 s | 7.45× |
| 1 | 25.172 / 23.749 s | 3.309 / 3.304 s | 7.61× |

Every term, score and parent budget matches the original captured outputs.
These are two descriptive matched trials, not confidence intervals or a
production service benchmark. Both policies run in the same interpreter and
share scientific imports and source features. The optimization does not fix the
fragmented topic partitions or establish useful semantic labels.

## Repeat, memory and tests

A second fresh process runs the same two trials under the existing 20 ms
Linux `memory_audit.py` observer. Compare every non-timing field against the
first capture; all non-timing fields match exactly. There are 2,103 samples,
with maximum observed RSS 255,836 KiB and PSS 247,910 KiB. This audit covers one target process running both policies,
including imports and feature building; it excludes observer/children. Its
sampled RSS/PSS maxima are lower bounds and monitoring can perturb times. It
does not provide separate attributable memory peaks for the two policies or a
production ingestion memory gate. Retain a separate same-image rusage audit;
do not substitute inherited launcher rusage for sampled target memory.

A further successful same-Python-image audit also repeats every non-timing
field. Retain `RUSAGE_SELF` maxrss 254,556 KiB and 1,892 external samples with
maximum observed RSS 255,288 KiB/PSS 247,433 KiB. Approximate/image/continuous
counters and sampled mapping observations are distinct; do not subtract or
attribute their peaks to a policy. The first rusage wrapper omitted the lab
module path and failed before benchmarking; its log/audit remain outside Git
and are excluded from performance results. The corrected wrapper supplies that
path explicitly. `label-ranking-rusage-final.json`, its log and
`label-ranking-rusage-memory-final.json` retain the successful result.

Independent AST comparison verifies that the scoring function and ranker input
validation are unchanged from the committed baseline.

All 255 lab tests pass. Fixed-seed cases compare bounded selection against a full
sort on dense/sparse/zero/tied scores, empty vocabularies and budgets above the
candidate count. Diversity tests require scanning beyond the initial top-k and
preserve vocabulary-permutation invariance; duplicate vocabularies still reject.
Public fixture checks are Python parity for this optimization, not Rust/Python CI.

Artifacts under `topic-public-stackoverflow-20261002`, outside Git:

- `label-baseline.prof` and `label-profile-baseline.json` retain the original profile.
- `label-ranking-first.json`: `fd8df2dd5c74c8b9e4864c391ee3e7af6200893c1e4c743e56814a262ebeb8d0`.
- `label-ranking-second.json`: `bd92f69336b11008096d91b2853027d2f32397cca3c8e3fecb8bc968e61555d4`.
- `label-ranking-memory.json`: `47f3a466f580bd1c2530228aa1acd293e2c794a148473acbf6f202c4c928454a`.
- `label-ranking-tests.log` retains the 255-test result.

Run `label_ranking_benchmark.py --root <original-capture-directory> --output
<fresh-outside-Git-path>`; repeat with `--compare <first-output>`. The baseline
commit must exist in local Git history. No model calls, downloads or index
mutations occur. Tracked in `mash-c0d.19.2.34`; full initial/incremental ingestion,
edits/deletes, persistence and the provisional ≤10% throughput-loss target remain
separate requirements in `.22`. Independent topic quality and Rust parity remain
open in `.25`, `.26` and `.33`.
