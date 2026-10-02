# Source-preserving prose/block-code channels: coverage foundation

Bead `mash-c0d.19.2.36`. This establishes an audited representation boundary
before testing channel weights. It does **not** measure topic quality, choose
a discovery method, or add production filtering.

## Prospective contract

Classify only explicit HTML `pre` content as block-code. Inline `code` content,
technical names and ordinary prose remain in the prose channel. Do not infer
code from vocabulary, indentation, language attributes or model predictions.
Treat inserted block separators as neutral and markup/comments as explicitly
excluded, using the original renderer's exact ledger.

`question_channels.py` subclasses the frozen `SourceParser` solely to attach
channel context. A second run through the unchanged original renderer must agree
exactly on displayed text and every source/rendered piece coordinate and SHA.
Preserve Unicode code-point and UTF-8 byte offsets separately. Nested, unmatched
or unclosed `pre` rejects with an explicit reason rather than guessing HTML
repair. Original active-content/declaration/size/piece caps remain enforced.
Self-closing `pre` is an empty boundary and does not classify later text as code.
This is a declared text projection, not browser rendering or full HTML validity.

Coalesce adjacent displayed pieces within a channel; inline markup does not
split a displayed identifier. Preserve contiguous runs across the complete
displayed text, including neutral separator runs. Every token from the unchanged
technical `TOKEN` regex must lie wholly inside prose or block-code. Title text
is counted separately once. No tokenizer fixes or model work are included.

Count the existing source-adjacent unigram/bigram analyzer independently per
content run. Require complete unigram conservation; any whole-body phrase lost
at a run boundary is recorded explicitly. Do not concatenate prose before/after
removed code into fabricated phrases. Neutral paragraph boundaries also break
phrases, so omissions are **boundary omissions**, not exclusively code/prose
transitions and not independently judged bad phrases.

Audit all 200 already-observed question sources. The loader retains its original
source pins, but no reference tags, label names, rationales or quality outcomes
enter this stage. No reserved-outcome performance is measured. An initial setup
run stopped before corpus output because parser/loader pins were looked up in
the topic rather than complete-unit capture. Validation now uses the explicit
original unit capture, itself pinned by the topic manifest; the method did not
change.

## Results

| Evidence | Result |
| --- | ---: |
| Source questions supported / observed | 200 / 200 |
| Questions with explicit block code | 166 |
| Prose lexical tokens | 21,564 |
| Block-code lexical tokens | 24,113 |
| Total lexical tokens | 45,677 |
| Source/rendered ledger pieces independently checked | 23,107 |
| Boundary bigram occurrences omitted | 104 |
| Questions with boundary omissions | 51 |

Block code accounts for 52.8% of lexical tokens. This warrants a channel-weight
experiment but does not prove code is noise: it may contain the decisive topic
evidence. All displayed text and unigrams are preserved; no synthetic words or
new within-channel phrases are created. The 104 omissions occur across explicit
content-run boundaries. Reference-quality judgments have not been made.

Two fresh processes match all non-timing output. A separate HTMLParser callback
state machine independently checks all piece channel assignments, run/source
offsets and hashes, all 45,677 token identities/channels and independent analyzer
counts/omissions. It shares Python's HTML lexer and the frozen displayed renderer;
this is not independent browser parsing. Seven new tests cover inline identifiers,
entities, Unicode/combining marks, CRLF, attribute/comment tag lookalikes, empty/
self-closing/uppercase code blocks, separators and unsupported boundaries/caps.
All **289** lab tests pass with the graph extra; two unrelated graph integrations
skip explicitly without that optional dependency.

The full source-only audit takes 1.252/1.204s after imports, including double
rendering and ledger/hash checks. No inference/download occurs. This is not
complete ingestion; CPU/RSS, p95, edits/deletes and throughput impact remain
unmeasured. Earlier model inference remains charged. Production stays unchanged.

Outside-Git artifacts under `topic-public-stackoverflow-20261002`:

- `channels-first.json`: `ce527aac29659a72872e9430a8bfc534f5c05113f9e31ecdc638cffd22b0b634`
- `channels-second.json`: `ca397125c86d03eef46ba72d53bfc860f750395580da36e0110c0f097ef5645c`
- `verify-channels.py`: independent channel-state/token/count/coverage audit.

Next freeze title/prose/block-code weighting and discovery controls before
outcomes, preserving a matched full-source baseline and explicit low-evidence
abstentions. No production source exclusion, useful topic hierarchy, independent
semantic quality, guided-search gain or Rust parity follows from source coverage.
