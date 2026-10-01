# Section allocation and concrete target coverage

2026-10-01; `mash-c0d.19.2.29`, following
[bounded section associations](2026-10-01-section-associations.md).
No production topic method is promoted.

## Frozen controls and source judgments

Keep the same74 source units, six oracle pairs/four exposed cases, prior exact
rankings and eight layout associations. Add an opt-in one-child-per-ranked-hit
allocation alongside unchanged standalone and greedy linked-child allocation.
The new control supplies only the first linked child at an intro hit, leaving
other children eligible at their own original ranks. Unique budgets1/3, source
order, deduplication, intro provenance and abstentions remain fixed. This does not
guarantee section/source diversity or globally round-robin semantic groups. No
query/approval phrases, target names or annotations enter the selector.

Before inspecting new allocation outputs, review concrete targets for every one
of the74 source units. The same coding assistant has already seen source texts,
old rankings and greedy results; this is an exposed development diagnostic, not
blind or independent human annotation. There are20 primary named targets:4easy/
next-tier families,4remaining-work families,10proposed reorganization contracts,
and2migration/compatibility subjects explicitly excluded from requested work.
Generic criteria and unrelated keep constraints do not count as evidence for
these primary named contracts. This is a chosen target rubric, not an exhaustive
topic taxonomy. Rejected work remains topical; its target coverage is separate
from included targets.

`target_coverage.py` pins the original manifest/scope review and requires complete
anchor/unit coverage. Target evidence must match exact source-unit identity/digest;
included targets need topical/included evidence, excluded targets need topical
excluded/mixed evidence. Reject unknown/duplicate/unsupported targets, stale pins,
changed hashes, missing units, conflicting scope and selections beyond their
unique-unit budget. Names/descriptions/rationales are evaluation-only. Integrity
checks do not prove the semantic correctness of the assistant's judgments.

## Results

Both prior control grids reproduce exactly. Two fresh3-control/4-query/2-budget
captures match every nonmeasurement field. For current-user queries at budget3:

| Case / primary targets | Standalone covered | Greedy children covered | One child per hit covered |
| --- | ---: | ---: | ---: |
| Easy/next-tier:4 | 1 | 3 | 2 |
| Remaining work:4 | 4 | 3 | 4 |
| Reorganization contracts:10 | 0 | 0 | 0 |
| Rejected migration/compatibility:2 | 2 | 2 | 2 |
| Total:20 | 7 | 8 | 8 |

Included-scope unit counts are5/10,9/10 and6/10 respectively; included-target
coverage is5/18,6/18 and6/18. Both section controls cover all2excluded-but-topical
targets. On common3-anchor query support, total target coverage is3/16,5/16 and
4/16 respectively. Do not mix those denominators with own4-anchor support.

One-child allocation preserves the separate next-tier family and the remaining-work
overview containing all four families, but admits the broad mixed all23 interpretation
and a completed-work bullet. Greedy supplies more included units but crowds out those
other aspects. Neither finds any of the ten concrete reorganization contracts: the
selected criteria/keep/background material is insufficiently specific. The mixed
migration unit remains unresolved. Thus higher included-unit purity does not imply
more useful topic detail, and neither allocation dominates or satisfies the quality
gate. No default or production port follows.

## Repeat and performance limits

157 lab tests pass, including unknown allocation rejection, preservation of original
controls, target scope separation, no unsupported/missing source coverage, selected
evidence identity/digest/deduplication, explicit query support and budget overflow.
Two final target summaries are byte-identical. No new inference occurs; prerequisite
passage ranks still used81calls. Three-control layout0.91–0.94ms, selection/counting
0.74–0.76ms, complete validation/loading23.24–24.19ms after imports. These are
descriptive six-record costs, not ingestion, memory or production latency gates.

Private root `topic-python-review-v3-20261001`:

- `section-target-review.json`: SHA256
  `f4f60c40177ecf52cef467415d10971484d9d1b26334484fd5702aadb138702e`.
- `section-allocation-{first,second}.json`: SHA256
  `c7e626898691f4a985390d624a228de925d801de4524088b4a25c77194bc39cb`,
  `6872b77e7cf8ddf36faecd4ac00556870a92b17f893a60e75aa5dfa8a504656d`.
- `section-allocation-summary.json`: own/common support and repeated measurements.
- `target-coverage-confirmed-{first,second}.json`: byte-identical SHA256
  `04afd805cd1deb80f6d90f0d6604c600be8f6e6a65d22592d8c1fae9c0fba434`.

Earlier target summaries predate the explicit budget-overflow check and are excluded.
Run `section_experiment.py` with its prior inputs plus `--one-child`; repeat with
fresh output/`--repeat`/`--summary`. Run `target_coverage.py --first ... --second ...
--manifest ... --scope-review ... --annotations ... --output ...` twice with fresh
outputs. Current default section controls remain the original two. Histories and
judgments stay outside Git. Wider source/provider evaluation remains required.
