# Bounded block representations before macro-topic discovery

The [cut-free hierarchy experiment](2026-10-02-qmsum-hierarchy.md) does not justify
tuning another frontier of the current tree. This prospectively frozen control
tests source-only bounded pooling before another discovery method. It neither
detects semantic episode boundaries nor selects production topic representations.

## Frozen procedure

All162 QMSum training meetings/88,336 original turns. Reuse the audited installed
raw embeddings, common-ready mask, original model token counts, exact-marker-cleaned
source counts and frozen source IDF. No inference, model download, LLM or held-out
author validation/test source. Original capture pins:

- Source: `55964a968e4b4c1c9da2434678f213f3b70c0c3d3cf609f5cd4ecd44cc8d2427`.
- Vectors: `05642c1a77dab33d2940c39979b0ac4e4b420420debcb30ca529d7bad99bdf98`.
- Causal units: `e22d6878b6640dec38f7f2ced42956ae73abfaa3e1502741743a82d44c3eff33`.
- Counts: `064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08`.

One block policy: at most16 contiguous original turns and sum of original model
token counts<=512. Unsupported gaps flush without compacting source order.
At a next ready turn, test turn bound before token bound; gaps/end flush first.
Counts include each turn's model special tokens, not a newly tokenized concatenation.
Equal float64 raw-direction sum then L2 at block closure; norm<=1e-12 cancels
explicitly. Unknown/multi-annotated ready turns contribute to features. Original
lexical counts are summed once, weighted by `(1+log tf)*IDF`, then L2 normalized.
Frozen IDF uses the full training corpus, including the internal reserved partition.

Compare raw dense, causal-five dense, block dense, raw lexical, block lexical,
raw/block RRF60 and original chronological distance. Queries are source turns
represented with the corresponding source representation, not independently
written user queries. Block representations can contain later turns until closure;
these are final closed-block snapshots, not causal quality at initial turn arrival.
Clock-based idle flushing, edits/deletes and draft-topic publication are undefined.

