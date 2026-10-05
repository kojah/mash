# Additional message clustering algorithms

This is a separately bounded follow-up to the rejected first study, authorized
2026-10-05. Offline methods are eligible. Production remains unchanged.

## Fixed protocol before outputs

Following the user's larger-cohort request, freeze 72 fresh threads, two-thirds
development and one-third reserved within each stratum. After excluding prior
components, Claude has only six medium and six long independent candidates.
Use Claude short/medium/long counts 18/6/6 and Codex counts 18/12/12. Report
stratum metrics separately; do not describe this availability-limited allocation
as balanced or representative of provider market share.
Exclude every detected component containing a previous-sample thread. Use seed
`mash-message-clustering-alternatives-20261005-v1`, prioritize >1,000-vector
threads within long strata, then SHA-256(seed + identity) order. Positions 0,3,6,...
are reserved. Screen fifteen configurations on 48 development
threads. Freeze sparse agent-authored pair judgments before viewing fit outputs;
label provenance and ambiguity remain explicit. Reserved threads receive no
judgments or scoring until a candidate qualifies for a final evaluation.
Keep the frozen 70% message / 30% eligible-parent context view, duplicate mapping,
support proxy, cosine membership threshold 0.35 and overlap margin 0.05 identical
across methods. Also rerun average-link 0.35 and raw chronological windows of eight
on the same threads. No embedding inference, LLM, new model or representation
tuning. Previous held-out threads are not examined or rescored.

| Method | Three settings | Other fixed choices |
| --- | --- | --- |
| Complete-link | cosine cut 0.35, 0.50, 0.65 | sklearn cosine agglomeration |
| DP-means | squared penalty 1.30, 1.00, 0.70 | arithmetic centres, global-mean initialization, source-order ties, at most 100 iterations |
| OPTICS | xi 0.03, 0.05, 0.10 | cosine/brute, min_samples 5, minimum group size 3 (capped for tiny inputs) |
| Spectral | 4, 8, 16 clusters (capped at n−1) | exp(-(1−cosine)/0.2) affinity, cluster_qr assignment, ARPACK, seed 0 |
| Affinity propagation | preference quantile 0.25, 0.50, 0.75 | negative squared distance, damping 0.8, max_iter 300, convergence_iter 30, seed 0 |

DP-means penalties correspond to cosine cuts only for unit-vector pairs, not
for later unnormalized arithmetic centres. This is a Euclidean DP-means candidate,
not a claim of an equivalent cosine objective. All candidates get the same final
centroid membership rule; report primary-only metrics separately to expose its
influence.

Run each fit in a separate Linux process, one numerical thread, with a 30-second
wall deadline per thread/configuration. Record nonconvergence, timeout, errors,
process peak RSS and grouping time. Incomplete candidates are not ranked as if
they had passed on all threads. Resource limits are screening safeguards, not
changed acceptance gates. No extra settings after viewing results.

For a fresh held-out evaluation, a candidate must first satisfy development
precision >=0.85, recall >=0.70, F1 >=0.75, coverage >=0.80 and improve on the
best simple baseline by >=0.10 F1, with every thread fit completed. Otherwise
stop this screen and report its negative result. Fresh held-out data and stronger
case-specific negative controls remain required before any promotion claim.
The original quality/coherence/provider/performance gates remain unchanged.
Development results alone cannot certify a winner. GRINCH is not included in this
batch; this does not evaluate or reject it. No production cache is implemented.

## Sources

