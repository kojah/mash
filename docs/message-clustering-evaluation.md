# Within-thread message clustering: preregistered evaluation

Issue: mash-2we. Status: bounded comparison completed on 2026-10-05; reject the
tested pipeline for production promotion. No production algorithm selected.
This experiment groups related messages inside one thread. It does
not restore global topic modelling, topic trees, production tags or a new search
artifact. Python is the experimental reference; production remains unchanged.

## Question and output contract

Can existing message embeddings produce useful groups of messages concerning the
same concrete subject, including a subject that returns later in the thread?
Shared vocabulary, repository or broad intent alone is insufficient: discussion
of Rust compilation and Rust database indexing must not become one group merely
because both concern Rust performance. Questions, answers, decisions and relevant
follow-ups may belong together. A message discussing several subjects may have
several memberships; uncertain/noise messages may remain unassigned.

Output identifies source messages, memberships, similarity scores and an actual
representative message per group. Group numbers are run-local, not durable topic
identities. No LLM, rewriting, summarization, fine-tuning, PCA or new embedding
inference is used in the algorithm. Canonical text is preserved.

## Corpus and frozen references

A read-only inventory of Xenia's current SQLite store on 2026-10-05 finds 42,150
384-dimensional message vectors in 817 threads: Claude Code 13,214 vectors/528
threads; Codex 28,936/289. Roles are 34,482 assistant and 7,668 user messages.
Thread vector counts have median 5 and maximum 4,118; 304 threads have at least
eight vectors and ten exceed 1,000. This inventory proves availability, not topic
quality or coverage of other providers.

Freeze 24 real threads before inspecting candidate outputs: twelve development
and twelve held out, covering both represented providers, short/medium/long
threads, recurring subjects, mixed messages and short replies. Prefer at least
one independent >1,000-vector thread in each split. Supplement with explicitly
synthetic contract fixtures for cases missing from the sample; synthetic fixtures
do not count toward real-data quality. If independent long cases are unavailable,
record the gap and obtain another node's read-only snapshot rather than silently
claiming that stratum was tested.

Split by duplicate/ancestry group, not arbitrary message ID. Copies/forks sharing
at least half their unique substantive projections (and at least three such
projections), plus known common session ancestry, stay in one split. Preserve
message order and explicit parent boundaries. Publish content hashes, selection
rule, model/build generations and split counts; keep identities, histories,
vectors and detailed reports in the private artifact directory outside Git.
Do not mix embedding generations within a thread or sample only successful
embeddings while claiming coverage of all canonical messages.

The first corpus is frozen with SHA-256
`fd625ba86b8d44940062f52335380518164d40af5aa6dca73b2210013012ed16`.
Seed `mash-message-clustering-20261005-v1` selects four independent components per
provider/length stratum (8–24, 25–150, 151+ vectors), alternating development and
held-out assignment. SHA-256(seed + thread ID) orders candidates, with >1,000-vector
threads prioritized in long strata. All 817 source files were checked for lineage:
Codex session/fork/parent links and Claude UUID/parent/logical-parent/session links.
Copy grouping conservatively uses all nonempty projections, not just projections
later judged substantive. No component appears in both splits.

| Split | Threads | Vectors / searchable messages | Canonical messages | >1,000-vector threads |
| --- | ---: | ---: | ---: | ---: |
| Development | 12 | 11,081 / 11,081 | 76,574 | 4 |
| Held out | 12 | 4,705 / 4,705 | 39,441 | 3 |

Each split has two threads in every provider/length stratum. Canonical counts
include non-searchable provider/tool records; they are not evidence of missing
embeddings. The sample uses one existing embedding generation and the default
dialogue build: quantized all-MiniLM-L6-v2 on CPU, stored as symmetric signed-byte
vectors. Frozen references below preceded candidate output inspection.

Blind diagnostic references are now frozen with SHA-256
`8379d8ccf9ff4ef8df39472db0eb2e279108c0678b0eb8112c04e6329a4bf568`.
They contain 282 judgments: development 71 related / 57 unrelated / 14 ambiguous;
held-out 70 / 59 / 11. Tagged case counts are 15/13 returning-subject, 12/12
short-reply and 16/10 mixed-message pairs (development/held-out). References are
agent-authored; long source messages were reviewed as head/tail excerpts, not
independently annotated in full by humans. Treat resulting metrics as diagnostic,
with that limitation attached, rather than established human-ground-truth accuracy.
Exact repeated projection pairs are excluded; ambiguous scope is not forced.

