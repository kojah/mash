# Source-unit allocation: better evidence coverage, more referential-only topics

## Matched procedure

This is a fixed development control, not a selected production topic method.
Reuse the original exposed eight-thread packet and familiar single-assistant
review, with oracle admission of 68/84 records and nine primary/nine secondary
subjects. Review names and support sets enter evaluation only, never discovery.
Separately reuse the frozen 128-thread, 1,850-source cohort with 7,038 explicit
empty projections excluded. No source reselection, inference, LLM, download or
paid call occurs. Both controls reproduce their prior snapshots exactly.

Keep the same tokenization, duplicate pooling, vocabulary, thread phrase
weights, DF cutoff, containment pruning and eight-tag cap. The alternative
first shortlists up to two diverse eligible terms from each distinct exact
source input, ranked by the existing **thread** weights. Duplicate identities
of identical text have one vote. Candidate support counts only units that
shortlisted the term, not every occurrence of that term. Greedily select from
the union, with utility half the original score divided by the initial maximum
candidate score and half newly covered candidate-voting units divided by the
current maximum gain. When all gains are zero, the coverage component is zero.
Exclude atom-contained choices; ties use original score, then lexical order.
Output scores remain the original phrase weights, never utility/probabilities.
Final evidence still includes all literal matches in the selected thread.

This is a lexical coverage heuristic, not semantic MMR. Exact-input votes are
not semantic deduplication. The candidate shortlist can restrict available
tags; the shared cap is a maximum, not a guarantee of eight tags. The rule was
fixed before the run; no outcome-driven weight, threshold or stoplist tuning.

## Results

| Diagnostic | Whole-thread control | Source-unit allocation |
| --- | ---: | ---: |
| Primary any-phrase evidence recall | .928571 | .986111 |
| Secondary any-phrase evidence recall | .481481 | .796296 |
| Primary best single-phrase support F1 | .734512 | .734512 |
| Secondary best single-phrase support F1 | .385714 | .559788 |
| Primary subject-name atom hits / 9 | 5 | 3 |
| Secondary subject-name atom hits / 9 | 3 | 5 |
| Secondary subjects without literal evidence / 9 | 4 | 1 |
| Reviewed topics supported only by referential records | 4 | 10 |
| Reviewed topics / selected thread tags | 63 / 63 | 48 / 48 |
| Broad topics / selected thread tags | 618 / 1,007 | 442 / 720 |
| Broad corpus-recurring / provisional topics | 309 / 309 | 291 / 151 |
| Broad sources without selected evidence / 1,850 | 1,276 | 769 |
| Broad sources matching multiple selected phrases | 326 | 413 |

All threads have tags in both arms. Broad literal coverage rises from 31.03%
to 58.43%, but that is **not semantic recall**. Any unrelated selected phrase
on a subject's multi-topic support record counts toward evidence recall.
Best support F1 and atom overlap are proxies, not topic precision or synonyms.
The review is exposed, nonindependent and oracle-admitted. The increase from
four to ten referential-only topics signals a quality risk, not ten proven bad
topics: the diagnostic inspects record kinds after fitting, and contextual
references can be meaningful. Higher coverage does not establish better labels;
primary name overlap actually declines. No promotion is justified.

Next test source-grounded content candidate admission against these frozen
controls, preserving brief technical subjects and explicit abstention rather
than borrowing unrelated context. Evaluate label specificity, primary/secondary
membership precision and useful thread browsing; retain independent review,
full-ingestion budget and selected Rust parity as open gates.

## Verification

Two runs match every non-runtime field. The alternative passes refreshed
checkpoint/restart, one duplicate input per thread (8/128), invariant original
tags/scores/support after expanded evidence, duplicate cleanup, actual source
deletion/absence, deleted restart, reinsertion and reverse final semantics.
The producer checks lifecycle; the auditor does not independently replay it.

