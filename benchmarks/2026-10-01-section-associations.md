# Bounded section associations for source evidence

2026-10-01; `mash-c0d.19.2.29`, following the
[two-axis passage diagnostic](2026-10-01-two-axis-passage-scope.md).
No production method is promoted.

## Frozen procedure

Reuse both already verified passage-ranking captures, the same74 structural units,
six oracle source pairs, four known cases and unchanged two-axis judgments. Validate
complete baseline grids/scores/coordinates, source/code/review hashes and regenerate
all causal source units before accepting the new control. This remains a post-ranking,
single coding-assistant diagnostic on one exposed Codex group. The source shortlist
is oracle-selected; this is not representative, blind or independent quality evidence.

Freeze associations before inspecting results: an explicit Markdown heading or a
paragraph ending in a colon (including closing emphasis) can link to the immediately
following list/table run in the same record. Stop at any other kind, including prose
and protected code. Table framing remains an explicit source gap. Store at most4
child identities, plus the total run length; capped children remain ordinary ranking
candidates. This is a bounded layout rule, not general Markdown parsing or semantic
hierarchy. No task terms, approval phrases or judgments are consulted.

Compare standalone selection against heading-to-child replacement using identical
unique-unit budgets1/3. Traverse the original ranking; an associated intro supplies
its linked children in reading order, otherwise supply the original unit. Deduplicate
identities, retain the matched intro as `via` provenance, and stop at the budget.
Do not recurse or cross records. Return the original child text identity, code-point/
UTF-8 coordinates, digest and original score; the child score is not a new heading-
conditioned similarity. No text concatenation, rewriting or new inference occurs.
Query abstentions are unchanged. Evaluate relevance/scope only after selection;
standalone counts must exactly reproduce the original reviewed baseline.

## Results and coverage losses

Eight associations are found; four list/table runs exceed the4-child link cap.

| Mode | Own supported anchors | Returned at budget3 | Standalone included | Section included | Section mixed |
| --- | ---: | ---: | ---: | ---: | ---: |
| Current user | 4 | 10 | 5 | 9 | 1 |
| Preceding user | 3 | 7 | 0 | 3 | 1 |
| Captured user chain | 3 | 7 | 0 | 3 | 1 |
| Recency | 4 | 10 | 0 | 0 | 2 |

Current-user budget1 changes from1included/1mixed/2nontopical among4 returned to
3included/1mixed. On the common3-anchor cohort, budget3 included counts are current
3→6 of7, preceding/chain0→3 of7, recency0→0 of7. Own-support denominators differ;
the migration case has only one candidate, so cannot return three distinct units.

The improved count is insufficient for topic quality:

- Easy/next-tier fixes: the matched easy-fix intro now supplies three concrete
  families, but exhausts the budget before the separately requested next tier.
- Remaining work: the remaining-work intro supplies process waits, returned
  supervisor and goroutine joins. The channel-contract family is dropped. The
  standalone ordering overview explicitly mentions all four remaining families,
  so replacing it can reduce aspect coverage despite greater included-unit purity.
- Reorganization: expansion supplies three decision criteria rather than named
  split targets. These are reviewed included constraints but generic label evidence.
- Migration: the single mixed paragraph has no matching section layout and stays
  mixed. Layout association cannot establish accepted atomic scope.

Previous/chain queries still favor the intervening installation task for deferred
reorganization. Recency still chooses the wrong source, unaffected by better layout.
Association improves navigation from some relevant headings, but greedy children
can crowd out other sections/aspects. Do not promote9/10 as semantic precision,
topic-label quality, a complete context resolver or guided-search gain. Broader
source coverage, multiple-aspect evaluation and fine mixed-scope boundaries are
still needed. Next compare section/source-diverse allocation under fixed budgets,
alongside specific-target coverage; do not optimize only the included-unit count.

## Reproduction and cost limits

Two fresh captures match every nonmeasurement field: all eight associations/caps,
complete2-control/4-method/2-budget grids, original coordinates/scores, selected
identities/intro provenance, visited entries, counts and abstentions. Source layout
takes0.95–0.98ms; selection/counting0.52–0.54ms; total validation/loading/control
23.84–24.21ms after imports. These are descriptive costs for six records, not
production latency, statistical distributions or complete-ingestion overhead.

There are zero **new** inference calls; the prerequisite ranking experiment made81
calls per capture. Reusing those rankings does not establish that production can
rank passages with no embedding work. There is no ingestion or memory gate result.

152 lab tests pass. New tests cover Unicode children, table gaps, emphasized colon
intros, caps, code/prose stops, duplicate/self/cross-record links, equal budgets,
unchanged selections under label changes, query abstention, exact baseline counts,
JSON coordinate/association round trips and repeat mutations. This is Python fixture
verification, not Rust parity CI or a selected topic-tree implementation.

Run `section_experiment.py --baseline-first ... --baseline-second ... --contexts ...
--manifest ... --annotations ... --output ...`; repeat with fresh output plus
`--repeat FIRST.json --summary SUMMARY.json`. A repeat regenerates controls and
requires all nonmeasurement fields to match the first capture.

Private root `topic-python-review-v3-20261001`:
`section-controls-{first,second}.json`, SHA256
`d4e81d835544a89f8cdff3ffdb611c5426ea40aa3d825072818230c2e78e6fe4`,
`a008985f964f6d86b4c5eee96b2d754291ebc29f9d08f80396ce893a348bbebc`;
`section-controls-summary.json` retains own/common support totals and provenance.
Source histories/judgments remain outside Git.