Before any candidate outputs, fix a structural minimum-support proxy: at least
five whitespace-separated tokens and no whole-record injected header (`# AGENTS.md`,
`<skill>`, `[gas-city]`, `<hook_prompt`, `<task-notification>`). Vectors remain
unchanged. This proxy controls minimum group support and parent eligibility; it
does not read reference labels or infer topics. Unreviewed qualifying projections
are potentially substantive, not verified substantive. Report this assumption;
only reviewed substantive messages enter measured coverage. Qualitative review
must still verify the substantive support/coherence of retained groups.

Before algorithm output is visible, freeze up to forty pair judgments per thread:
same concrete subject, different subject, or ambiguous. Include distant returning
subjects, nearby subject changes, question/answer pairs and referential replies.
Record whether each message is substantive, referential, boilerplate or uncertain.
Balance clear positive and negative pairs; do not infer negatives from missing
judgments. Synthetic labels are authored fixtures; real labels reviewed by the
coding agent must be identified as agent-authored, not independent human ground
truth. Report that limitation and retain examples for subsequent human review.

## Bounded candidates and tuning

Baselines are one group for the whole thread, individual-message singletons and
fixed chronological windows of eight messages. A whole-thread mean vector is a
representation baseline for later work, not a substitute for message grouping.

Compare only these initial methods:

1. Online cosine leader/centroid clustering in source order: assign to the closest
   qualifying normalized centroid, otherwise create a new group.
2. BIRCH on normalized embeddings, with branching factor 50, no final global
   clustering (`n_clusters=None`), and one-message `partial_fit` in source order.
   Use final leaf centres to assign the current thread. Its tree is internal;
   results are flat. Edits/deletions rebuild only the affected thread.
3. Average-link agglomeration with cosine distance as a batch quality reference.
   This is not a streaming algorithm; measure affected-thread rebuilds separately.
4. Offline HDBSCAN with cosine distance, brute-force distances, one job,
   `min_samples=2`, leaf selection and no forced single cluster. Its noise labels
   remain unassigned in the primary partition, never one shared noise group.

Leader and average-link use cosine thresholds 0.35, 0.50 and 0.65. BIRCH uses
radius/membership-cosine pairs (0.40, 0.65), (0.50, 0.50), (0.60, 0.35).
These are predefined configurations, not a claimed radius-to-cosine conversion.
HDBSCAN uses minimum-cluster-size/membership-cosine pairs (2, 0.65), (3, 0.50),
(5, 0.35). The user broadened the scope to offline methods before any candidate
quality outputs: the budget is now twelve first-round settings and at most twelve
second-round settings, with no additional families. The twelve initial settings
run on development data only. Use signed-byte stored vectors decoded exactly as
Rust, verify dimensions/norms and normalize without changing the embedding basis.
Collapse exact repeated prepared text within a thread for fitting; retain the
mapping back to every original message and prevent duplicate pairs inflating
metrics. Report missing, zero and invalid vectors separately.

Round one uses individual message vectors. One second development round is
allowed, with at most twelve additional configurations: either refine thresholds
around the strongest method, or test a fixed context view (0.7 normalized anchor
+ 0.3 normalized eligible parent, renormalized). Choose the round-two change
from recorded development errors before running it; do not combine both changes.
An explicit root or missing parent never borrows unrelated previous text.

Round one development has completed. Best overlapping F1 is 0.579 (leader at
0.35), versus 0.722 for the eight-message window baseline; the best leader finds
3/12 short-reply links and misses 7/15 returning-subject links. Fix round two to
the already specified 0.7 anchor / 0.3 eligible-parent view for the same twelve
configurations. Do not also refine thresholds. This decision precedes round-two
output and any held-out scoring. Eligibility uses the structural proxy, never
reference message kinds. NULL links use the immediately preceding visible source
message only when eligible; explicit roots abstain, and explicit missing parents
do not fall back. Baseline window boundaries always use raw-projection deduplication,
independent of the context view.

Apply the same membership policy to each method: groups need two distinct
substantive source projections to qualify. Allow secondary memberships when
centroid similarity meets the configuration's cosine threshold and is within 0.05 of the best
qualifying similarity. These memberships do not update centroids. Unassigned
messages are not one shared noise cluster. Ties use source ordinal then message
ID. Evaluate both primary partitions and final overlapping memberships so soft
assignment cannot conceal a bad partition.

