# Support-aware topic label experiment

Topic labels already use c-TF-IDF. The remaining problem is that rare procedural
phrases crowd out concrete subjects, and semantic ranking can retain greetings.
This experiment combines distinctiveness, indexed-thread support, semantic
relevance and complete conversational-filler filtering. It remains an offline
prototype, not a production topic API or default policy.

## Candidate and evidence boundaries

`MASH_TEST_TOPIC_SUPPORTED_LABELS=1` selects the typed `Supported` policy.
Existing TF-IDF and balanced controls remain reproducible. Both profiles use
the previously validated CPU singleton label inference profile, so vocabulary
and cache-order changes do not change an identical phrase's embedding.

Each candidate carries its original c-TF-IDF and a separate integer support
count. The assignment pipeline already unions supporting segment words before
counting each indexed thread once per topic; ancestor labels similarly union
child evidence before counting each thread once. The new policy consumes these
counts rather than inventing extra votes from repeated messages. Counts are
indexed-thread support, not a general guarantee of independent canonical
conversations across mirrored histories.

The shortlist score is `0.3 × relative c-TF-IDF + 0.7 × relative support`, with
each component normalized by the maximum eligible candidate in its topic.
Original c-TF-IDF values are retained for later diagnostics. Complete phrases
recognized by the existing conversational-continuation parser are excluded
only from the label shortlist. Source words, document embeddings, memberships,
retained source evidence and the topic tree are preserved. Unknown words keep
named subjects such as “hello world” and “Go language” eligible. Rare legitimate
subjects remain eligible even when their support is one.

Final semantic ranking uses `0.6 × cosine + 0.3 × relative support +
0.1 × relative c-TF-IDF`. Reports explicitly record these typed weights and each
candidate's support, c-TF-IDF, cosine and combined score. The existing cosine
minimum of 0.25 and lexical/semantic diversity checks remain. Support alone
cannot qualify an unrelated candidate. Shortlists remain capped at 64, selected
labels at eight, and the generation-scoped embedding cache at 16,384 phrases.

## Initial candidate diagnosis

The first trial used semantic/support/c-TF-IDF weights 0.6/0.25/0.15. It admitted
the previously omitted concrete domain words and removed the reviewed greeting,
but procedural phrases still displaced the domain term from the eight labels.
The research-pipeline topic had two distinct canonical supporting threads:
“deadlock” ranked 12 with cosine 0.276; “runtime” ranked 10 with cosine 0.204
and failed the semantic gate. This motivated the stronger support contribution
in the final candidate. No topic-specific word whitelist is added.

All 32 first-trial corpus reports and eight focused reviews reproduced their
controls, preserved assignments/ancestor structure/source evidence, and had no
cache-dependent diagnostic/full-benchmark label discrepancies. Primary
representative/radius-0.4 named-leaf counts fell by one in every partition/order.
The inspected full-forward loss was a leaf previously labeled only “hello”;
it became unnamed after filler filtering. That is reported separately from
losing an actual subject label and is not counted as proof of overall relevance.

## Final candidate results

The second trial also produced 32 corpus reports and eight focused reviews.
Controls reproduce the prior singleton baseline excluding timing/new support
and weight metadata. Direct assignments, source identities/evidence, forest
relationships and independently recomputed thread/provider/ancestor counts are
unchanged. Focused/full-benchmark labels agree in all reviewed topics. Probe
support counts match the independently aggregated supporting-word counts;
combined scores match the recorded weights.

Primary representative/radius-0.4 results:

| Discovery | Order | Changed label sets | Named leaves, control → candidate | Label time, control → candidate |
| --- | --- | ---: | ---: | ---: |
| Full | Forward | 98 | 409 → 408 | 5,896 → 5,273 ms |
| Full | Reverse, warm cache | 105 | 408 → 407 | 842 → 935 ms |
| Reserved | Forward | 103 | 373 → 372 | 5,515 → 4,595 ms |
| Reserved | Reverse, warm cache | 102 | 370 → 369 | 1,028 → 573 ms |

The research-pipeline example now includes “deadlock” in both full and reserved
discovery, in both orders. Its full-forward labels previously began “case
recall audit”, “inspectable confirmation”, “agent diagnose missing”; the
candidate begins “deadlock”, “case recall audit”, “agent diagnose missing”.
The cBioPortal subject retains its named label and drops “hello”. A memory
optimization example gains direct memory/heap labels instead of longer path
phrases. These are specific recoveries, not a corpus accuracy score.

A private comparison includes all 50 existing review cases. Inspection of 14
previously troublesome cases uses their existing bounded source samples, not
complete thread transcripts or a blind review. It shows remaining regressions:
some longer specific labels become generic words such as “documentation”,
“analyzer” or “reports”; a reserved research topic splits “recall audit” into
separate words; conversational fragments such as “sounds” and “good” remain
eligible because they are not complete recognized continuations. “Runtime” in
the full-forward research topic still fails the fixed semantic gate (0.204).
The candidate therefore stays opt-in; it has not passed the broad quality gate.

All-eight-config full runs took 28.86 s control / 28.72 s candidate; reserved
runs took 27.06 s / 25.93 s. Peak RSS was approximately 1.03–1.06 GiB. These are
single contended offline runs, and warm label timings reuse earlier cache
entries. They do not establish a production speedup.

Validation passed: 550 regular tests, all-target/all-feature Clippy, doctests,
formatting and diff checks, plus 64 corpus reports and 16 focused reviews across
the two trials. New tests cover rare-subject retention, meaningful named phrases
containing greetings, shortlist saturation by one-off phrases, support-aware
ordering and invalid zero-support candidates. Slow installed-model corpus
checks remain explicit ignored diagnostics rather than extending ordinary CI.

## Reproduction and remaining gates

Use explicitly installed models and the frozen 789-thread SQLite snapshot with
`topics::evaluation::benchmark_topic_discovery` and
`topics::evaluation::review::benchmark_label_candidates`. Enable the existing
phrase, representative, semantic-label, clean-input, hierarchy, exact-routing,
conserved-support, context, visible-link and singleton-label experiment flags.
Run both insertion orders, radii 0.4/0.6, representative/anchored discovery,
and full/reserved discovery. Selected threads and label probes use private TOML.
No source repair, model download or index mutation is performed.

Private first-trial captures are under `mash-artifacts/topic-supported-20261001`;
final-candidate captures are under `mash-artifacts/topic-supported-final-20261001`.
Raw conversation IDs/text and scripts remain outside Git.

Support/score arithmetic, invariant assignments and recovered examples are not
a full quality gate. Broad source-grounded label precision, major/secondary
subject recall, mirrored-history support semantics, native Core ML validation,
topic lifecycle/persistence and MCP topic tools remain incomplete. Do not make
the policy a production default solely because vocabulary or timing decreases.
