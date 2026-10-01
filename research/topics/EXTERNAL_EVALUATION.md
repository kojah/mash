# External dialogue evaluation preparation

`mash-c0d.19.2.30`, in progress. No external topic scores or selected method yet.

Use an external corpus alongside coding-history development diagnostics to reduce
dependence on repeatedly inspected assistant judgments. MultiWOZ2.2 is an annotated
task-oriented dialogue resource with corrected state/span annotations, not a corpus
of LLM coding sessions. Its annotation errors and domain mismatch must remain explicit.
See [the primary paper](https://aclanthology.org/2020.nlp4convai-1.13/) and
[the pinned upstream README](https://github.com/budzianowski/multiwoz/blob/fe0c8e65cfcd8462bd33c86e35f21addc84ca82b/data/MultiWOZ_2.2/README.md).

Source inventory,2026-10-01: upstream commit
`fe0c8e65cfcd8462bd33c86e35f21addc84ca82b`. Only README/schema and development
`dialogues_001.json` downloaded under
`/home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001`.
`source-manifest.json` retains URLs, byte sizes and SHA256; `inventory.json` records
the observation, not a quality experiment. No train/test shard or model output
inspected, no utterance values used in model inputs, zero inference calls.

The development shard contains512dialogues/7,690turns (3,845USER/3,845SYSTEM),
412multiservice dialogues and6observed dialogue-level services. Schema has8services.
Dialog IDs/per-dialog turn IDs are unique; speaker alternation has zero observed
errors. This inventory is not full schema or alias validation.

Every USER turn includes all8service frames, so frame presence would incorrectly
label every turn with every domain. There are30,760user frames:27,036have no active
intent,3,724have an active intent, and471user turns have more than one active intent.
6,036frames contain slot values, which may persist cumulatively. These are distinct
metadata views, not interchangeable current-utterance topic gold. Define any
service/intent/state-change evaluation proxy explicitly and inspect its limitations
before computing quality. Names, state values, slots and schema descriptions must
never become model text or feature shortcuts.

Upstream documentation discloses T5-assisted action corrections with manual review;
do not describe the dataset as a purely human topic annotation benchmark. The
separate dialogue-act file was not downloaded. This preparation has not established
independent coding-session topic labels, semantic parents or guided-search success.

Next build an executable loader that validates identities, roles and state/span
boundaries while separating exact utterance strings from all evaluation metadata.
Preserve official splits and content aliases, freeze development selection/controls
before model outputs, retain untouched test data until method selection, and pin
assets/code/data/parameters. Compare matched local-model windows and abstentions,
domain/aspect coverage, clustering order effects and complete cost. No generative
model calls or topic inference payments. External proxy results complement real
agent-session evaluation and do not replace complete-ingestion/Rust-parity gates.

Pinned raw SHA256:

- README: `a9b2c9bb4ca61fdf1eb64a3c5d421271b14ffcc45029852fd26c71ca32302c9e`.
- Schema: `ae9e2390f38fb967af64623c2f4f7e0c636fb377ad523b582a03161d3ddbdf68`.
- Development shard: `e7ddb563e4da5766ea820cc826dead77e7ca219c19b761e218d62d9c999a252e`.

The raw corpus stays outside Git; do not execute upstream instructions or scripts.