After these at most twenty-four configurations, freeze one setting per method and
evaluate the held-out split once. Do not tune on held-out failures. No ensemble,
new model or extra clustering family is introduced within this experiment.

## Acceptance and decision rule

Clear judged pairs are predicted related if they share a retained group. Compute
per-thread precision, recall and F1, then macro averages; also report pooled counts
and provider/length/case strata. Ambiguous and unjudged pairs are excluded from
quality denominators and reported, not treated as negatives. A degenerate empty
prediction has zero recall and cannot pass. Coverage counts substantive evaluated
messages assigned to retained groups, not singleton/noise placeholders.

Choose a method only if the held-out results meet all these gates:

- Macro related-pair precision >= 0.85, recall >= 0.70 and F1 >= 0.75.
- Substantive-message coverage >= 0.80; macro F1 improves at least 0.10 over the
  best degenerate/window baseline. Report each baseline separately.
- Precision >= 0.75 in the returning-subject, short-reply and mixed-message
  strata; no representation/provider stratum loses >0.10 F1 versus the best
  baseline. Insufficient judged cases are an evidence gap, not a pass.
- Representatives and farthest members expose coherent subjects in at least
  90% of reviewed retained groups. Count and record failures, including broad
  generic groups, unrelated context borrowing and boilerplate hubs.
- On Xenia, warm p95 grouping <= 2 seconds at 1,000 messages and <= 15 seconds
  at 5,000, with peak process RSS <= 512 MiB. Record imports separately and bound
  numerical-library thread counts. Use real long threads plus clearly marked
  scaling fixtures; do not claim synthetic timing is real-data quality evidence.
- Repeated identical input produces identical memberships/representatives.
  Append, edit, deletion and restart/rebuild checks preserve source identities,
  valid parents and deterministic results for a given source order. Measure
  incremental versus affected-thread rebuild cost; whole-corpus rebuilding is
  prohibited. Local batch recomputation after debounce is an acceptable online
  maintenance strategy if it meets the latency gates.

Online insertion is not a requirement for the winning algorithm. Following the
user's scope change, offline recomputation on request is eligible: cache by thread
revision, embedding generation and algorithm/configuration version; reuse results
for unchanged threads and rebuild only a requested changed thread. These cache
semantics describe the candidate maintenance strategy, not an implemented feature.
Measure unchanged-cache lookup separately from changed-thread computation; judge
the latter against the same latency gates. Do not hide rebuild time in ingestion.

Among passing methods, maximize held-out macro F1. Within 0.02 F1, prefer the
simpler method with cheaper updates and lower memory. Report sensitivity around
the selected development threshold and all failures, not just aggregate success.

If none passes after the two rounds, reject the tested pipeline and identify the
limiting representation/context/data issue with evidence. Stop this search;
propose a separately scoped experiment rather than adding exceptions or moving
thresholds indefinitely. A negative decision is an experimental outcome, not a
claim that Mash has a production-ready clustering feature. Similar-thread search
and thread-vector evaluation remain follow-ons, not gates for this decision.

## Research basis and limits

