# Stored message vectors and frequency penalties

## Source and prospective procedure

This complements the [complete question-unit experiment](2026-10-02-bounded-question-units.md)
with actual coding histories and **zero new embedding calls**. It does not replace
independent semantic/topic/ancestor judgments. Earlier geometry studies already
used the frozen corpus; the new representation results remain exposed development.
The complete procedure was recorded in `mash-c0d.19.2.33` before each stage's outcomes.

`stored_vector_reuse.py` opens the frozen SQLite snapshot read-only, verifies
generation `ca9c7766f955402eb34db3c324da24a512e1118de44e3d9aa9a0faeeac0da5f4`,
fingerprint `2d219b5cb5947231612d4612e6f930e6d6ac740ea00d9cf2eaf9606c0bc772a2`
and canonical partition. Exclude all 16 original/v2/v3 reviewed source groups
including aliases. Decode stored 384D signed-byte vectors with dimension/norm/
generation checks, preserving exact message identities, ordinals, projection
hashes and raw vector hashes. Deduplicate identical within-thread inputs and
reject conflicting stored vectors. This verifies frozen stored directions, not
complete Python/Rust encoder parity or the correctness of upstream admission.

From 161 canonical threads with 2–128 distinct nonempty stored user projections,
select 128 by SHA256(`mash-stored-vector-reuse-v1-20261002` + group bytes). Do not
use metadata, source judgments or model scores for selection. Only afterward,
read exact project identity as an ancillary reference. Missing/blank identities
abstain without replacement. No path normalization, project-name embedding,
new inference, clustering, cutoff selection or reference-derived feature.

Compare normalized message-mean cosine, maximum cross-message cosine, and the
arithmetic mean of the two directional nearest-message cosine averages. The last
view asks whether messages match broadly rather than requiring only one high
match. It remains sensitive to thread length and does not identify semantic aspects.

## Coverage and first-stage result

The sample has 90 Codex and 38 Claude Code threads, 26 exact project identities,
1,812 distinct stored vectors and 38 duplicate within-thread inputs, versus 8,888
original user messages. Thus 1,850 nonempty stored source occurrences represent
only 20.8% of user-role records. The later [projection audit](2026-10-02-projection-coverage.md)
corrects the embedding-completeness interpretation: all 1,850 nonempty search
projections have vectors; all other selected records have explicit empty overrides.
Native corroboration identifies 6,744 excluded Claude tool-result-only records.
This denominator is not independent human-turn or semantic topic coverage.
Selected threads have 2–128 vectors. All 8,128 thread pairs have metadata support:
1,289 equal-project positives and 6,839 negatives, prevalence .15859.

| View | AP | AUC |
| --- | --- | --- |
| Normalized mean | .31535 | .58795 |
| Maximum message match | .33121 | .60300 |
| Bidirectional nearest mean | .30906 | .54586 |

The maximum has a concrete recurring-input confound: 400 positive and 873
negative pairs share an identical message projection. Scores are at least
`1 - 1e-12` for 401 positive and 876 negative pairs. Some near-unit scores therefore
occur without exact input equality. Do not label every repeated input filler:
source content has not been independently classified, and project equality is
not semantic topic overlap. Different projects may legitimately share a topic;
same-project threads may discuss unrelated tasks. Project paths can also differ
across nodes for the same repository. No private strings or project names are published.

## Post-hoc frequency mechanism control

After the first-stage finding, freeze a separate control before its outcomes.
`stored_frequency.py` requires the exact first capture and original code hashes.
Count document frequency once per selected thread for each exact input SHA256.
For N=128 and a source input with frequency df, use

`w = (1 + log((N + 1)/(df + 1))) / (1 + log((N + 1)/2))`.

Compare cosine of the normalized weighted message sum and maximum cross-message
cosine multiplied by both messages' weights. Preserve all vectors and source
occurrences, without threshold tuning or removal of common strings. Project
metadata never enters DF. There are 1,642 unique source inputs with DF 1–47;
weights range .38489–1. Rare inputs have weight 1. This is whole-input frequency,
not word IDF or evidence that conversational fragments have been understood.

| Fixed frequency control | AP | AUC |
| --- | --- | --- |
| Weighted mean | .33197 | .59783 |
| Weighted maximum | .36408 | .68657 |

Weighted maximum gains .03287 AP/.08357 AUC over unweighted maximum; weighted
mean gains .01662 AP/.00988 AUC over mean. This supports testing frequency-aware
reuse further, but cannot select a topic method on this exposed incomplete
project-scope proxy. Multiplying negative cosine by small positive weights also
moves it toward zero; interpretation depends on score ordering. Rare noise and
secondary-topic loss remain possible. There is no independent quality judgment,
semantic tree, labeling, guided retrieval or cutoff-precision gate.

DF is transductive on these 128 source threads. New/edited/deleted threads change
weights and possibly old rankings; online DF maintenance and reassignment must be
defined and measured rather than treating these fixed snapshot weights as stable
topic identities. No clustering or persisted assignments are implemented here.

## Cost and verification

Stored-vector runs take 3.235/2.172 s after imports: collection 737.89/390.44 ms,
all-pairs scoring 1283.78/1150.34 ms. Total also includes complete corpus/partition
validation. There are 1,582,677 cross-message comparisons. Frequency-control runs
take .417/.347 s, weighting 16.42/15.31 ms and scoring 188.01/140.95 ms. The two
runners have different validation/data-loading scope; these totals are not an
algorithm speed comparison. Output serialization is excluded. No model is loaded,
downloaded or called. No inference cost does not imply no topic-ingestion overhead:
this is neither production initial/incremental ingestion nor a CPU/RSS/p95 gate.

Both stages repeat exactly outside timing fields. Independent SQLite reconstruction
verifies source-only selection/exclusions, all 1,812 decoded vectors and their
input/raw hashes, source counts, all 8,128 project truths, all five score grids,
all DF/weights and all five tie-aware AP/AUC results. The research suite passes
236 tests. Tests include competing secondary matches, cancellation/nonfinite
rejection, duplicate conflicts, generation drift and per-thread DF invariance.
The first invocation failed on a nonexistent v3 TOML file before outcomes;
v3 exclusions use the actual source-pinned JSON review packet, not a conversion.

Private captures are outside Git under `topic-stored-vector-reuse-20261002`:

- Stored first: `270fdf50405136931791905b8555f5e4d7a35cb5a5fd935c4a201fd72b618167`.
- Stored repeat: `583a3b223e20a355ab04f81c4d9cb2c30571127bb632245099851eaab566976e`.
- Frequency first: `c8722463953eae559510328446f50469877c12b7096cadf32a5b9dceadf52a1a`.

Next prioritize validated evidence/context admission, genuinely annotated aspect pairs
and independently reserved thread outcomes. Freeze any selection before evaluation;
require label/ancestor/guided-search and full ingestion/lifecycle gates before Rust.
