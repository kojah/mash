# Distinct subject evidence and redundant local tag support

Diagnostic-only evaluation of four frozen phrase controls. Original discovery,
source selection, labels, memberships and captures remain unchanged. No new
annotations, inference, download, generative LLM or paid call occurs.

## Procedure

`distinct_support.py` verifies capture/packet/review hashes and loads the existing
eight-thread review: 68 oracle-admitted records, nine primary/nine secondary
subjects. Uncertain subjects remain excluded. Corpus-familiar single-assistant
annotations are exposed development data, not independent human gold; no review
input enters discovery.

For each subject/tag within its original thread, support F1 is the exact rational
`2 * |expected ∩ local_tag_sources| / (|expected| + |local_tag_sources|)`.
A partial one-to-one assignment maximizes total F1, then positive pair count,
then lexicographic path. Subjects follow original ordinals; tags follow sorted
canonical IDs; unmatched sorts last. Zero-overlap edges remain unmatched.
Primary and secondary subjects have equal weight in the joint objective; their
reported means are not optimized separately. Subset DP accepts at most 16 tags
and fails beyond that bound; these controls have at most eight. This is optimistic
evidence separability, not matching based on label meanings.

For each thread in the separate 128-thread/1850-record cohort, count distinct
local support signatures and all unordered tag-pair Jaccards. A signature is the
exact set of supporting source IDs **inside this thread**; other-thread evidence
is excluded. Identical signatures suggest potential redundancy, not synonymy:
one message may contain multiple legitimate subjects. Do not automatically merge
or discard tags from this diagnostic.

## Results

| Control | Primary independent / distinct F1 | Secondary independent / distinct F1 | Secondary unmatched | Threads with reuse gap |
| --- | --- | --- | --- | --- |
| Whole-thread weight | .734512 / .734512 | .385714 / .385714 | 4/9 | 0/8 |
| Source coverage | .734512 / .734512 | .559788 / .515344 | 2/9 | 1/8 |
| Candidate phatic exclusion | .734512 / .725253 | .559788 / .528042 | 1/9 | 1/8 |
| English background weighting | .720370 / .720370 | .589418 / .544974 | 2/9 | 1/8 |

Source coverage's secondary gain survives the stricter constraint: .385714 to
.515344, smaller than the independently optimized .559788. Contention occurs in
one thread per coverage arm. Background weighting's higher distinct secondary
mean accompanies poorer primary support and previously recorded risks; this does
not reverse the mixed-outcome decision. Small dependent samples establish no
generalizable quality or statistical promotion.

| Control | Tags | Distinct local signatures | Identical pairs / all pairs | Threads with identical-support tags |
| --- | ---: | ---: | ---: | ---: |
| Whole-thread weight | 1007 | 331 | 2192 / 3496 | 125/128 |
| Source coverage | 720 | 545 | 175 / 1899 | 98/128 |
| Candidate phatic exclusion | 718 | 544 | 174 / 1890 | 98/128 |
| English background weighting | 725 | 537 | 188 / 1927 | 98/128 |

Evidence diversification is substantial. Remaining identical signatures and
weak labels justify testing whether multiple phrases represent one useful
subject while preserving genuine multi-topic messages. More stopword/scalar-weight
tuning alone is not justified. Distinct support, useful names and navigation
remain different goals; no topic method is selected.

## Validation and cost

Two sequential runs in the fresh pinned CPython3.11.15 science environment, one
OMP/OpenBLAS/MKL worker, reproduce all non-runtime fields exactly. Diagnostic
evaluation is 50.970/47.850ms after imports/loading and before serialization;
final input/code hash rechecks are included. Discovery, ingestion, inference and
startup are excluded; this is not a throughput comparison or ingestion budget.

`audit_distinct_support.py` imports no producer. It reconstructs admitted IDs and
support from original packet/review, verifies admitted source text hashes,
enumerates all 1152 feasible positive-edge assignments across four controls,
checks 72 subject choices/rational F1s and 9847 local support pairs, and verifies
complete repeat. Choices/F1 rationals match exactly; floating display-summary
error is at most 1.11e-16. The initial bitwise float-mean audit failed because
host/reference interpreter summation differs in rounding; its log is preserved.
The corrected audit uses rational accumulation and a 1e-15 bound only for display
means. Broad memberships come from the previously source-audited capture; this
audit does not re-ingest SQLite, evaluate semantics or replay edits.

Five standard-library fixtures cover 150 seeded exhaustive-oracle matrices,
ties, empty/zero/invalid inputs, scope isolation, identical support, reused-tag
credit and corrupt redundancy. CI runs them. All 666 science tests pass in 4.903s
with author-reference paths. The living manuscript builds successfully.

Private artifacts under `/home/james/scratch/mash-artifacts`:

- `distinct-support-{first,repeat}-20261002.json` (0600).
- `distinct-support-audit-20261002.log` (initial mean rejection).
- `distinct-support-audit-corrected-20261002.log` (successful audit).
- `distinct-support-tests-20261002.log` (suite).
- `distinct-support-final-tests-20261002.log` (final 666-test suite).
- `distinct-support-paper-build-20261002.log` (manuscript build).

Capture SHA256: `8f392fdbe90c3debcc4abe17ebf99611b4c1901438e13e89aa18158be0d87507`.
First SHA256: `a4b804c5a6508864d72f0b416577dbb1537f2e94db1b60715cb29f175ff09197`.
Repeat SHA256: `3aaa37966f8e50ba71097719519eef88470388bf07ff1a00c6eccbf5fc2b3841`.
Original packet/review identities and diagnostic producer hashes are in outputs.
No production method, Rust parity, independent semantic quality or full-ingestion
performance gate is established.
