# Frozen topic labels: document coherence and reuse diagnostics

Bead `mash-c0d.19.2.30.4.4`. Measure the existing labels rather than changing
them or tuning words on observed failures. Corpus-IDF improves these diagnostic
coherence/reuse measures, but neither measurement establishes semantic quality.

## Frozen procedure and prior art

Keep all three top-eight label lists from each of the four verified
[PCA25 snapshots](2026-10-02-fixed-pca-topic-routing.md): plain c-TF-IDF,
corpus-IDF multiplication and lexical-containment diversity. No memberships,
labels, vocabulary, thresholds, source texts or source counts change.
Verify both capture manifests and every array hash. Only original candidate
counts are features; queries, duplicate references and reserved outcomes are
never consulted. No inference, downloads, generative or paid calls occur.

The [Lau, Newman and Baldwin study](https://aclanthology.org/E14-1056/)
evaluates topic coherence using NPMI among topic words. Our estimator deliberately
differs: binary presence in each of the original 32,176 candidate documents,
with source-adjacent technical unigrams/phrases, no lemmatization or external
sliding-window reference corpus. This is not a replication or a human-validated
coherence score for agent chats.

For document frequencies `a,b`, joint frequency `c` and denominator `N`, use
`NPMI = log(c*N/(a*b)) / -log(c/N)`. No smoothing. Zero joint occurrence has
limit -1. Universal joint occurrence is explicitly undefined/null, not forced
to one; fewer than two labels has undefined topic coherence. Validate feasible
contingency counts. Retain every term-pair count/score, zero/undefined count,
and lexical-containment flag. Topic score averages all pairs; macro score is
defined only if every topic is defined. Report distinct terms / total label
slots within each snapshot and variant, not as a claim of rare-topic coverage.

Build int64 binary-presence pair counts only for the 190 selected terms (36,100
matrix cells). There are 186 topic-label lists, 1,488 label slots and 5,208
within-label pairs. Original term repetition frequency does not multiply
document presence; the same question contributes at most once per pair.

## Outcomes

| Order / membership | Plain NPMI | Corpus-IDF NPMI | Diverse NPMI | Plain unique ratio | Corpus-IDF unique ratio | Diverse unique ratio |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Forward / hard | .158006 | .271666 | .242633 | .643750 | .862500 | .850000 |
| Forward / overlap | .171482 | .280891 | .249384 | .687500 | .868750 | .862500 |
| Reverse / hard | .137989 | .264893 | .240454 | .693182 | .920455 | .909091 |
| Reverse / overlap | .163537 | .291133 | .244878 | .761364 | .943182 | .920455 |

All topic scores are defined. Plain/corpus-IDF lists have no zero-joint pairs;
diverse lists have 3/4/0/2. Plain containment pairs are 7/8/2/3, corpus-IDF
18/21/10/12, diverse zero by construction. Diverse score decreases do not mean
worse names: avoiding nested word/phrase labels changes the measured pair set.

A deterministic posthoc inspection of the highest corpus-IDF coherence list in
each snapshot finds the same testing vocabulary: `test, tests, unit, testing,
unit tests, unit testing, unit test, tdd`, with order permutations, NPMI .560010.
This is recognizably topical but repetitive; the score alone does not reward
concise, nonredundant naming. Code tokens can also co-occur consistently.
Corpus-IDF's numerical gain is evidence for label weighting, not proof of useful
topics, semantic ancestry or agent navigation. No semantic acceptance threshold
or automatic cluster rejection is introduced.

## Verification, costs and limits

Two fresh captures match every decoded nontiming field. Independent source
posting-set intersections validate all 190 DFs and all 36,100 joint matrix
cells. Independent scalar log-probability reduction checks all 5,208 label
pairs; maximum NPMI difference 2.23e-16. Original selected label order,
containment flags, zero/undefined counts and all macro summaries agree.
364 tests pass, including independent events, perfect association, disjoint
events, infeasible contingency rejection, universal-event abstention, binary
presence versus frequency, phrase containment and singleton labels.

Same Linux/AMD EPYC/Python/science environment as prior studies, one numerical
thread. First/repeat after-import wall times 1.504/1.246 s; separate rusage
345,688/345,796 KiB. External samples 135/129, one/zero vanished reads, requested
sleep 20 ms, actual median/max gaps 26.96/93.01 and 25.70/34.18 ms. Smaps RSS
maxima 346,244/346,408 KiB, PSS 337,099/333,118 KiB; approximate status VmHWM
matches RSS here. Snapshot loading, imports, concurrent audit and sampling
contribute; target-only scope excludes children/observer, sampled maxima are
lower bounds. These are not production ingestion/worker costs or the overhead gate.

This transductive document-presence estimator can inflate association in long
multi-topic sources or mechanically contained phrases. It does not assess
topic membership correctness, unknown-topic discovery, secondary-topic recall,
semantic parenthood, domain specificity or downstream retrieval. Independent
label intrusion/specificity and guided-navigation evaluations remain necessary.

## Reproduction

Run `cqadup_label_coherence.py --topics PCA_FIRST --topic-repeat PCA_SECOND
--output FRESH_JSON`, wrapped by `memory_audit.py`; fresh repeat adds
`--compare FIRST_JSON`. Outside-Git artifacts under mash-artifacts:

- First `topic-cqadup-label-coherence-first-20261002.json`, SHA256
  `2fd7b585aa6986b1786742b397797251c0f697787c2f599ee858177355601c9b`.
- Repeat `topic-cqadup-label-coherence-second-20261002.json`, SHA256
  `4f8a0b6cbf7dd8dcba6107235d38d5fc0331036a7208f51214338d52fd0653de`.
- Audit `topic-cqadup-label-coherence-audit-first-20261002.json`.
- Monitors `topic-cqadup-label-coherence-memory-first-20261002.json` and
  `topic-cqadup-label-coherence-memory-second-20261002.json`.

These diagnostics establish a baseline for prospective label-only comparisons.
No production topic method or semantic-quality gate passes from this measurement.
