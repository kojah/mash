# Model-dependent capacity rerun

Registered before new inference or scoring. Reference: frozen voyage4-joint-v1.
Original confirmation stays sealed. All 156 matched histories are development
data, including the former 52-thread evaluation subset consumed by selecting
the reference. This rerun makes no independent-confirmation claim.

Use the same 104/52 development split and source-preserving turn delineation.
Read the reference manifest and verify its report/split hashes; do not use a
hard-coded MiniLM reference, tokenizer, hidden-state size or 512-token window.
Inputs are current lean prompts and all visible assistant segments with the
same role markers as the joint-turn reference. Re-screen complete text before
upload and keep per-thread text/results private.

Run eight float arms: Voyage 4 and Voyage Code 4, each at 1024 and 2048 dimensions,
each with the historical 28,000-byte structural budget and with a model-token
budget of 31,992 (eight tokens below their documented 32,000-token contexts).
Both model-specific public tokenizers are loaded without truncation/padding;
pin their Hugging Face revisions and file SHA-256 hashes. Only tokenizer data
is downloaded, not model weights or executable remote code. Preserve every
source character when splitting; prefer late paragraph/line boundaries.
Measure actual tokens, not a MiniLM/tiktoken approximation.

Known tokenizer revisions: voyage-4 44f3b2ae4ddf33403ed4dd66bec3fa48ff7dbbf9;
voyage-code-4 ac780b405e54ff8b3aff681270e0e03840fed2f3. Both tokenizer.json files
have SHA-256 c0382117ea329cdf097041132f6d735924b697924d6f6fc3945713e96ce87539.
Preflight found 14 turns with changed segmentation, maximum 24,544 tokens and
124,516 UTF-8 bytes; hence every full turn fits the selected token budget.

Cache identity includes model, requested dimensions, task mode and tokenizer
hash, plus exact input text hash. Changing only the permitted input budget does
not change a vector for identical text. Import existing 1024-dimensional raw
floats only for exact compatible model/task/text identities; 2048-dimensional
vectors require new inference. Record imported provenance and new receipts.
Never use one dimensionality's vectors in another dimensionality's namespace.

Use OpenRouter, default task, float responses, no fallbacks and denied data
collection. Batch <=64 inputs and <=100,000 actual model tokens, two requests
concurrently; up to three transient retries. Keep the new reported/estimated
charge cap at $3, preserving the earlier caches. Record usage, model, dimensions,
response IDs and timings without secrets/input text.

For each arm report frozen settings first (BIRCH radius .50, membership .75,
relative-best margin .05, union; alpha .10/window four on pooled view). Replay
the 1024/byte-budget Voyage 4 reference predictions exactly. Also give every arm
48 Optuna trials with identical seeded draws, tuning radius {.40,.50,.60,.70},
cosine {.55,.65,.70,.75,.80,.85}, margin {0,.05,.10,1}, view {pooled,chunk,union}.
Enqueue the frozen configuration first; count completed trials, not queued ones.
Choose only on the 104 calibration histories; report chosen settings on the
same 52 consumed-development histories. No further adaptive tuning follows.

Keep within-thread fitting, distinct-turn support and normalized joint-piece
pooling unchanged. Report confusion counts, precision/recall/F1, long-thread
metrics, distant related-pair recall, and 2,000 paired thread-bootstrap F1 gain
intervals against the frozen joint reference. Intervals are exploratory and
unadjusted for multiple arms. Record hashes/trials and aggregates in local
Optuna/MLflow, explicitly using baseline ID voyage4-joint-v1. Update the paper.

Implementation guards cover tokenizer-bound Unicode/source preservation,
model-dependent dimensions and padding/context metadata, response shape checks,
cache separation, original local-generation parity and the production signed-byte
cosine path for 2048-dimensional vectors. Existing historical scripts remain
reproducible at their pinned revisions; defaults for the installed local MiniLM
model are intentional defaults, not restrictions on this remote experiment.

Primary capability and tokenizer sources:
[model limits](https://docs.voyageai.com/docs/embeddings),
[tokenization](https://docs.voyageai.com/docs/tokenization).
