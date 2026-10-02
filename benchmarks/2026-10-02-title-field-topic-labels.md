# Title-field topic labels: better term diversity, incomplete naming quality

Bead `mash-c0d.19.2.30.4.6`. Original title evidence improves cross-topic
vocabulary diversity on every frozen configuration. Coherence is mixed and
question phrasing remains. This is a completed source-evidence control, not a
selected topic method or semantic-quality gate.

## Prospective procedure

Keep all 32,176 original candidates, the frozen 32,768-term merged vocabulary,
four [PCA25 membership snapshots](2026-10-02-fixed-pca-topic-routing.md), equal
document mass over retained tags, eight labels, and the original technical-token
and source-adjacent bigram rules. Compare merged counts against original
title-only counts. Background document IDF remains original merged presence;
class counts, lengths and class-based TF-IDF are recomputed for each evidence
source. This isolates a field change, not a new vocabulary or geometry.

Count title and body independently and require exact sparse conservation against
the archived merged counts. No query/reference features, new document-frequency
selection, stopwords, numeric/identifier patches, model calls or reserved scoring.
Preserve all zero-evidence documents and topic rows; empty topics explicitly
abstain. Record original selected-field spans, hashes and distinct source support.
Compare all existing c-TF-IDF, corpus-IDF and lexical-containment variants using
the same merged document-presence NPMI/reuse diagnostics. As discussed in the
[coherence study](2026-10-02-topic-label-coherence.md), these are proxies rather
than independent semantic judgments.

Title counts have 322,810 nonzeros and mass 326,578; body counts have 4,727,380
nonzeros and mass 5,904,993. Their sum exactly matches archived merged counts
(4,879,216 nonzeros, mass 6,231,571). Overlapping field terms explain why nonzero
counts do not add. Thirteen documents have no title term in the frozen vocabulary;
all remain included. No topic has zero evidence in any of the eight field cases.

## Outcomes

| Order / membership | Merged / title corpus-IDF NPMI | Merged / title unique-term ratio |
| --- | --- | --- |
| Forward / hard | .2717 / .2743 | .8625 / .9250 |
| Forward / overlap | .2809 / .2848 | .8688 / .9250 |
| Reverse / hard | .2649 / .2608 | .9205 / .9432 |
| Reverse / overlap | .2911 / .3105 | .9432 / .9886 |

Plain c-TF-IDF NPMI changes from .1580/.1715/.1380/.1635 to
.2101/.2134/.1885/.2339; containment NPMI changes from
.2426/.2494/.2405/.2449 to .2241/.2385/.2222/.2477. All three variants
increase unique-term ratio in all four settings. Neither coherence nor vocabulary
diversity alone establishes specificity, usefulness or correct topic boundaries.

Exploratory inspection of previously identified groups finds title corpus-IDF
labels such as `algorithm`, `inheritance`, `mvc`, `asp.net`, and `design pattern`
instead of some body code/numeric tokens. Forward agile retains `scrum, agile,
team, project, development, sprint` but adds `how to`; another group includes
`why`, `why is`, and `why do`. Testing still repeats `unit, testing, test, tests,
unit testing, unit test, unit tests, tdd`. These are assistant observations,
not human gold. No outcome-specific exclusion is applied to repair them.

Public question titles are deliberately authored summaries. Their availability
and quality cannot be assumed for provider-generated agent-thread titles. Body
or episode evidence remains necessary for titleless sessions and topic shifts.
Changing labels does not repair the poor routing/order behavior of these frozen
topic groups; no improved tree, tags or guided-search outcome is claimed.

## Verification and measured costs

Fresh repeats match every decoded nontiming field and all 30 captured sparse
array hashes. Independent original-field Counter extraction checks all 32,176
documents, conservation, zero-title identities, class aggregation, formulas,
rankings, selected-source membership/counts, Unicode/UTF-8 spans, hashes and
document-presence coherence. It verifies 2,976 label entries and 1,419 source
supports; maximum scalar score difference is 5.55e-17. Class aggregation uses
an independent driver with the same SciPy sparse multiplication, not a wholly
independent numerical backend. The suite runs 368 tests, including Unicode,
technical identifiers, field boundaries and zero-evidence topic abstention.

First/repeat after-import wall time is 22.138/18.279 s; CPU 20.792/18.041 s.
Field counting, conservation and array capture take 12.253/10.480 s; setup
14.422/12.178 s includes that work. Separate rusage peaks are 547,004/546,772 KiB.
External 814/734 samples show RSS 548,036/547,336 KiB and PSS 535,377/530,294 KiB;
approximate status VmHWM equals those RSS maxima. Requested sleep is 20 ms;
actual median/max gaps are 28.57/69.76 and 27.15/56.62 ms, with two/zero vanished
reads. Both monitored workloads exit zero. The repeat overlaps the independent
audit; these are not controlled speed comparisons. Target-only scope excludes
children/observer; sampling adds overhead and maxima are lower bounds. Retained
source/count matrices and imports contribute. These batch research costs do
not establish production memory, update latency or the ≤10% ingestion target.
No embedding inference, download, paid or generative call occurs.

## Reproduction and decision

Use `cqadup_field_labels.py --topics PCA_FIRST --topic-repeat PCA_SECOND
--source VERIFIED_CANDIDATE_ROOT --output-directory FRESH_DIRECTORY`, externally
wrapped by `memory_audit.py`. Repeat adds `--compare FIRST_DIRECTORY/results.json`.
Use the pinned Python/science environment documented in the PCA25 study.
Outside-Git artifacts under `/home/james/scratch/mash-artifacts`:

- First `topic-cqadup-field-labels-first-20261002/results.json`, SHA256
  `b42ba4e190a2c0e62dc8d924b28fa8ed79c1aba9bb98fefa7ec13dd546230f83`.
- Repeat `topic-cqadup-field-labels-second-20261002/results.json`, SHA256
  `4fe122692d2af5f00165507251d4ef738640191110c054f846d2eb1997362050`.
- Audit `topic-cqadup-field-labels-audit-first-20261002.json` (verified).
- Monitors `topic-cqadup-field-labels-memory-first-20261002.json` and
  `topic-cqadup-field-labels-memory-second-20261002.json`.

Source-field quality is a worthwhile design consideration, but title-only
labeling is not promoted. Independent label specificity, intrinsic topic quality,
semantic ancestry, provider coverage, lifecycle, complete ingestion and Rust
parity remain open. Avoid another threshold grid on these failed topic groups.