A no-producer-import audit rebuilds original broad records directly from
immutable SQLite and the pinned selection. It checks original review admission,
hashes and support sets; independently recomputes counts, vocabulary, scalar
weights, per-unit shortlists, coverage votes, greedy gains/utilities/ties,
containment, identities and literal thread/source unions for all four snapshots.
It checks 68/1,850 sources per scope, 48/63/442/618 topics and 48/63/720/1,007
selected ranks/scores. Maximum score error is zero; support F1 error is
1.11e-16. Shared inputs/static English stoplist remain dependencies. Native
parsing, independent human semantics and lifecycle replay are outside the audit.
An initial auditor invocation failed because a phrase result was incorrectly
treated as a source-ledger checkpoint; its failure log is preserved. The
corrected audit reads SQLite directly. Two allocation fixtures and a corruption
fixture cover short-unit candidates, duplicate votes, explicit policy and
independent rejection of altered scores or evidence.
An additional regression rejects whole-file hashing allocations. The full
655-test research suite passes in 4.196s.

## Cost and reproducibility

Reviewed refresh plus evaluation takes 15.58/14.70ms for control and
12.31/16.64ms for allocation. Broad refresh takes 2,877.31/2,732.31ms for control
and 3,045.16/3,241.76ms for allocation. Whole experiment after reviewed loading,
including broad loading and both scopes' duplicate/delete/restart/reverse
checks, takes 31.010/66.510s wall and 30.937/35.070s CPU. It excludes imports,
initial reviewed loading, output serialization and repeat comparison; it is
not complete ingestion or an isolated topic worker. Host scheduling and
external observation limit timing interpretation.

Sampled target RSS and approximate status HWM are 1,250,648/1,249,924 KiB;
PSS is 1,240,554/1,239,861 KiB. Pre-serialization rusage is
1,250,392/1,249,172 KiB. These whole-target values are substantially larger
than the earlier phrase-only pilot and require investigation before any
production claim. Do not subtract counters or attribute them to the selector.
There are 1,282/1,853 samples, zero vanished reads and maximum gaps
46.18/141.46ms. RSS/PSS sample peaks are lower bounds, status counters
approximate, observer intrusive, children/observer excluded and launcher
history can affect rusage. The <=10% full-ingestion budget is untested.

Inspection identifies `digest(path.read_bytes())` loading the entire
1,078,059,008-byte SQLite file for its integrity hash. Replace it with streaming
`hashlib.file_digest`; the auditor streams its large-file hashes too. Two fresh
`phrase-allocation-stream-{first,repeat}` runs match all non-runtime fields,
and match every original scientific field exactly with only the declared digest
helper code pin changed. A second independent audit checks these captures.
The original captures/failure logs remain intact; no timing result is rewritten.

After this fix sampled target RSS/status HWM is 186,756/186,424 KiB (about
182 MiB), PSS 176,625/176,395 KiB and pre-serialization rusage
186,344/186,032 KiB. There are 1,242/1,230 samples, one/zero vanished reads and
maximum gaps 42.95/44.79ms. Scope/measurement limitations above still apply;
this is a controlled removal of a whole-file allocation, not worker attribution.
Broad control refresh is 2,955.36/2,694.13ms and allocation refresh
3,112.56/2,753.64ms. Reviewed control is 18.64/20.33ms and allocation
19.23/16.05ms. Entire matched lifecycle experiment is 30.892/30.081s wall,
30.745/29.743s CPU. No full-ingestion gate or general speedup is established.

Private result SHA256:

- First: `c9ecb88797cfe82b6c7fab7d24794d1c2b61b25919feeb18a78caecac705e547`.
- Repeat: `f0d7d7e115092d8158869ef35a8015a4ac9537fda93959756aebfa4518a8e0c8`.
- Streamed first: `e1e425b2d7f635df16e07f8f840e44f3da4f108899d4de233095325901a246c5`.
- Streamed repeat: `4332c2531271e47fe6f73550660712aacb535547edbfae3b7585b8c358d7939c`.

Artifacts: `phrase-allocation-{first,repeat}-20261002.json`, matching memory
sidecars/logs, `phrase-allocation-audit-verified-20261002.log`, the two streamed
captures and `phrase-allocation-stream-audit-20261002.log`. Input/code
hashes are embedded in the results; packet/review/database/selection identities
are unchanged from the linked prior reports. Raw sources, labels and credentials
remain outside Git. No Rust port, MCP integration or production method selected.
