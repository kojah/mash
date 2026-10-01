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
