# Context helps broad services but hurts within-service intent separation

Exploratory follow-up, 2026-10-01; Bead `mash-c0d.19.2.30` remains in progress.
Reuse the frozen 32-dialogue selection and vectors from
[the external context experiment](2026-10-01-external-dialogue-context.md).
No new inference, tuning, annotation features or test data.

## Procedure

Before computing these new outcomes, record the intent/domain slices and 1,000
paired whole-dialogue bootstrap draws in Beads. This follow-up was designed after
seeing broad-service results; it is exploratory, not independent confirmation of
the original experiment. Preserve original capture identities/code hashes.

`multiwoz.py` now also retains schema-declared `(service, intent)` values in its
evaluation-only `Label`. Aliases with the same complete transcript and service
but conflicting declared intents now fail validation. Original `Turn` objects
and model inputs are unchanged. Synthetic tests verify the distinction.

`external_analysis.py` requires the exact archived vector capture SHA256
`62eff98049bd418f359500c5c2d7b9c4bab2f879d48b639dcb3e6dd8e8f74b0c`,
revalidates all selected groups, original text/input hashes, proxy annotations,
vector shape/norm, supports and complete common pair grids against the pinned
source. It never initializes an encoder and makes zero new model calls. Unknown
`find_taxi`, empty proxies, source aliases and roots remain excluded as before.

All controls use the same 12,341 cross-dialogue common-support pairs. Intent
positives share a declared `(service, intent)` pair; within-service intent
evaluation retains only pairs sharing an active service, then distinguishes
matching intents from other intents within that service. Supported intent types
are attraction/find; hotel/find/book; restaurant/find/book; train/find/book.
These task-state proxies still are not granular coding-session topic gold.

Per-service slices retain pairs where at least one endpoint has that service;
positives have the service at both endpoints. A pair may enter multiple slices,
which have different prevalence; do not compare their AP as if supports matched.

Uncertainty uses 1,000 fixed-seed (`20261001`) resamples of the 32 complete dialogue
groups, each drawing 32 groups with replacement. For an original cross-group pair
`(i,j)`, use weight `multiplicity(i) * multiplicity(j)`; never fabricate same-original
dialogue pairs when a group is drawn twice. All methods/slices share the same draws.
Report empirical 2.5/50/97.5 percentiles of paired AP/AUC changes versus current text,
and report every one-class replicate rather than repairing it. There are 31 groups
in the eligible pair grids; one selected group has no eligible common pairs.
All slices support both classes in all 1,000 draws.

This group resampling is inspired by vertex-based treatment of dependent pair
data ([Chen et al., 2018](https://journal.r-project.org/articles/RJ-2018-056/RJ-2018-056.pdf)).
Coverage for our nonlinear AP/AUC statistics is not calibrated; these are empirical
resampling ranges, not validated confidence coverage or formal significance tests.
Source templates, annotation mismatch, small group count, repeated development
inspection and seven correlated slices remain limitations. No multiple-comparison
claim or coding-session transfer is made.

## Results

| Slice | Pairs | Positive | Current AP | Mean AP | Joined AP |
| --- | ---: | ---: | ---: | ---: | ---: |
| Service overlap | 12,341 | 3,448 | 0.464235 | 0.534925 | 0.555472 |
| Intent overlap | 12,341 | 2,488 | 0.399102 | 0.415504 | 0.417893 |
| Intent within shared service | 3,448 | 2,488 | 0.802038 | 0.752176 | 0.737828 |
| Attraction | 5,807 | 849 | 0.223221 | 0.226965 | 0.226880 |
| Hotel | 6,418 | 1,104 | 0.281696 | 0.351224 | 0.381683 |
| Restaurant | 4,742 | 531 | 0.155075 | 0.189513 | 0.232215 |
| Train | 6,103 | 970 | 0.489262 | 0.534548 | 0.566958 |

| Slice | Mean paired AP range | Joined paired AP range |
| --- | --- | --- |
| Service overlap | [0.030232, 0.113276] | [0.037493, 0.145551] |
| Intent overlap | [-0.025443, 0.060741] | [-0.033016, 0.073371] |
| Intent within shared service | [-0.075000, -0.019972] | [-0.097649, -0.030696] |
| Attraction | [-0.039911, 0.061792] | [-0.056242, 0.070886] |
| Hotel | [0.019614, 0.142323] | [0.023692, 0.207558] |
| Restaurant | [-0.009859, 0.095341] | [0.005152, 0.172589] |
| Train | [-0.026684, 0.118267] | [-0.010409, 0.179941] |

Overall intent AUC is 0.656245/0.691091/0.692769 for current/mean/joined. For the
harder within-service slice it is 0.583550/0.512996/0.490344; paired AUC ranges
are [-0.115989, -0.016328] for mean and [-0.149669, -0.022709] for joined.
Within-service positive prevalence is 72.16%, so its high AP does not imply
strong fine-intent separation; AUC exposes the weak discrimination.

Context appears to strengthen domain similarity while obscuring find/book
differences. This is a hypothesis from the controlled metrics, not an established
causal explanation. It invalidates promotion of joined context as the sole topic
representation on the basis of the previous broad-service improvement. Attraction
shows almost no AP gain, while hotel gains more; the overall result is not uniform.

## Artifacts, verification and remaining work

Private captures remain under
`/home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001`.
Input provenance, current/historical code hashes, NumPy/scikit-learn versions,
complete original vectors via pinned input, augmented intent labels, all pair
scores/truth, bootstrap multiplicities and every paired replicate delta are retained.
The original inference capture is not regenerated or rewritten after the loader
annotation extension. Reproducing its exact code pins requires commit `1ba0bca`.

```bash
env PYTHONDONTWRITEBYTECODE=1 OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
  /home/james/scratch/mash-artifacts/topic-python-env/bin/python \
  research/topics/external_analysis.py \
  --root /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001 \
  --capture /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001/context-controls-final-first.json \
  --output /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001/intent-bootstrap-new.json \
  --compare /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001/intent-bootstrap-first.json
```

The runner refuses to overwrite captures. Require fresh-run equality of every
non-timing field, including all resampling draws and per-replicate deltas.
Two fresh runs pass this exact equality check. Capture SHA256 values:

- `intent-bootstrap-first.json`:
  `b31098e45cec9d521690e67fc395c90819ca7aad01dbc2b98672cb20dd587ec6`.
- `intent-bootstrap-second.json`:
  `46a903d0adad9cf00a6a6c8c5623cb4cbddf911a910612b099e93886d5b6e8e2`.

NumPy 2.4.6/scikit-learn 1.9.1; complete analysis after imports takes
101.24/98.96 seconds, including source/capture validation, all seven slices,
1,000 draws and 21,000 weighted method evaluations. Output serialization is
excluded. This repeatability test does not calibrate statistical coverage.
All 171 isolated research tests pass, including source/input/vector drift,
within-service intent conflicts, class absence, weight products, ties and paired
identical-method zero deltas. The manuscript builds with the new methods/results.
The bootstrap is offline evaluation cost, not per-document production work.
No ingestion-performance, memory or Rust-parity gate is established.

Next preregister bounded representations that retain the original utterance's
specificity while admitting context where useful. Compare within-service negatives
as well as broad overlap; never use gold intents to choose model inputs or add
approval-phrase exceptions. Independent coding-session aspect, label, hierarchy
and guided-search evaluation remains required before method selection.
