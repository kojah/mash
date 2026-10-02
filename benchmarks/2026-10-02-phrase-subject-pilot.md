# Phrase-centered subjects: stable evidence, unresolved topic usefulness

## Frozen procedure

Use the same exposed 128-thread cohort: all 1,850 admitted user-message
occurrences with 7,038 explicit empty projections kept in the source ledger.
Database SHA256 `251795191c9df51352f4a703faa2928017c99196596cd6ec16ec83803a40d463`;
selection SHA256 `270fdf50405136931791905b8555f5e4d7a35cb5a5fd935c4a201fd72b618167`.
The prior flat baseline SHA256 is
`9b0abe50883562377e7871d640473cfdd888fb2f407574ddca9f451c3615cb68`.
No source reselection, inference, download, LLM or paid call occurs. This is
exposed development data, not held-out semantic validation.

`topic_phrases.py` extracts the existing technical unigrams and contiguous
whitespace bigrams without bridging stopwords. It is not an entity recognizer,
POS model or synonym merger. Each thread pools exact-duplicate input text once,
retaining every original source identity for evidence. Vocabulary is capped at
8,192 by descending raw deduplicated frequency then lexical tie, finally sorted.
Thread TF is `1+log(pooled_count)`, with class length normalization,
`log1p(mean_class_length/global_class_frequency)` and thread IDF
`log((1+thread_count)/(1+thread_DF))+1`. Terms need an alphabetic character;
DF above .4 is excluded after three threads (explicit small-corpus warmup).
Select at most eight positive terms per thread, descending score/lexical ties,
pruning atom-contained phrases as in the prior label path. This selection cap
is new; fewer subjects cannot be attributed solely to better discovery.

A phrase's ID hashes the explicit namespace plus literal normalized phrase.
Its recurring status requires two corpus threads AND two distinct full inputs;
otherwise it is provisional. Corpus mention support and selected-thread support
are separate. Tag a source only when it literally contains one of its thread's
selected phrases. All original sources remain in memberships, including empty
tag lists. Scores are thread phrase weights, not cosine or probabilities.
These are literal subject facets, not proven semantic topic clusters.

`topic_phrase_store.py` is a bounded typed source ledger: 2,048 sources, 8 MiB
text and 256 KiB/source, atomic validation before replacement, provider/thread
scope, idempotent updates, deletion and strict checkpoint/restart. Snapshot
refresh recomputes corpus statistics; online update latency does not imply an
online fixed IDF frame or a selected publication policy. IDs stay phrase-stable,
but ranks/status/selection can change as the corpus changes.

## Results and lifecycle

The pilot discovers 618 subjects: 309 corpus-recurring and 309 provisional.
All 128 threads have tags; 1,007 thread tags total (7.87/thread). Only 111
subjects are selected by multiple threads; 507 are selected in one thread.
Of the 309 corpus-recurring subjects, 223 are selected in just one thread.
Thus reporting 309 as shared assigned topics would be misleading. Literal
evidence covers 574/1,850 sources (31.03%); 1,276 have no selected-phrase
evidence and 326 match multiple phrases. Corpus-recurring subjects have evidence
on 375 sources. The dense baseline has 870 topics and 11.52 tags/thread, with
different assignment rules and no comparable thread-tag cap.

Forward/reverse complete final semantics match exactly apart from arrival
revision/version counters. Two runs match all non-runtime fields. Checkpoint
and refreshed restart match exactly. Insert 128 duplicates (first admitted
source per selected thread, new identities) without changing phrase IDs,
thread ranks/scores, support or original-source memberships; evidence identities
expand intentionally. Remove those duplicates and recover initial semantics.
Delete an actual original source, verify absence from memberships/topics, restart
the deleted state, and reinsert the original source to recover semantics.
No upstream or native history is modified. Semantic replacement is a synthetic
fixture, not a real-source edit experiment.

An auditor imports no producer modules and reconstructs source token counts,
duplicate handling, vocabulary choice, complete term scores/rankings, containment
pruning, identities and source/thread/provider unions. It checks all 1,850 sources,
128 threads, 618 topics and 1,007 selected scores/ranks with zero score error.
It shares sklearn's static English stoplist through an independent scanner.
Lifecycle is checked by the producer and synthetic tests, not independently
replayed by this auditor. Native parsing, semantics and agent usefulness remain
unverified. Four new fixtures cover duplicate/provisional support, boundary/
namespace/multi-tag behavior, edit/delete/restart/atomic rejection and escaped
inspection. The full research suite passes 648 tests in 4.455s.

These results establish evidence correctness and deterministic final selection,
not better topics. The eight-term budget, literal matching, unresolved synonyms,
short contextual replies and provisional single-thread phrases remain limitations.
Do not adopt this as production topic modeling or compare its literal coverage
directly with embedding-based message assignment accuracy. Next assess source-
reviewed subject specificity and major/secondary subject coverage before further
selection changes or Rust promotion.

## Runtime, memory and artifacts

Initial source ingestion takes 149.21/124.90ms; first full subject refresh
4,105.61/2,833.67ms. Whole original/duplicate/cleanup/delete/restart/reinsert/
reverse experiment is 82.229/25.639s wall and 32.385/25.386s CPU after load,
before serialization/comparison. Large wall/CPU variation shows host scheduling
limits; these are not full production ingestion measurements or a throughput
win. Corpus-wide recomputation and no selected refresh policy remain open.
The <=10% complete-ingestion overhead target is untested.

External sampled target RSS peaks are 206,396/204,336 KiB; PSS
195,344/193,348 KiB. Approximate status HWM is 206,396/206,352 KiB;
pre-serialization rusage is 205,912/206,016 KiB, not final process peaks.
The target includes the prior flat JSON, source loading, repeated snapshots,
checkpoints/lifecycle and 1.60 MB result serialization plus repeat comparison.
Do not subtract counters or attribute this whole target to an isolated worker.
There are 1,947/1,061 samples, three/one vanished reads and maximum gaps
164.06/46.18ms. Samples are lower bounds, status approximate, monitoring
intrusive, observer/children excluded; launcher history can affect rusage.

Private result SHA256:

- First: `22bee439241a0797534f7ae0379e29a101abe253513785f6e9c31050c2f26efa`.
- Repeat: `2459806c2edce223a367dc42277d361af0192d191fec906f199dd330f3ad007b`.

Artifacts use `topic-phrase-{first,repeat}-20261002`, memory sidecars, logs and
`topic-phrase-audit-20261002.json` under mash-artifacts. The private escaped offline
`topic-phrase-inspection-20261002.html` links every subject/thread to original
admitted evidence and distinguishes corpus versus assigned support. No raw
sources, labels or vectors enter Git. Preparation of this view is not actual
independent review. No method, Rust port, MCP integration or default is selected.
