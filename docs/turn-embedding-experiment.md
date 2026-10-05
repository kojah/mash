# Turn embeddings: separate experimental gold artifact

Issue mash-b2f; experimental goal only, no production migration. The user fixed
the boundary: consecutive user prompts before an assistant response form a
bundle; after an assistant response starts, the next user reply closes that turn
and starts another. Several assistant segments may belong to one turn. The last
turn remains open. Explicit roots/parent edges outside the active turn split
boundaries; never merge threads or forks. Preserve assistant-only/orphan runs as
explicit orphans rather than inventing a user prompt. Ignore only known injected
instruction/envelope records; provider progress/reasoning metadata is absent
from the frozen corpus, so do not guess which assistant prose to delete.

## Frozen experiment before outputs

Use the 48 development threads and frozen references from the additional
algorithm screen; 24 reserved threads stay closed. Keep source identities, exact
text spans, turn boundary reasons and revisions. Build a separate private SQLite
gold artifact, versioned by source hash, recipe, model/tokenizer and runtime.
Retain original corpus/message vectors untouched. SQLite transactions protect
active publication; content-addressed vectors allow reuse across edits/restarts.
This is an experimental artifact store, not production search integration.

Four fixed representation candidates:

1. Prompt-only: window the full prompt bundle; orphan turns use assistant text.
2. Joint truncation control: concatenate prompt and answers, one 512-token input
   including special tokens; report discarded tokens rather than hide truncation.
3. Prompt-conditioned passages: cover the complete prompt and all answers;
   answer windows carry at most 96 prompt tokens. Each model input fits 512
   tokens including special tokens, with 32-token overlap. Keep source spans.
4. Pooled existing-message vectors: normalized equal-weight message mean per
   turn, a zero-new-inference control, not equivalent to joint text inference.

Use the already installed Xenova all-MiniLM-L6-v2 quantized ONNX and local
tokenizer only. CPU inference, batch eight, one intra/inter-op thread, masked
mean pooling and symmetric per-vector signed-i8 quantization. Asset digests and
engine versions identify this artifact's generation; do not claim numerical
identity with Rust. Compare a fixed 64-message inference probe against stored
vectors and report drift separately. No new models, API calls or LLM.
512 tokens matches the inspected FastEmbed default used by this corpus;
the upstream model card recommends short inputs and describes default
256-wordpiece truncation. The chosen window is an explicit experimental recipe,
not evidence that long inputs retain short-input quality.

Cluster each representation with average-link cosine cuts .35/.50/.65 only;
fixed final membership cutoff .35 and margin .05. Retained clusters must contain
at least two distinct turns with substantive support, not merely two overlapping
passages from the same turn. Score the same original-message pairs, plus
cross-turn pairs separately; report the turn-bundling-only baseline and existing
message/window reference. Do not call automatic question/answer joining a gain
in cross-turn discovery. Ambiguous labels remain excluded; no invented negatives.
The representation screen has twelve settings. Select the representation with
the highest cross-turn macro F1 (complete fits only; recipe registration order
breaks ties), then retry the unchanged twelve first-study configurations and
fifteen second-study alternatives on that representation. This is exploratory
development selection, not an independent confirmation. Each fit has a
30-second process deadline; failures exclude a setting from ranking. No extra
thresholds after viewing outputs. Record existing-message and same-runtime
raw-message average-link controls at the three registered cutoffs. Passage
primary assignments map to source messages by union, separately from soft
memberships; do not arbitrarily force a multi-passage source into one cluster.

Record artifact size, turn/passage counts, inference/build/reuse cost, truncation,
source coverage, grouping runtime/RSS and reviewed examples. Test multiple
prompts/responses, user interruption, branches, orphan/incomplete turns,
Unicode spans, tokenizer budgets, source edit/deletion, restart reuse and
failure preserving previous publication. Development P>=.85/R>=.70/F1>=.75 and
>=.10 F1 gain over best simple baseline are screening requirements; original
coherence/provider/coverage gates and stronger independent held-out references
remain prerequisites for promotion. Negative screening is a finished experiment,
not a production feature. Update the living LaTeX paper with actual findings.

Sources: [model card](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2)
and inspected FastEmbed 6.0.1 source in the pinned local Cargo dependency.

## Completed diagnostic results, 2026-10-05

Four recipes produced 2,406 turns from 5,389 eligible source messages. The
48-thread development sample has 111 related / 117 unrelated / 60 ambiguous
pairs; cross-turn counts are 57 / 59 / 37. Twenty-four threads have cross-turn
positives. Another 54 positive pairs are within turns; 47 negative and 14
ambiguous pairs touch excluded instruction records. Selection macro cross-turn
F1 averages the 24 positive-bearing threads; micro scores include negative-only
threads. All-pair macro F1 averages all 48. These denominators are different.

Average-link representation screen, best registered cut (.35) for each:

| Representation | Units | Cross macro F1 | Cross micro P / R / F1 |
| --- | ---: | ---: | --- |
| Prompt-only | 2,755 | .667 | .833 / .614 / .707 |
| Joint truncation | 2,406 | .726 | .673 / .649 / .661 |
| Prompt-conditioned passages | 3,727 | .736 | .698 / .649 / .673 |
| Pooled stored messages | 2,406 | .718 | .632 / .632 / .632 |

Passages won the registered representation screen. Best complete setting per
family on those vectors, ranked by the registered cross macro metric:

