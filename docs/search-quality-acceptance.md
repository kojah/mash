# Gold search acceptance, round 1

This contract is fixed before inspecting held-out variant rankings. It governs
selection of gold build/ranking defaults, not ordinary search availability.
The existing synthetic real-model matrix remains a regression check; it is
neither an independent real-session benchmark nor evidence for a new default.
No relevance judgments have been collected for this round yet.

## Dataset and judgments

Freeze certified silver snapshots, binary/model hashes, active build/query TOML,
node/provider coverage and source revisions before capture. Use real sessions
from at least two providers and two nodes. Include short referential replies,
long mixed-topic threads, duplicates and identifier collisions. Provider files
must be unavailable during variant gold rebuilds; silver digests must remain
identical. Keep transcripts, query text, judgments and result captures outside
Git under the private artifact directory.

Prepare a development set and a held-out set with disjoint source sessions.
Related branches and duplicate copies belong in the same split. A reviewer must
write queries and expected evidence from source history before seeing rankings;
the evaluating agent must not manufacture those labels. Freeze the initial
judgments before variant capture. Blindly pool each variant's top ten results
and have the reviewer judge previously unseen results before scoring; do not
turn unjudged results into negatives or relabel known results to favor a run.
Retain both judgment versions and hashes. Once held-out results are examined,
those sessions cannot supply a fresh tuning/evaluation split.

Use 48 held-out queries per scope (local and combined): 10 exact identifiers or
errors, 10 semantic paraphrases, 8 referential questions, 8 known-thread scoped
questions, 8 metadata-filtered questions and 4 negative/absent queries. Scope
specific relevance sets may differ; local queries must target the local corpus.
Filtered questions must exercise provider, node, time and project/repository
predicates, including compound filters. Do not invent missing metadata. For a
referential question, freeze its resolved standalone query and context before
search; this round does not evaluate an automatic conversational resolver.

Grade logical threads by `(provider, conversation_id)`: 2 means directly
answers the question with usable evidence, 1 means useful partial/contextual
evidence and 0 means irrelevant. Equivalent node copies are one thread, not
additional recall. Separately judge the first supporting excerpt as useful or
not and record whether its source can be recovered with `view`. Thread metrics
alone do not establish passage quality or agent task completion.

## Bounded comparison and decision

Compare exactly three build configurations with the same model, query settings
and corpus: the current message/all-visible baseline, parent-and-message with
all-visible content, and message/user-messages. Each alternative changes one
build field. Use the current contextual-rerank query policy first, then the
existing fusion-only policy as one ranking ablation for those same captures.
There is no new algorithm grid, adaptive routing or ensemble in this round.
Topic exploration remains gated by its separate independent topic review;
ordinary search must work without topic artifacts.

For each scope, require judged-pool recall@10 >= 0.90 and overall nDCG@10 >= 0.80.
Require category MRR@10 >= 0.95 exact, 0.80 semantic, 0.65 referential, 0.85
thread-scoped and 0.85 filtered. Require >= 0.90 useful supporting excerpts and
all sampled evidence recoverable by source identity. All four negative queries
must abstain. No filter violation, canonical-data change, shared fallback,
incomplete context or silent missing-source publication is acceptable.

A replacement must improve nDCG@10 by >= 0.03 or useful-excerpt rate by >= 0.05
over baseline in a paired comparison, without lowering any category MRR@10 by
more than 0.02. Report per-query differences, not just pooled averages, and
inspect failures before acceptance. These are explicit product targets, not
claims of statistical significance from a small sample.

Use three complete build trials and ten query trials per query/configuration on
each available supported node class. Report initial rebuild, edits/deletes,
storage, CPU, peak process RSS, resident/direct latency and p95 separately;
declare filesystem/model-cache conditions. Budget each variant at <= 2x baseline
full build wall time, <= 2x gold bytes and peak RSS, and <= 1.25x baseline p95
query time per scope/mode. This allows slower indexing for quality, while
keeping query cost controlled. Missing node measurements leave cost acceptance
open; do not substitute clustering-only or small-fixture timings.

Stop after the declared variants and one completed independent review. Keep the
baseline if no alternative meets every gate; if the baseline misses an absolute
target, retain it operationally but leave quality acceptance open and file the
specific failures. Choose the simplest qualifying configuration. No tuning on
held-out results, extra sweeps or automatic new experiments. Further algorithm
work requires a separately declared evaluation round.

## Offline scoring

`tools/search_evaluation.py` consumes private TOML judgments and saved JSON CLI
or MCP search bodies; it performs no searches or inference. Request threads,
limit 10 and diagnostics. Store captures as
`{"version":1,"queries":[{"id":"q1","response":<search body>}]}`.
Judgments have `version = 1`, then entries such as:

```toml
[[queries]]
id = "q1"
text = "the frozen standalone question"
category = "semantic"
split = "held-out"
intent = "related-discussion"
scope = "combined"

[[queries.judgments]]
provider = "codex"
conversation_id = "canonical-thread-id"
grade = 2
```

Include grades for every pooled returned thread, including zeroes. Use the
categories above, `development`/`held-out` splits and `local`/`shared`/`combined`
scopes. Query IDs must identify distinct scope/filter cases; preserve exact
commands or MCP parameters alongside captures to audit filter values. The
scorer verifies query text, intent, scope, result granularity, source completion
and degradation flags. It cannot authenticate capture provenance, independently
verify filters or certify human independence.

```bash
python3 tools/search_evaluation.py \
  --judgments /path/outside/repo/judgments.toml \
  --captures /path/outside/repo/captures.json \
  --output /path/outside/repo/scores.json
```

The report pins both inputs and the scorer by SHA256. Unknown returned threads
produce an incomplete report without aggregate scores and exit 2. Malformed,
duplicate or mismatched queries and degraded searches fail. The scorer reports
precision, judged-pool recall, MRR and graded nDCG at fixed cutoffs, with negative
abstention separate. It does not automatically approve a configuration: excerpt,
filter, lifecycle, reviewer/split, cost and sample-size gates require the other
recorded evidence. Empty/missing categories cannot satisfy this contract.
