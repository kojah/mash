# External dialogue evaluation preparation

`mash-c0d.19.2.30`, in progress. External service-proxy results now exist;
no topic method is selected.

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
inspected during the inventory. The subsequent frozen development experiment
below uses original utterances with installed local embeddings only.

The development shard contains512dialogues/7,690turns (3,845USER/3,845SYSTEM),
412multiservice dialogues and6observed dialogue-level services. Schema has8services.
Dialog IDs/per-dialog turn IDs are unique; speaker alternation has zero observed
errors. The subsequent executable loader validates the complete pinned shard,
including exact source spans and complete-text aliases.

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

`multiwoz.py` now separates immutable original turns from evaluation labels,
validates all 1,401 Unicode spans with corresponding UTF-8 offsets, and retains
95 copy annotations. Eight copied-value lists differ from their source-slot
alias lists; these differences are preserved without normalization. The schema
declares `book_taxi`, while 248 frames use `find_taxi`. Preserve the raw unknown
intent and exclude affected turns from primary proxy evaluation, rather than
fabricating a repair. Noncategorical schema slots may omit `possible_values`;
categorical slots must declare them. Synthetic corruption and feature-separation
tests exercise these boundaries.

The frozen first comparison uses 32 full-text canonical development groups,
chosen by hash seed `mash-topic-multiwoz-v1-20261001` without service stratification.
All 512 source groups are unique. `external_context.py` takes only typed original
turns into its current/mean/joined input constructor; annotations enter pair
evaluation separately. Positive pairs share a schema-declared active service,
negative pairs are disjoint, and both must have nonempty supported proxies.
Same-dialogue pairs, unknown intents and no-active states are excluded.

The sample has 238 USER anchors: 14 have unknown intents, 43 have no declared
active service (categories can overlap), and 191 support nonempty known proxies.
Only restaurant/hotel/train/attraction occur in those supported labels. Common
model support has 206 anchors, after excluding 32 roots for contextual controls;
no model-window exclusions occur. The common evaluation has 12,341 pairs,
3,448 positive and 8,893 negative. AP is 0.4642/0.5349/0.5555 and AUC
0.6455/0.7105/0.7201 for current/mean/joined respectively. Current-only own-support
AP is 0.4940 on 17,606 pairs; do not compare it directly to contextual common
support. See the [full benchmark](../../benchmarks/2026-10-01-external-dialogue-context.md).

Two fresh CPU-one-thread runs agree exactly on vectors, pair grids, memberships
of the proxy evaluation sets, pins and inference counts, excluding timings.
Each uses 650 unique singleton local calls plus 206 cache hits. Model inference
takes 1.29–1.42 seconds; experiment wall time after imports is 2.26–2.50 seconds.
This gives evidence for bounded context on this task-oriented proxy, not a
validated topic hierarchy, granular labels, coding-session quality, statistical
significance or production ingestion overhead. Pairs are dependent within
dialogues; pair count is not an independent sample size. Model pretraining
overlap is unknown. No generative calls, training/test shards or tuning were used.
Next evaluate episode/topic specificity, cross-domain breakdowns and independent
coding-session quality before selecting a method; retain untouched test data
until selection. Complete-ingestion and Rust-parity gates remain required.

The [intent-specificity follow-up](../../benchmarks/2026-10-01-external-intent-specificity.md)
reuses frozen vectors with evaluation-only declared intents and paired whole-dialogue
resampling. Context's broad-service gain does not transfer to within-service intent
distinctions: AP is 0.8020/0.7522/0.7378 for current/mean/joined; joined's empirical
paired AP range is [-0.0976, -0.0307]. These task proxies do not establish granular
coding topics, and resampling coverage is not calibrated. Do not promote joined
context as the sole topic representation. Bead `mash-c0d.19.2.31` tracks controls
that preserve specificity while admitting context. The source loader now retains
declared intent pairs and detects alias conflicts within a service; old inference
captures retain their historical code pins, reproducible at commit `1ba0bca`.

Pinned raw SHA256:

- README: `a9b2c9bb4ca61fdf1eb64a3c5d421271b14ffcc45029852fd26c71ca32302c9e`.
- Schema: `ae9e2390f38fb967af64623c2f4f7e0c636fb377ad523b582a03161d3ddbdf68`.
- Development shard: `e7ddb563e4da5766ea820cc826dead77e7ca219c19b761e218d62d9c999a252e`.

The raw corpus stays outside Git; do not execute upstream instructions or scripts.