[Kulis and Jordan (2012), DP-means](https://icml.cc/2012/papers/291.pdf)
defines the distance-penalized assignment and cluster-creation loop.
[Scikit-learn spectral clustering](https://scikit-learn.org/stable/modules/generated/sklearn.cluster.SpectralClustering.html),
[affinity propagation](https://scikit-learn.org/stable/modules/generated/sklearn.cluster.AffinityPropagation.html)
and [OPTICS](https://scikit-learn.org/stable/modules/generated/sklearn.cluster.OPTICS.html)
provide the batch implementations. Their documentation is not evidence of quality
on these chat threads.

## Completed development screen

The corpus froze before algorithm outputs with SHA-256
`a4f84e869808f6ae243a973de125cfd25ccd4738e86f63e359f6758c41554773`.
It contains 12,037 embedded messages in 72 independent components: 5,498 messages
in 48 development threads and 6,539 in 24 reserved threads. Development spans
8–716 messages; only the reserved split has a >1,000-message thread (4,118).
Detected lineage/copy components were kept intact and excluded if represented
in the previous study. This prevents detected leakage, not proof that all semantic
duplication is absent. The selected corpus uses the same model generation.

For every development thread, sample six message positions in three two-message
blocks: start, midpoint, end. Review the three adjacent pairs and three fixed
cross-block pairs (first-to-last, second-to-fourth, third-to-last). Excerpts show
140 leading and 90 trailing characters. Freeze six verdicts per thread:
111 related, 117 unrelated, 60 ambiguous (288 total). References are sparse,
agent-authored, and not an independent human annotation. Unclear relations were
marked ambiguous, not forced positive or negative. Exact duplicate projections
were checked. No case-specific acceptance claim is made from these references.

Reference SHA-256:
`dc66aa568ab442e84abc810778e0c0a2abb4e102b0a66bbff4fa9f183d212732`.
The screen records protocol SHA-256
`70015f5d1c4e47866e0eb6d2d07dea117ede17f4fd02007355a7702bee898e47`
(protocol before this results section), and output SHA-256
`d0ae05e9b40b4e31a435664a9ab5fd05ea13a8cc6c49e3afb1764118a017703e`.
Artifacts live in the private `message-clustering-alternatives-20261005` directory
outside Git. Raw source content is not included in the paper.

| Best complete setting per family | Macro P | Macro R | Macro F1 | Coverage | Primary-only F1 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Complete-link cut 0.35 | 0.773 | 0.701 | 0.696 | 0.973 | 0.670 |
| DP-means penalty 0.70 | 0.684 | 0.833 | 0.716 | 0.948 | 0.610 |
| OPTICS xi 0.03 | 0.650 | 0.875 | 0.710 | 0.913 | 0.442 |
| Spectral 8 clusters | 0.755 | 0.748 | 0.716 | 0.951 | 0.556 |
| Affinity preference quantile 0.25 | 0.703 | 0.661 | 0.652 | 1.000 | 0.663 |
| Average-link reference | 0.720 | 0.819 | 0.716 | 0.974 | 0.728 |
| Chronological windows (8) | 0.773 | 0.748 | 0.718 | — | 0.718 |
| Whole-thread baseline | 0.517 | 0.979 | 0.653 | — | 0.653 |

Every complete family fails precision 0.85, F1 0.75, and the required baseline
gain 0.10. All settings' development F1:

| Family | Settings in protocol order | Macro F1 |
| --- | --- | --- |
| Complete-link | 0.35 / 0.50 / 0.65 | 0.696 / 0.693 / 0.597 |
| DP-means | penalty 1.30 / 1.00 / 0.70 | 0.668 / 0.706 / 0.716 |
| OPTICS | xi 0.03 / 0.05 / 0.10 | 0.710 / 0.679 / 0.694 |
| Spectral | 4 / 8 / 16 clusters | 0.646 / 0.716 / 0.673 |
| Affinity propagation | preference q 0.25 / 0.50 / 0.75 | 0.652 / incomplete / incomplete |

Affinity q0.50 failed to converge on one thread, as did q0.75 on another; each
completed 47/48 fits. Their completed-subset F1 values (0.664/0.678) are not
comparable full-cohort rankings. The other thirteen settings completed all 48
fits, as did the reference; no timeouts or other errors occurred
(766/768 total complete).
Do not infer zero clusters or zero bugs from those missing fits.

Provider differences remain: DP-means best Claude/Codex F1 is 0.727/0.708;
spectral 0.712/0.718; average-link 0.665/0.752. The reference primary-only
score exceeds its overlap score, while OPTICS/spectral gain substantially from
the shared overlap rule. Therefore the overlap heuristic itself needs controlled
evaluation; this screen does not isolate the partition algorithm as the sole
quality determinant. Scores from the first study's held-out corpus are not
directly comparable to this different development cohort.

One observed fit per thread gives maximum grouping times across each family's
settings: complete-link 0.078 s, DP-means 1.133 s, OPTICS 1.226 s, spectral
0.045 s, affinity 0.247 s on completed fits, average-link 0.063 s. Peak process
RSS maxima are about 164/163/175/190/197/166 MiB respectively, including the
forked interpreter's inherited input state. These are not repeated p95 values,
end-to-end costs, or proof of 5,000-message scaling; the largest scored thread
has 716 messages. The 30-second deadline includes worker execution/communication;
grouping timings exclude process startup, preparation and scoring. Whole-thread
baseline scores were computed afterward using the same frozen raw preparation
and scorer, without changing candidate fits.

Decision: none qualifies for fresh held-out evaluation under this screen's fixed
rule. Keep all 24 reserved threads unjudged and unscored; stop this fifteen-setting
batch without expanding thresholds. This is evidence against these particular
pipelines, not a claim that other algorithms or better representations cannot
work. More robust annotation, dialogue-aware units, overlap calibration and
GRINCH remain distinct possible follow-ups. No production behavior changed.

## Next representation question (not measured here)

The user proposed grouping prompts and answers before embedding/clustering.
Compare message units with user-turn units: prompt alone, combined prompt/answer
text, and prompt plus bounded answer passages. A pooled combination of existing
message vectors is a cheap separate control, not equivalent to jointly embedding
the text. Avoid using tool/progress boilerplate as answers, crossing explicit
parent/fork boundaries, or losing canonical source mappings. Account for multiple
assistant messages per prompt, incomplete active turns, multi-subject answers,
and tokenizer limits/truncation. Preserve the same original-message pair scorer
so changing unit counts cannot manufacture a quality gain. Freeze rules and
development settings before new outputs; keep reserved threads closed until a
candidate qualifies. This follow-up is tracked separately, not implemented or
claimed as a result of this screen.