[Hearst (1997), TextTiling](https://aclanthology.org/J97-1003/) motivates evaluating
local coherence and subject boundaries, but contiguous segmentation alone cannot
recover a subject revisited elsewhere in a thread. This experiment explicitly
tests those returning subjects.

[Xing and Carenini (2021)](https://aclanthology.org/2021.sigdial-1.18/) evaluate
utterance-pair coherence for dialogue segmentation using a trained BERT scorer.
That supports testing conversational context; their learned scorer and reported
accuracy are not implementations or performance claims for our embedding-only
experiment.

[Scikit-learn's agglomerative clustering contract](https://scikit-learn.org/stable/modules/generated/sklearn.cluster.AgglomerativeClustering.html)
provides cosine average linkage and distance-threshold partitioning.
The prototype will use those implementations rather than invent a clustering
library. Their hierarchy is an internal computation; our output remains flat.

[Scikit-learn's BIRCH contract](https://scikit-learn.org/stable/modules/generated/sklearn.cluster.Birch.html)
provides incremental CF-tree updates with `partial_fit`. Its merge threshold
bounds subcluster radius, not cosine similarity; `n_clusters=None` avoids a final
batch agglomeration. Incremental insertion does not imply native deletion support
or stable cluster IDs, so both are evaluated explicitly.

[Scikit-learn's HDBSCAN contract](https://scikit-learn.org/stable/modules/generated/sklearn.cluster.HDBSCAN.html)
supports flat density clusters and explicit noise labels. Cosine uses brute-force
distances; this experiment measures memory and rebuild time rather than assuming
that an offline method will be fast enough. The internal hierarchy is not exposed.

[Monath et al. (2019), GRINCH](https://arxiv.org/abs/2001.00076) describes
incremental hierarchical clustering with rotations and grafting that revise early
decisions. It could supply a flat tree cut followed by our membership rule; a
product hierarchy is not required. It is not implemented or measured here.
Revisit it in a bounded follow-up if simpler online methods fail quality and
offline rebuilds fail latency. Its paper's benchmark results are not evidence of
performance or accuracy on Mash sessions.

## Completed comparison and decision

The two development rounds used exactly twenty-four configurations: the twelve
registered settings on raw projections, then the same twelve on the fixed
0.7-message / 0.3-eligible-parent normalized context view. No reference labels
affect preparation or clustering. Best development macro F1 by family changed
from 0.579 to 0.717 (leader), 0.423 to 0.588 (BIRCH), 0.578 to 0.741
(average-link), and 0.497 to 0.560 (HDBSCAN). Context is promising, but these
development improvements do not establish held-out acceptance.

Freeze one setting per family by development F1 before scoring held-out data:
leader cosine 0.35; BIRCH radius 0.60 / cosine 0.35; average-link cosine 0.35;
HDBSCAN minimum size 3 / cosine 0.50. All selected the context view.
Frozen-settings SHA-256:
`fad6a71e3a34d8b788c81294c90869dc0eade38de652ffb01cec9e3f9bafdbb0`.
Held-out data were scored once; no settings were changed afterward.

| Held-out method | Macro precision | Macro recall | Macro F1 | Reviewed substantive coverage | Primary-only F1 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Leader + context | 0.719 | 0.842 | 0.736 | 0.945 | 0.672 |
| BIRCH + context | 0.842 | 0.749 | 0.763 | 0.963 | 0.673 |
| Average-link + context | 0.839 | 0.815 | 0.774 | 0.963 | 0.731 |
| HDBSCAN + context | 0.687 | 0.573 | 0.618 | 0.862 | 0.531 |
| Whole thread | 0.543 | 1.000 | 0.698 | 1.000 | 0.698 |
| Singletons | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| Chronological windows of eight | 0.839 | 0.741 | 0.769 | 0.991 | 0.769 |

Scores are per-thread macro averages; F1 is not computed from macro precision
and recall. Every method fails the precision gate, and the strongest method's
0.005 F1 improvement over windows falls well short of the required 0.10.
Provider F1 for average-link is 0.786 Claude / 0.762 Codex, versus windows'
0.703 / 0.835. HDBSCAN's 0.376 Claude / 0.859 Codex exposes a large provider
gap. These are twelve held-out threads, not population estimates.

Average-link recovers 11/13 positive returning-subject pairs versus windows'
3/13; short-reply recall is 10/12 for both, and mixed-message recall is 7/10
versus 3/10. All held-out case-tagged pairs are positives. Their apparent perfect
precision has no negative controls and **cannot pass the case precision gates**.
Overall negative judgments do exist, but this case-specific evidence gap remains.

A fixed-hash selection reviewed up to three retained average-link groups from
each of four held-out threads: eleven groups total, representative plus the two
farthest primary members. Nine were coherent (81.8%, below 90%). Failures mixed
unrelated Git work under generic progress language, and mixed a stopped-session
investigation with codebase-index health under generic session vocabulary.
This small agent-authored audit is diagnostic; it does not verify every group.
It supports prioritizing subject/context representation, rather than claiming
that switching clustering families alone will fix the problem.

One reporting correction was made without rerunning or retuning candidates:
chronological baselines now always deduplicate raw projections independently of
the context view. The round-two development artifact used context-dependent
deduplication for its window baseline (0.721); use round one's raw baseline
(0.722) for the development comparison. The held-out baseline already uses
the corrected raw boundaries. Algorithm scores were unaffected.

## Measured cost and update behavior

Each method ran in its own Python process on Xenia, with numerical thread counts
bounded to one. Ten warm rebuilds estimate the sample p95; three controlled
append/edit/delete rebuilds measure update cost. Source preparation, inference,
transport, cache lookup and cold imports are excluded from grouping time.
Imports cost 0.84–1.15 seconds separately. Peak RSS includes the interpreter,
libraries, fixture and prepared update variants.

The actual development thread contains 3,775 prepared context units. The 1,000
case is its prefix. The 5,000 case cycles its vectors with deterministic tiny
jitter; it is a synthetic scaling fixture, not real-data quality evidence.

| Method | 1,000 p95 / peak MiB | Real 3,775 p95 / peak MiB | Synthetic 5,000 p95 / peak MiB |
| --- | ---: | ---: | ---: |
| Leader | 0.065 s / 163 | 0.292 s / 220 | 0.389 s / 228 |
| BIRCH | 0.192 s / 171 | 1.650 s / 255 | 2.712 s / 288 |
| Average-link | 0.077 s / 174 | 1.678 s / 327 | 2.476 s / 431 |
| HDBSCAN | 0.049 s / 187 | 2.525 s / 493 | 4.047 s / 700 |

All meet the grouping latency gates; HDBSCAN exceeds the 512 MiB memory gate
at 5,000. These small-sample Python measurements are not Rust production SLAs.
For the real thread, median append/edit/delete rebuild times respectively are
0.246/0.242/0.249 s (leader), 1.494/1.525/1.425 s (BIRCH),
1.351/1.325/1.328 s (average-link), and 2.662/2.147/2.301 s (HDBSCAN).
All repeated result hashes matched within each operation.

Single observed native insertions at 1,000/3,775 units cost 0.044/0.093 ms for
leader and 0.162/5.323 ms for BIRCH. They exclude reassignment and publication;
they are neither p95 estimates nor complete update costs. Native edit/deletion
support is not claimed. Contract tests cover source identity preservation,
edited-parent context propagation, deletion abstention and serialized restart.
Controlled timing changes prepared units; it does not measure provider ingestion.

Decision: **reject this tested pipeline for production promotion; stop this
algorithm search at its registered budget**. Context-aware average-link is the
best research candidate, and on-request affected-thread rebuilding is affordable
in this fixture. No cache or production clustering feature has been implemented.
GRINCH remains a possible follow-up, not a measured winner or an immediate need:
offline latency did not fail. A separate next study should improve dialogue units
and context representation, add case-specific negative controls and independent
review, and use fresh held-out data rather than tune on these failures.

## Reproduction evidence

Code and dependency pins live in `experiments/message_clustering/`. Private
artifacts live under `/home/james/scratch/mash-artifacts/message-clustering-20261005`
(directory 0700, data files 0600). Histories, vectors and judgments are not in Git.
The held-out command requires frozen settings matching the corpus/reference hashes.

| Artifact | SHA-256 |
| --- | --- |
| Development round 1 | `5a18feb4401c42c690d2a0807bee29658fb554f7d855f34d3d9a30315592bbfe` |
| Development round 2 | `d1f1e3de03a83c1aaab4d1b7d30c5f4872c7e32cb9ee9ec9765e555c4dbddd4e` |
| Held-out report | `8ebc10a38593be7640e971ebbb2afa5518fdcfa02103cb364847dd7406eca00c` |
| Timing fixture | `af3ef2fd428171d9977ffac25ab72eac59a9a42ed0fced3f2bae1bd804658966` |
| Group audit | `8ac69ab9ed6921608db9c6beae9bd84653cc75a87b598a9acef0d174be49f959` |
| Native insertion observations | `60dd2aabc06a58fa3e824abe61a4fed277ef447a5f814ccf53553edb5f0f767f` |

Limitations: only Claude Code/Codex, sparse agent-authored pair references,
head/tail review of long messages, unverified structural support proxy, few
audited groups, no case-specific negative controls, and one machine for timing.
These limitations prevent a broad accuracy claim and strengthen the decision
against production promotion.

Validation: all 21 experiment tests and all nine existing Python tests pass;
Ruff lint/format and ty pass for the experiment. An additional broad check of
unchanged `tools/` found existing lint/format failures and nine type diagnostics
in older benchmark scripts; those files were not changed. Rust checks and
deployment were not run because production code is unaffected.