All methods share matched singleton-known query/candidate support. Require original
gap>4, distinct physical blocks, and exclude same native topic within the same
merged original span. Positives are the same topic in another native span.
Unknown/multi sources stay in pooled features but outside conditional gold;
no assumption they are noise. Cancellations would remove their sources from every
method. For positive-score retrieval, round to12 decimals, sort descending then
original ordinal. Zero/negative scores abstain rather than create lexical matches.
RRF uses the [original reciprocal-rank formula](https://plg.uwaterloo.ca/~gvcormac/cormacksigir09-rrf.pdf)
with fixed k60; positive-branch abstention and score rounding are this control's
rules. Keep all relevant sources in AP's
denominator, even when a branch cannot retrieve them. Report AP/MRR/Hit10 and
topic-macro AP, complete relevant ranks/top10/order hashes, explicit no-return
queries and original source/block lineage.

Before outcomes, split ordinal%3==0 as54 reserved-internal meetings, other108 as
development. All training sources were explored in prior experiments: this is
**not pristine held-out evaluation**, and neither partition calibrates a parameter.
No cutoff/budget grid. This study is message-expanded: members of a block share
one pooled representation but remain separate candidate messages.

## Complete outcomes

84,434 usable ready sources/77,892 known;8196 blocks, zero cancellations.
7242 blocks have one known native topic,421 mix known topics,533 have no known
topic. This is broad annotation support, not semantic purity or clean noise labels.
Eligible pairs18,961,803; return positives84,323 after matched block exclusions,
27 fewer than the prior84,350. There are2410 eligible return queries across29
topics/21 meetings;75,482 other known sources have no eligible return and are not
silently counted as correct retrieval. Prior span diagnostic had30 supported topics/
22 meetings, so compare within this study's matched support.

| Representation | MAP | MRR | Hit10 | Topic-macro AP |
|---|---:|---:|---:|---:|
| Raw dense | .300767 | .457126 | .772199 | .245970 |
| Causal-five dense | .320640 | .423800 | .695851 | .254918 |
| Block dense | .348504 | .364900 | .453527 | .333904 |
| Raw lexical | .134592 | .430315 | .745643 | .134682 |
| Block lexical | .368752 | .321091 | .478423 | .359152 |
| Raw RRF60 | .300241 | .458464 | .782158 | .246714 |
| Block RRF60 | .356136 | .377863 | .528216 | .348526 |
| Chronological distance | .262109 | .037070 | .077593 | .216470 |

Raw lexical has30 zero-result queries; all other branches have none here.
Development support1707 queries/18 topics/12 meetings; reserved703/11/9.
Reserved raw dense MAP/Hit10 .255499/.812233 versus block dense .301499/.419630,
block lexical .362450/.568990 and block RRF .334379/.539118. Pooling improves
average/topic-macro precision but loses early retrieval in both partitions.
It is not an unequivocal quality gain or a selected replacement for raw search.
Native broad-topic returns are sparse, correlated meeting evidence, not coding
labels, multi-topic/ancestor gold or guided-search results.

## Post-hoc duplication diagnostic

Without changing frozen scores, map every top10 message to its physical block.
Raw dense averages7.61 unique blocks/top10; block dense1.53, block lexical1.62,
block RRF1.99. Raw dense has zero top10s wholly in one block, versus1561/2410
(64.77%) block dense,1450 (60.17%) lexical and831 (34.48%) block RRF.
The message-expanded evaluation can consume its result budget with copies of one
block representation. This diagnostic does not establish that duplication explains
all early-retrieval degradation; geometry itself may lose useful detail.

Do not promote the MAP increase or run macro discovery yet. Next freeze one result
per physical block for **all** branches, with max eligible message score for raw
passages, duplicate-free branch ranks before RRF, explicit mixed-topic relevance/
graded known support and matching query support. Changing result granularity also
changes relevance counts: do not compare new block-level AP numerically as a direct
gain over message-level AP.

## Validation and stage cost

Four new fixtures cover gap/token/turn bounds, immutable sources, cancellation,
count ownership, sublinear TF-IDF arithmetic, positive-score abstention and tie/RRF
rules. Full enabled lab suite:476 pass. Complete fresh repeat matches every
nontiming field, lineage, vector/file hash, query/ranking/summary/protocol/code pin.
First/repeat result SHA256:
`3f0b2c8632b8dda5ad34f3d058032cb24afb99d776998060e603b92cac63fdb1` /
`6d07ae089678f0afe30e9fa54228e6d866c3c611215a3a1eead6ed1d1c77d7e3`.
Block8196x384 float64 file is byte-identical SHA256
`95882ec78c20cf5ff0065343bf9463f94101428aecb3a62b22b6edd13cefb436`.

Separate original-source auditor imports no lab feature/ranking/source helpers.
Independent greedy block reconstruction, token/gap/ownership checks, scalar norm
and source-count aggregation, lexical normalization, native memberships, direct
candidate eligibility, cosine matrices, Python sorting and AP arithmetic verify
all88,336 turns/8196 blocks/2410 queries/19,280 rankings/24 summary cases.
Maximum direction error5.55e-17, metric error3.33e-16; integer support, all ranked
source hashes and top10s exact. Shares NumPy/SciPy; prior count/inference audits
are prerequisites, not independently repeated encoder execution. Auditor SHA256
`a0e597df4c1c994bc24bef7b23e40054f48790ebc6e0ce041f2ca1ca30a65074`;
terminal `topic-qmsum-block-geometry-audit-first-20261002.json`.

Pool wall/CPU across all original slots first .312/.276s, repeat .312/.262s.
These are closure-pooling stage totals, not per-update p95 or complete ingestion.
First after-import wall/CPU22.125/15.773s, setup16.752s; repeat19.243/14.896s,
setup14.688s. Cosine/lexical scoring .868/.785s, ranking1.037/.964s. Includes block
NPY writing; imports/final JSON/repeat comparison lie outside after-import timer,
inside external target scope. No timing-variation speed claim; first briefly
shares startup with lab validation and external observers are intrusive.

First rusage2008612KiB; sampled RSS/PSS/status2010012/2002232/2010012KiB,
596 samples,zero vanished,median/max gaps38.92/101.17ms. Repeat rusage2008228KiB,
RSS/PSS/status2009468/2001336/2009468KiB,600 samples,two vanished,gaps34.95/79.75ms.
This is full offline capture loading/scientific imports/input ledgers, not local
model-worker memory. One target process includes parsing/output/comparison; observer
and children excluded. Sampled maxima are lower bounds, status approximate,
launcher high water can persist; no subtraction. Full-ingestion <=10% provisional
overhead gate, persistence and lifecycle are unverified.

Post-hoc duplication/cost script SHA256
`2ffa2abcc9265b0b8871f5c8fef8fbd2cff0d05b0da692869e7e6e2c76520851`;
terminal `topic-qmsum-block-geometry-diagnostics-first-20261002.json`. Raw artifacts
stay outside Git. Driver `qmsum_block_geometry.py` arguments match discovery control:
`--source --vectors --units --labels --baseline --reference-manifest
--output-directory`, repeat `--compare FIRST/results.json`, run via
`memory_audit.py --output MEMORY -- COMMAND`. Same pinned scientific environment.

Study bead `mash-c0d.19.2.20.8.3.3.2.1` complete; parent active, P0 unique-block
follow-up `.2.2`. Coding topic/label/multi-tag/ancestor/guided-search evidence,
shared namespaces, lifecycle, full ingestion and chosen Rust parity remain open.
