# Option approvals and conversational filler: offline evaluation

Topic quality is still experimental. This experiment extends the existing
branch-aware reply-context prototype to complete option approvals. It does not
enable production topics or change stored history/search embeddings.

## Source diagnosis

In the reviewed-startup BIRCH representative run (radius 0.4, forward order),
one topic labeled `good / sounds` was assigned to 14 threads. Bounded selected
evidence covered eight threads and 12 samples: all were approvals such as
“let’s do (1)” or “sounds good let’s do (A)”. This is a selected-source diagnosis,
not a full annotation of that topic or the corpus.

One reviewed native branch approves option A after an assistant proposes
evicting stale graph-cache entries. Its following substantive assistant response
discusses that fix and the race investigation. Intermediate tool records have
empty searchable projections; their canonical role does not make them new human
requests. The branch resolver must skip them without crossing the next actual
user request or taking a sibling answer.

Deleting this approval would discard a meaningful choice. Label-only masking
also leaves its original, acknowledgment-shaped document vector unchanged.

## Controlled change

`MASH_TEST_TOPIC_OPTION_CONTEXT=1` admits complete, bounded option approvals
through the existing branch-aware following-response resolver. The recognizer
is shared with the literal-safe label projection; unknown subjects, quoted
identifiers, code, operators, malformed selectors and trailing technical requests
remain outside this extension. The default remains deictic replies only.

Context keeps the original user text, records typed source provenance, and
requires a later substantive answer on an unambiguous native branch. Existing
window, text, unique-inference and compatibility caps still apply. New vectors
are inferred before committing any in-memory replacement. Aggregate and per-kind
eligibility, enrichment and skip counts distinguish deictic from option replies.

This borrows the **following answer**, rather than parsing the preceding option
definition. That answer can introduce results or new proposals. It is therefore
contextual evidence, not proof of the exact subject selected by the user.

The frozen 789-thread corpus, installed local models, reviewed startup exclusions,
skill exclusions, label policies, radii, orders and full/reserved-source split
are held constant. Control and option policies each run both BIRCH and exact
GRINCH: 64 corpus reports and 16 focused candidate reviews. Private source text,
identities, manifests and reports live outside Git under `mash-artifacts`.

## Results

All 64 corpus reports and 16 candidate reviews completed. Independent report
checks reproduced the previous BIRCH control, reconciled direct/inherited and
provider membership counts, and verified GRINCH tree connectivity, counts and
maximal-radius cuts. Serialized reports lack vectors for independent centroid
recomputation; these checks establish structure, not semantic correctness.

There are 15 eligible option approvals: nine receive context, one has no answer
and five hit the truncated-window safeguard. Existing deictic counts remain
three eligible, one enriched and two without answers. Ten unique inputs require
fresh inference, including the deictic input; the full BIRCH context pass took
345 ms on this shared host, excluding model loading.

For full-source representatives, radius 0.4, forward order, the `good / sounds`
topic falls from 14 assigned threads to eight under both algorithms. Case 2 loses
that generic tag while retaining its tracing/race-investigation labels. Both
algorithms change assignments or complete label lists in five of the fixed
50 selected cases (2, 6, 9, 18 and 40). This is not a five-case correctness score.
Case 18 gains deadlock/performance context tags; those need full-source semantic
review rather than being counted automatically as recovered subjects.

The 14 preselected comparison cases still show important remaining weaknesses:
`wure`, `eta / long`, generic workflow phrases and broad groups. The startup-only
case remains unassigned. Selected cBioPortal clinical-file and knowledge cases
and the PostgreSQL/HNSW/core-wrapper case have unchanged labels in this setting;
bounded excerpts do not establish their complete primary/secondary recall.

Forward/reverse coassignment Jaccard for full representatives at radius 0.4 is
0.8777 → 0.8764 for BIRCH and 0.5654 → 0.5645 for GRINCH. Reserved-source-only
scores are 0.8576 → 0.8567 and 0.9091 → 0.9134 respectively. These small, mixed
changes measure order stability, not topic quality. The extension remains opt-in;
neither algorithm nor production topic integration is promoted by this trial.

Validation: 568 regular tests, 97 focused topic tests, one doctest and all-target,
all-feature Clippy passed before adding the separate embedding pilot. Additional
pilot validation is recorded with that evaluation.

## Filler removal and document embeddings

The separate `mash-c0d.19.2.17` evaluation must compare unchanged inputs against
derived cleanup inputs. Dense models can tolerate filler; vector movement alone
does not establish better retrieval. Complete greetings/acknowledgment clauses
are a first ablation, with independent retrieval judgments for negation,
technical identifiers, rare subjects and short contextual replies. Canonical
text stays intact. Any production embedding projection would need versioned
generation/cache identity and a deliberate reindex policy.

Label weighting and embedding preprocessing are separate interventions.
[BERTopic's c-TF-IDF implementation](https://maartengr.github.io/BERTopic/api/ctfidf.html)
offers frequent-word reduction after normalized term frequency; this does not
justify deleting frequent words from dense-model inputs. Generic word frequency
also cannot distinguish a domain's central vocabulary from noise.

[Rust regex](https://docs.rs/regex/latest/regex/) already bounds a single search
by pattern and input length. Matcher speed alone is not a reason to add an
Aho-Corasick dependency or broaden removal rules. POS/dependency pruning and
rewriting require preservation and inference-cost evidence.
[LLMLingua-2](https://aclanthology.org/2024.findings-acl.57/) evaluates learned
token removal for prompt compression; it does not establish retrieval or topic
quality for mash's local embedding model.
