# Within-thread message clustering: preregistered evaluation

Issue: mash-2we. Status: protocol fixed; no algorithm selected or quality result
claimed yet. This experiment groups related messages inside one thread. It does
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

Leader and average-link use cosine thresholds 0.35, 0.50 and 0.65. BIRCH uses
radius/membership-cosine pairs (0.40, 0.65), (0.50, 0.50), (0.60, 0.35).
These are predefined configurations, not a claimed radius-to-cosine conversion.
This shortlist amendment precedes any candidate output or quality measurement.
The nine settings run on development data only. Use signed-byte stored vectors decoded exactly as
Rust, verify dimensions/norms and normalize without changing the embedding basis.
Collapse exact repeated prepared text within a thread for fitting; retain the
mapping back to every original message and prevent duplicate pairs inflating
metrics. Report missing, zero and invalid vectors separately.

Round one uses individual message vectors. One second development round is
allowed, with at most nine additional configurations: either refine thresholds
around the strongest method, or test a fixed context view (0.7 normalized anchor
+ 0.3 normalized eligible parent, renormalized). Choose the round-two change
from recorded development errors before running it; do not combine both changes.
An explicit root or missing parent never borrows unrelated previous text.

Apply the same membership policy to each method: groups need two distinct
substantive source projections to qualify. Allow secondary memberships when
centroid similarity meets the configuration's cosine threshold and is within 0.05 of the best
qualifying similarity. These memberships do not update centroids. Unassigned
messages are not one shared noise cluster. Ties use source ordinal then message
ID. Evaluate both primary partitions and final overlapping memberships so soft
assignment cannot conceal a bad partition.

After these at most eighteen configurations, freeze one setting per method and
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