| Family / setting | Cross macro F1 | Cross micro P / R / F1 |
| --- | ---: | --- |
| Leader / .35 | .758 | .672 / .719 / .695 |
| Average-link / .35 | .736 | .698 / .649 / .673 |
| BIRCH / radius .60 | .712 | .714 / .614 / .660 |
| HDBSCAN / minimum 2 | .572 | .692 / .474 / .562 |
| Complete-link / .50 | .650 | .681 / .561 / .615 |
| DP-means / cosine parameter .50 | .749 | .554 / .807 / .657 |
| OPTICS / xi .05 | .688 | .597 / .702 / .645 |
| Spectral / 4 | .634 | .571 / .491 / .528 |
| Affinity / preference quantile .50 | .700 | .680 / .596 / .636 |

1,871/1,872 candidate fits and all 288 message-control fits completed. Affinity
quantile .75 failed convergence on one thread and was excluded from ranking.
There were no timeouts or other errors. Equal-similarity affinity warnings were
retained in the private log. The repeated average-link settings reproduced
identical pair and primary scores. Largest completed candidate fit: 1.311 s;
timings exclude process startup, inference and preparation and are single runs.

Matched raw-message average-link .35 gives cross macro .698 / micro .679;
stored-message .35 gives .648 / .648 under the same distinct-turn retention.
The original context-message reference gives .712 / .667; chronological windows
give .673 / .724; whole-thread gives .787 / .659 (micro precision only .491).
The selection metric alone therefore favors broad merging: use precision and
negative-only-thread counts before judging an improvement. No turn candidate
beat the chronological baseline's cross micro F1 (.724).

Cluster-only all-pair macro F1 is low: leader .515, best complete setting .533
(BIRCH), versus chronological .718. This contract requires two-turn support and
does not automatically return a lone prompt/answer bundle as a retained cluster.
To distinguish that abstention from embedding quality, a **post-hoc diagnostic**
adds disjoint within-turn groups to the already selected leader result. It makes
no new cross-turn predictions, changes no selection, and is not confirmation.
The combined view has all-pair macro P/R/F1 .767/.858/.772; micro .754/.856/.802.
It gains .054 macro F1 over the chronological baseline, failing both precision
.85 and the required .10 gain. Cross-turn scores remain exactly unchanged.
This is a modest exploratory indication, not a production-qualified algorithm.

### Cost and coverage

| Recipe | New vectors | Build seconds | Embedded source characters |
| --- | ---: | ---: | ---: |
| Prompt-only | 2,716 | 110.50 | 50.53% |
| Joint truncation | 664 | 38.86 | 69.85% |
| Prompt-conditioned passages | 1,387 | 85.42 | 99.97% |
| Pooled control | 0 | .20 | No new text inference |

Recipes ran sequentially and reused earlier cached text; these are incremental
costs in this order, not four independent cold-build comparisons. Joint inputs
discarded 179,585 tokenizer tokens. Passage inputs covered all non-whitespace
source content, with overlap/context repetition (653,502 model input tokens
including specials). Character coverage counts each original character once;
window-boundary whitespace explains the small uncovered remainder. Prompt-only
coverage includes assistant-only orphans and does not mean half of prompts were
discarded. Pooled vectors already contain source information despite zero new
text coverage. Membership associations and actually embedded spans are distinct.

The fixed 64-message raw-inference probe versus stored vectors had mean cosine
.99184, minimum .98339, fifth percentile .98671: close, but not identical.
Keep the same-runtime controls. Entire inference/control/sweep job took 681.30 s;
reusing the three text recipes took .122 s with zero new inference and exactly
identical cached vectors. The combined SQLite experiment store is 43.05 MiB,
including all recipes and raw-message control vectors; this is not the size of
one production artifact. Linux parent high-water RSS reached 929.08 MiB; sampled
child maximum was 889.56 MiB and includes inherited model/corpus state. Child
sampling at one second can miss peaks. This unoptimized Python cohort runner
does not establish production per-thread memory or latency.

The visible corpus omits 1,346 assistant parent records and yields 1,393 orphan
runs. It cannot validate complete-history bundling. References are sparse,
agent-authored, and only cover two providers. **Stop without promotion or held-out
scoring.** Complete ancestry and independent cross-turn labels are tracked in
mash-7w0 before another quality claim. No production schema or search changes.

### Private reproduction ledger

Files are in `/home/james/scratch/mash-artifacts/turn-embeddings-20261005`.
The pre-output protocol is saved as `protocol.md`; this document now adds results.

- Corpus SHA-256: `a4f84e869808f6ae243a973de125cfd25ccd4738e86f63e359f6758c41554773`
- References: `dc66aa568ab442e84abc810778e0c0a2abb4e102b0a66bbff4fa9f183d212732`
- Frozen protocol: `7f8d33e97d5b9b46e2d5cc38ddb5af0f8c7a9a8f09056be8c5c7216a7691b6ed`
- Main report: `182d46159f959bd5d56d23979960f82370e9d919c2a943642336751c6054b02b`
- Encoder manifest: `2894b8dc20a8bea9f9f78c49d9e349728c69450a273a509298ec8aaf94905491`

`summary.json` contains original-baseline comparisons; `overlay.json` labels the
single post-hoc bundle-union diagnostic. `gold.sqlite` preserves source mappings
and cached vectors. Raw history, judgments and vectors stay outside Git.
