# Injected skill documents: conservative topic-input experiment

A full canonical user-source review of GRINCH comparison case 25 changed the
interpretation of an apparent secondary-assignment loss. Its three stored user
messages contain environment context, a startup/persona prompt and an injected
skill document. There is no explicit human request to execute an exact claim
command. Losing that BIRCH label cannot be counted as lost user-topic recall.
The private review includes all three messages and their SHA256 hashes; the
bounded earlier evidence reports did not contain their complete text.

Add an opt-in, derived-input exclusion for complete known skill documents:
`MASH_TEST_TOPIC_SKILL_CONTEXT=1`, used with `MASH_TEST_TOPIC_CLEAN_INPUT=1`.
The default preserves them. This is an offline topic prototype experiment,
not a production ingestion or search change.

The recognizer requires a complete standalone `<skill>` wrapper, name and
absolute Unix `.../SKILL.md` path, and the observed two-field frontmatter with
a matching name and a nonempty single-line description. It limits input to
65,536 bytes. Quoted examples, leading discussion, trailing human requests,
nested/unfinished wrappers, unknown metadata, different filenames, relative
paths, multiline descriptions and oversized documents survive unchanged.
This deliberately recognizes a narrow observed format; it is not an XML/YAML
parser or a general context-removal rule. Plain startup prompts remain.

Canonical stored history, search embeddings and source-group identities are
unchanged. Derived user-source/message alignment remains intact, including
replacement-vector positions when an earlier source is omitted. Typed reports
record the policy and omitted-document count. No additional replacement text
or embedding inference is introduced by this exclusion.

The manual ignored Rust test
`topics::evaluation::input::runtime_context::inventory::benchmark_skill_context_inventory`
reads an explicitly supplied frozen SQLite database and writes a typed private
inventory. Set `MASH_TEST_SEARCH_DB` and `MASH_TEST_TOPIC_REPORT_DIR`; the
inventory compares both policies without downloading or loading models.
It counts affected threads and newly empty derived inputs and checks that
replacement texts remain identical. It does not run clustering or measure
quality, inference latency or model memory.

On the frozen 789-thread corpus used by the GRINCH trial, the inventory reports:

- 165 omitted skill documents across 165 threads, all Codex.
- 165 fewer retained derived sources after ordinary input cleanup.
- Zero newly empty threads.
- Identical replacement-text inventories.

Private artifacts are under
`/home/james/scratch/mash-artifacts/topic-skill-context-20261001`.
These counts establish the scope of the recognizer, not an improvement in
topic precision, recall, labels or hierarchy. The next gate is a controlled
clustering comparison plus source-based review of affected subjects and
secondary assignments. Bead `mash-c0d.19.2.14` remains in progress.

Validation: 563 regular tests passed, including conservative framing and
source/replacement-position checks; the ignored real-corpus inventory passed.
Clippy with warnings denied, doctests, formatting and diff checks passed.

## Controlled clustering comparison

The follow-up release experiment completed 64 topic reports and 16 focused
candidate reviews: preserve/omit policies, BIRCH/GRINCH, full/reserved-source
discovery, representative/anchored inputs, radii 0.4/0.6 and both insertion
orders. Embedding assets, label scoring, discourse masking, exact routing,
assignment weighting and hierarchy construction stayed fixed. Representative
label scoring remained disabled. No models were downloaded.

All preserve controls reproduced the earlier GRINCH experiment, excluding
timings and the new policy metadata. Structural audits passed for direct and
inherited thread/provider counts, semantic forest reciprocity and cycles,
GRINCH tree/cut coverage, and focused candidate scores. Canonical source-group
identities, embedding generation and eligibility remained unchanged across
policies. Both algorithms received identical discovery vectors within each
policy; exclusion intentionally changes those vectors between policies.

All exclusion runs omitted exactly 165 skill documents. Projected-message,
deduplication and replacement-embedding counts stayed unchanged, as did the
context-enrichment summary. Full representative discovery used 704 vectors
before exclusion and 706 afterward; anchored discovery used 484 in both.
Removing a source changes segment construction and bounded representative
selection, so it need not reduce the number of discovery representatives.
These are bounded discovery inputs, not a raw-message ingestion benchmark.

At radius 0.4 with full representative discovery, both algorithms still
assigned all 778 eligible threads, with four tags at the 95th percentile.
Changing counts is not evidence of better topic precision or recall.

| Representative discovery scope | Radius | BIRCH preserve / omit | GRINCH preserve / omit |
| --- | --- | --- | --- |
| Full, canonical sources | 0.4 | 0.9185 / 0.8944 | 0.6137 / 0.5654 |
| Full, canonical sources | 0.6 | 0.3965 / 0.3875 | 0.8507 / 0.8505 |
| Reserved sources only | 0.4 | 0.8789 / 0.8576 | 0.9487 / 0.9114 |
| Reserved sources only | 0.6 | 0.9093 / 0.5944 | 0.7539 / 0.7080 |

These numbers are forward/reverse co-assignment Jaccard after collapsing
canonical copies. They measure order sensitivity, not agreement with human
topics. Exclusion did not improve stability in these configurations. Reserved
sources were excluded from discovery, but label statistics still use assigned
threads; this is not a completely held-out label-quality evaluation.

In the fixed 50-case bounded-source comparison, labels or assignments changed
in 18 cases for BIRCH and 20 for GRINCH. These counts include term reordering;
they are not quality scores. The existing 14-case review selection was retained.
Generic acknowledgment topics still appeared, and context-only case 25 still
received workflow labels from its surviving startup context. Skill-document
exclusion alone does not solve that contamination.

An additional exploratory review read all four canonical user messages of
case 43, with hashes captured privately. Its explicit human request concerns
Dolt maintenance; the Mayor workflow appears in injected material. Exclusion
removed that Mayor label while retaining the maintenance topic under both
algorithms. This supports the narrow exclusion for this case, but does not
establish corpus-wide secondary-topic recall or recognizer precision.

Full representative fit times on the shared host were 357–498 ms for preserved
BIRCH, 406–535 ms for excluded BIRCH, 1,572–3,273 ms for preserved GRINCH and
1,480–2,347 ms for excluded GRINCH. These exclude model work, assignment and
label generation and are not end-to-end latency or a controlled speedup claim.

Private reports and audits are under
`/home/james/scratch/mash-artifacts/topic-skill-context-clusters-20261001`.
Keep the policy opt-in. The next work is source-aware handling of generated
startup context and independent primary/secondary-topic review, rather than
promoting either algorithm or interpreting stability as quality.
