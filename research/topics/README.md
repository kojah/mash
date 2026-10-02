# Topic methodology lab

The [TIAGE training/development boundary experiment](../../benchmarks/2026-10-02-tiage-boundary-calibration.md)
uses `boundary_scoring.py`, `tiage_calibration.py` and `tiage_development.py`.
Training-only exact-F1 cutoff selection is sealed before development capture;
no development recalibration or test download occurs. Adjacent development F1
.4287 retains low precision .3416; previous-two averaging is weaker. Full source
coverage, inference repeats and independent score/threshold/prediction/bootstrap
audits pass, but this does not select episode extraction or topic modeling.
Use pinned inputs, explicit installed assets, fresh output/repeat arguments and
the external memory monitor. Wide timing variation is not production performance.

The [TIAGE boundary preflight](../../benchmarks/2026-10-02-tiage-boundary-preflight.md)
pins the author's human-annotated training data without downloading development,
test or weak-supervision text. `tiage_capture.py`/`tiage.py` preserve all original
turns, labels, sentinel semantics and source hashes; typed parsing, exact repeats
and an independent every-row audit pass. The preflight itself runs no inference
or boundary metrics. These are independent open-domain boundary annotations, not coding-topic
labels or ancestor gold. Freeze calibration/evaluation rules before the next run.

The [stage comparator](STAGE_PARITY.md) prepares the future Rust/Python parity
boundary. Its ten adversarial standard-library tests run in CI without private
data or model downloads. Exact memberships/ancestry cannot be excused by numeric
tolerances. This is comparator coverage, not actual selected-method Rust parity.

The [character feature control](../../benchmarks/2026-10-02-character-topic-features.md)
uses `character_features.py`/`character_questions.py` for six word/character
source-similarity matrices with matched weighting/IDF. Characters preserve
synthetic identifier distinctions but rank the exposed tag proxy worse; no
tested cut passes. This is not a topic assignment, hierarchy or query benchmark,
and vocabulary construction differs between bases. Run with the standard
fresh-output/repeat arguments; no representation is promoted.

The [source token-boundary audit](../../benchmarks/2026-10-02-topic-token-boundaries.md)
uses `token_boundaries.py`/`token_audit.py` to preserve and inspect all 200
title/prose/code inventories. It exposes punctuation/identifier collisions and
stopword/vocabulary exclusions without changing old tokenization or claiming
semantic quality. Maximal diagnostic spans are not a selected replacement
tokenizer. Use the standard fresh-output/repeat arguments; original capture
guards and production search remain unchanged.

The [fixed-K online LDA control](../../benchmarks/2026-10-02-online-lda-topics.md)
uses `online_lda.py`/`lda_questions.py` with pinned sklearn partial-fit updates,
18 fits and 108 exposed settings. None qualifies; arrival-order predictions
and historical memberships drift. Exact repeats and direct library replay
check wrapper fidelity, not semantic quality or Rust parity. Fixed K supplies
neither unknown-topic discovery nor semantic ancestry. No production method
is selected. Run with the standard `--root`, fresh `--output`, repeat `--compare`
arguments in the pinned graph research environment.

The [blind topic-review pilot](../../benchmarks/2026-10-02-blind-topic-review-pilot.md)
prepares 32 source-grounded pairs with predictions/tags kept in a separate
manifest. Use `blind_topic_packet.py` and the
[review rubric](blind-topic-review-rubric.md) for reproduction. Answer fields
remain unanswered; no independent human judgments or semantic quality claim
follow from packet preparation. `topic_review.py --packet review.json --answers
completed.json` validates complete submissions. Add `--second other.json
--manifest unblinded.json` for separate per-stratum categorical agreement.
Reviewer declarations are self-reports; agreement is not accuracy or independently
verified gold. Free-text topic labels and semantic ancestry are not scored.

Topic modeling is the first priority. The agreed sequence is research compilation,
approach selection, Python experiments with recorded results, and Rust
implementation only after selection. This directory is an offline lab, not a
runtime dependency or a new CLI command. See the
[research compilation](../../benchmarks/2026-10-01-topic-quality-research.md).

Topic modeling must be fast enough to avoid materially slowing ingestion and must
not use an LLM. Exclude generative summarization, rewriting, labels and taxonomy
synthesis, including local LLMs and paid topic-inference calls. Reuse existing
local embeddings first; additional non-generative encoder work needs measured
quality gains and bounded costs. LLM-based prior art is context, not a candidate
implementation. Independent source review remains an evaluation requirement.

The provisional performance gate is no more than 10% ingestion-throughput loss
versus topics disabled on matched workloads. Measure the full path, including
context processing, extra encoder calls, clustering, label maintenance and writes,
for initial indexing, new threads, appended turns, edits and deletions. Keep model
assets, provider inputs, routing/backends and hardware fixed. Report warm/cold
runs, repeated timings, p95 update latency, CPU and peak process RSS, and distinguish
amortized label refresh from immediate processing. Current clustering-only timings
do not satisfy this gate. Quality and performance both need evidence before Rust
method selection or promotion.

## Shortlist and decision protocol

| Boundary | First comparisons | Reason |
| --- | --- | --- |
| Unit | Whole-thread means, bounded user episodes, turn-pair/context units | Long sessions mix subjects; short approvals need context without importing unrelated assistant topics. |
| Discovery | BIRCH reference, offline HDBSCAN, average-link control; then GRINCH/online alternatives | Separate geometry failures from label failures and quantify online tradeoffs. |
| Labels | c-TF-IDF; contrastive training-background weights; representative phrases/MMR | Preserve technical and secondary subjects while reducing conversational labels. |
| Hierarchy | Independently reviewed semantic parents and overlapping leaves | Tree topology alone does not establish ancestor correctness. |

The [Links primary-source assessment](../../benchmarks/2026-10-02-links-assessment.md)
adds a bounded reference trial to the online shortlist. Its graph repair is
relevant to measured centroid drift, but indivisible subclusters, calibration,
dynamic identity/publication and deletion/replay remain concerns. Connected
components are not semantic ancestors. `links_thresholds.py`/`links_preflight.py`
verify the pinned author formulas and frozen candidate-only geometry; exact
repeats and independent reductions pass. No full Links clustering, topic quality
or performance is established, and the reference defaults are not product defaults.

Freeze native source-group splits before fitting or label selection. Annotate
primary and secondary subjects, irrelevant/context-only text, episode boundaries,
and parent relationships independently of algorithm outputs. Review full sources
where available and mark incomplete evidence explicitly. Keep a development set
and a reserved set; do not tune on reserved labels. Report provider and rare-topic
breakdowns, not just a single coherence score.

Record assignment precision, secondary-topic recall, label specificity, intrusion
judgments, unassigned coverage, ancestor correctness and insertion-order stability.
Record wall time, peak process RSS, inference calls, and incremental update cost.
The [annotation protocol](annotation-guidelines.md) defines source/context scope,
episode granularity, overlap, ancestors, uncertainty and independent review for
the next dataset; it does not retroactively alter the initial pilot.
Automatic coherence is a diagnostic, not a replacement for source review.
Select an approach only after reviewing these tradeoffs against a frozen baseline;
no numeric quality acceptance threshold has yet been justified by annotations.
Then specify bounded online insertion, edit/delete, restart and model-version
behavior before porting to Rust or exposing topic MCP tools and filters.

Update the [living LaTeX paper](../../paper/README.md) as experiments proceed,
including procedures, setup, citations, results, analysis and limitations.

## Source-pinned boundary development pilot

`episodes.py` accepts private TOML annotations with `schema_version = 1`, the
embedding `generation`, and `[[cases]]` containing `thread`, `source_sha256` and
ordered `[[cases.boundaries]]` records (`left`, `right`, `judgment`, `rationale`).
Judgment is `same`, `change` or `uncertain`; ordinals identify adjacent nonempty
searchable user turns. Source SHA256 covers every such turn's native ID, ordinal
and full searchable text using compact UTF-8 JSON. Every boundary must be present,
including uncertain ones. Reserved groups are rejected for this development pilot.

The controls are windows of three turns, adjacent cosine below 0.55 and current
segment-centroid cosine below 0.55, plus disjoint two-turn block means at the same
threshold. Blocks use one future user turn, never share input across the boundary,
and abstain on missing vectors or numerical cancellation. This needs bounded
lookahead/provisional decisions for an online implementation; it does not re-embed
context or resolve references. Nonempty unembedded turns remain in sequence;
cosine predictors abstain on adjacent boundaries and reset rather than silently
bridging missing evidence. Uncertain judgments are excluded from scoring;
abstention does not earn positive recall. This is user-only linear presentation,
not branch-aware dialogue segmentation or independently validated context labels.

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/episodes.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --annotations "$PRIVATE_ANNOTATIONS" --output "$PRIVATE_REPORT"
```

The first development pilot is recorded in the paper: four purposively bounded
threads, 29 boundaries, six uncertain. One coding assistant reviewed user texts
before these predictions, with prior corpus exposure; this is not blind human
ground truth. All controls need a broader independently annotated test set before
method selection or threshold tuning can establish quality.

The block comparison increased false splits from 14 to 16 while retaining all six
judged changes. The negative result is in the paper; thresholds/annotations were
not adjusted to improve it. The lab's signed-byte decoding was checked against
Rust, with exact reconstructed norm parity on all 6,714 stored user vectors.

## Re-embedded text context control

`text_context.py` uses explicitly supplied installed MiniLM ONNX/tokenizer assets,
without model downloads. It re-embeds each user turn individually as an inference
control, then compares disjoint blocks of up to two user turns joined by two
newlines. Singleton CPU inference uses one thread, attended-token mean pooling and
Rust-compatible symmetric signed-byte rounding. Inputs over 512 tokens are
rejected, never silently truncated. A bounded 1,024-input cache avoids repeating
identical work within the run. Every context records its component message IDs.

```bash
PYTHONDONTWRITEBYTECODE=1 HF_HUB_OFFLINE=1 OMP_NUM_THREADS=1 \
OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/text_context.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --annotations "$PRIVATE_ANNOTATIONS" --snapshot "$INSTALLED_MODEL_SNAPSHOT" \
  --output "$PRIVATE_REPORT"
```

The installed snapshot must contain `onnx/model_quantized.onnx` and
`tokenizer.json`. Reports pin their SHA256s, code/lock hashes, runtime versions,
source/annotation fingerprints, stored-versus-re-embedded drift, inference/token
cost, elapsed time and peak process RSS. These are private reports; source texts
are not committed. This is an offline development comparison of linear user
contexts with one-user lookahead, not native branch resolution or assistant-context
selection. Re-embedded nonempty unembedded turns can also differ from stored-vector
coverage; report that when extending the cohort.

On the existing four-thread pilot, re-embedded single turns yield six detected
changes and 15 false splits; concatenated blocks yield six and 17. This does not
improve boundaries at the fixed 0.55 threshold. The re-embedded singleton control
differs from stored vectors and is essential to interpretation. Results, costs
and limitations are recorded in the living paper.

## Expanded development source review

`review_packets.py` samples two canonical development groups from each provider ×
length stratum: short (3–10 nonempty searchable user records), medium (11–40).
It excludes prior annotation source groups and reserved groups, caps each turn at
4,096 characters and each thread at 40,000 UTF-8 bytes, and ranks candidates by
SHA256 of the fixed seed plus canonical group identity. Selection never reads
candidate model outputs. Insufficient strata fail explicitly. This bounded
sampling is deliberately not a representative corpus estimate.

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/review_packets.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --exclude-annotations "$PRIOR_ANNOTATIONS" --output "$PRIVATE_PACKETS"
```

Packets contain the complete selected searchable user sequence, native IDs,
ordinals, source hashes and stored-vector availability. The display guard redacts
standalone credential-like values without changing the source database or source
hashes. It can redact ordinary opaque identifiers and is not a complete secret
scanner. Review remains private; do not print entire packets or treat redacted
values as topic evidence. Preserve original source identity for annotation checks.

The expanded review adds eight conversations/84 records. It freezes source-only
admission and major/secondary subject judgments before model scoring, alongside
76 boundary judgments (40 known, 36 uncertain). The source review sidecar pins
packet/guideline hashes and supporting ordinals. One coding assistant reviewed
the searchable user text, with prior corpus familiarity; full assistant/native
branch context and independent human review remain incomplete. Detailed aggregate
procedures, counts and unchanged-parameter baseline results are in the paper.

## Preceding conversational context

`preceding_context.py` produces private source-pinned review packets for every
visible user anchor in a frozen development annotation file. Claude records follow
the snapshot's native parent ordinals; linear records follow preceding stored
order. Missing, forward, mixed or ambiguous ancestry fails rather than falling
back to a nearby assistant message. No future or sibling records are borrowed.
The packet walks at most 64 raw records to the previous visible user or root.
It displays only searchable user/assistant prose, with 16 KiB per-record and
32 KiB total display bounds, explicit omissions and the existing display guard.
Tool/developer text is not displayed; hashes of traversed canonical/searchable
records still make context edits detectable. Outputs must be outside Git, cannot
overwrite existing files, and stdout contains only aggregate counts.

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/preceding_context.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --annotations "$PRIVATE_ANNOTATIONS" --output "$PRIVATE_CONTEXT_PACKETS"
```

This verifies traversal of stored edges, not independent native-file fidelity or
semantic referent resolution. Reaching a previous user record does not prove that
the referent is resolved: that record may itself be an approval or generated frame.
Linear histories may contain inherited or compacted context. The source guard is
not comprehensive secret scanning, so do not print entire packets. Keep existing
user-only judgments frozen; any full-context judgments need their own evidence
scope. No topic/search input or production admission behavior changes here.

For chained replies, `--maximum-users 3` follows up to three preceding searchable
user records along the same causal path. The 64-record traversal and display
budgets apply to the entire walk, not separately to each interval. The default
one-user context pins remain unchanged. Three-user pins include the user limit;
neither mode treats a reached user bound as semantic reference resolution.

The expanded development capture contains 84 contexts: 56 reach the three-user
bound, 22 reach a root and six exhaust the raw-record budget. There are no display
omissions; the largest display is 11,727 UTF-8 bytes. A fresh capture reproduces
the packet exactly. This is stored-projection context: the earlier native audit
verified anchors and Claude anchor edges, not every preceding message body.
Separate source-pinned reference judgments are still required before contextual
embedding experiments. Do not turn reviewer-written interpretations into model
inputs or change the frozen user-only judgments.

## Source-backed reference judgments

`reference_review.py` validates a separate private annotation sidecar against the
exact chained-context and original source-review bytes. It requires every one of
the originally referential anchors exactly once, preserves anchor/source hashes,
uses a closed resolution enum and requires source evidence for resolved cases.
Evidence must match intact, unredacted display text and an identity on the
captured causal path. Sibling/future/absent evidence, omissions, edited hashes,
duplicates and incomplete annotation sets fail validation. The command prints
only aggregate counts; it does not generate representations or topic labels.

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/reference_review.py \
  --contexts "$PRIVATE_CHAINED_CONTEXT" --source-review "$PRIVATE_SOURCE_REVIEW" \
  --annotations "$PRIVATE_REFERENCE_REVIEW"
```

Focused source review classifies the 29 referential anchors as 20 resolved tasks,
three resolved metadata references, four interaction prompts and two unresolved
references. A reported number denotes a count of unresolved findings rather than
an issue identifier; named old sessions are selectors rather than evidence that
their topics are currently discussed. Corrections must not inherit rejected
entities or all assistant proposals. This provides evaluation targets for later
context selection, not measured topic improvement. The single coding-assistant
reviewer had prior source, method and prediction exposure; these are exploratory
judgments, not independent human truth. Preceding bodies use stored projections,
not comprehensive native verification. Original user-only judgments remain frozen.

## Causal context-selection controls

`context_selection.py` evaluates three fixed selectors: nearest assistant prose
within the captured chain, the preceding exchange through its nearest user, and
the full captured three-user chain. Selectors consult only roles, causal order
and display availability, never reference labels, evidence indices or rationales.
The nearest-assistant selector abstains if that assistant record is omitted or
redacted; it does not jump over that record to an older answer. Exchange boundaries
still respect omitted or redacted users. Original reference labels are used only
to score selections afterward. Selection itself does not resolve or assign topics.

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/context_selection.py \
  --contexts "$PRIVATE_CHAINED_CONTEXT" --source-review "$PRIVATE_SOURCE_REVIEW" \
  --annotations "$PRIVATE_REFERENCE_REVIEW" --output "$PRIVATE_SELECTION_REPORT"
```

| Selector | All annotated evidence retained / 23 resolved cases | Evidence records / 31 | Text bytes across 29 anchors | Additional records beyond annotated evidence |
| --- | ---: | ---: | ---: | ---: |
| Nearest assistant | 15 | 23 | 25,698 | 4 |
| Preceding exchange | 21 | 28 | 29,592 | 43 |
| Captured chain | 23 | 31 | 91,617 | 194 |

The chain's complete retention follows by construction: the annotations were
made within that capture. Additional records are not necessarily irrelevant;
retaining evidence is not semantic resolution. All selectors also supply context
to some interaction/unresolved cases (four nearest-assistant, five exchange/chain
of six), so indiscriminate topic inheritance remains unjustified. The cohort uses
oracle original referential admission, not an automatic eligibility classifier.
Selection for all 84 captured contexts takes under 0.5 ms in these two materialized
packet runs; capture, encoding, persistence and ingestion are excluded. No model
calls occur. Repeated non-timing fields are identical; 53 lab tests pass. Next
compare source-only contextual representations and semantic quality, without
feeding reviewer-written interpretations into models or tuning on reserved groups.

## Source-only contextual embedding pilot

`context_embeddings.py` compares re-embedded user-only text, a joined preceding
exchange plus anchor, and an equal mean of separately encoded normalized record
directions. Context is applied only to originally referential anchors according
to the frozen oracle admission sidecar; source-backed resolution labels and
reviewer rationales never influence input selection. Content anchors stay
user-only. Generated framing/sensitive/attachment anchors remain barriers in the
original sequence. This is not an automatic admission or referent classifier.

Use the same database/partition/boundary/source-review inputs as the lexical
controls, plus `--contexts`, `--reference-review`, `--snapshot` and a fresh private
`--output`. Assets must already be explicitly installed; no download or LLM call
occurs. The exchange permits at most four preceding prose records. Missing,
redacted, over-cap or record-limit-incomplete exchanges abstain. Tokenization has
truncation disabled; inputs exceeding 512 tokens are rejected before inference.
For record means, any oversized constituent rejects the whole representation.

| Representation | Encoded anchors / 68 | Known scored / 40 | Common AP | Common ROC AUC | Fixed cosine .55 TP / FP / abstentions |
| --- | ---: | ---: | ---: | ---: | ---: |
| User only | 66 | 38 | .561 | .697 | 11 / 25 / 2 |
| Joined exchange | 60 | 35 | .726 | .889 | 9 / 14 / 5 |
| Record mean | 61 | 36 | .720 | .897 | 10 / 10 / 4 |

The common comparison has 35 known boundaries with nine changes. Codex AP rises
from .465 to .852/.884, but Claude AP falls from .833 to .583/.583 on just two
changes. The unchanged fixed cutoff still has weak precision and loses changes
through abstentions: recall including abstentions .917/.750/.833. This is a
promising context-representation signal, not robust semantic topic quality or a
method/default selection. Frozen boundaries are user-only exploratory judgments;
overlapping context can also create artificial continuity and inherit unendorsed
assistant proposals. Full-dialogue judgments and independent review are needed.

Two fresh runs reproduce every non-measurement field. Across all methods, the
shared bounded cache performs 121 unique encodes and 120 cache hits, with a maximum
475 tokens actually encoded. In the final run, inference takes 926 ms, elapsed
after imports 1,731 ms and process peak RSS 751.2 MiB. Sequential cache reuse means
new-call counts are not standalone method costs. Timing excludes production
ingestion/persistence and does not establish the <=10% overhead target. Record
means may permit reuse of compatible existing vectors later; that reuse is not
implemented or verified here. Fifty-seven lab tests pass, including rejection
before inference, context integrity and cancellation abstention.

## Oracle-subject label controls

`subject_labels.py` isolates label extraction using the 18 non-uncertain reviewed
subjects and their source-support ordinals. Subject names and reviewer rationales
never enter the vocabulary. This is oracle overlapping support, not clustering,
assignment quality or a production topic-name generator. The background is the
same 68 admitted development user records, each counted once. Supporting records
are counted separately and summed per subject, avoiding invented bigrams across
message boundaries. Overlapping subjects can count a record in multiple classes.

The fixed controls use English-stopword-filtered technical unigrams/bigrams,
an 8,192-feature cap and top-eight keywords. Baseline c-TF-IDF uses the earlier
formula. The second control multiplies by smoothed source-record IDF,
`1 + log((1 + N) / (1 + df))`; the third greedily skips keywords whose token sets
contain or are contained by an already selected keyword. This lexical control
does not implement semantic MMR, typo correction or noun-phrase extraction.

Run with the database/partition/boundary/source-review arguments used above and
a fresh private output. The 1,279-feature vocabulary produces 138 baseline
keywords across 18 subjects. Corpus IDF changes the top-eight set for only three
subjects and order for four. Containment pruning reduces the output to 126
keywords but drops useful compounds such as `remote control` after retaining
its individual words. Conversational labels such as `accept suggestions` persist;
two distinct subjects sharing the same source record have identical labels.
These are exposed single-reviewer qualitative failure observations, not measured
semantic precision or independent review. Rarity alone does not identify useful
domain terms, and statistical weighting cannot separate subjects represented by
the same undivided source record. Neither adjustment is promoted.

Two fresh runs reproduce all non-timing fields. Fit and labeling take 20–22 ms;
the final elapsed time after imports is 462 ms, including frozen corpus loading.
No inference calls occur. These timings do not measure ingestion or online
vocabulary/label maintenance. English stopwords can also suppress domain terms.
Next test source-supported contextual/episode representations and phrase quality
before adding noise filters or selecting a discovery method.

## Source-preserving phrases and YAKE

`phrase_labels.py` uses the same frozen oracle supports to compare source-adjacent
c-TF-IDF, a fixed phrase-length multiplier with containment pruning, and the
official YAKE 0.7.3 implementation. YAKE uses local text statistics without a
trained model. Its dependency is isolated in this research project's lockfile;
no Rust dependency, model download or production behavior changes.

The source analyzer treats stopwords and non-whitespace punctuation gaps as
boundaries rather than deleting them before forming bigrams. Every returned
keyword must occur as a casefolded, whitespace-normalized phrase within an
individual supporting source record. This avoids cross-record or skipped-word
phrases. It does not establish semantic correctness or endorsement. YAKE uses
English, maximum two words, sequence deduplication .9, window 1 and top 32
candidates; source-supported candidates retain YAKE order and are capped at 8.
Supporting records are separated by newline-period-newline, affecting YAKE's
position statistics. Source documents are capped at 64 KiB. Length weighting is
a fixed heuristic, not RAKE or semantic MMR; RAKE motivates delimiter handling.

Run with the preceding label-control inputs, adding `--label-report` pointing to
the frozen `subject-labels-final.json`. The script verifies its source fingerprints
and exact subject supports. The original baseline has 36/138 keywords that are
not contiguous in their supporting records. The new 796-feature vocabulary emits
135 source-adjacent keywords, 123 length-weighted and 129 YAKE keywords. Four YAKE
candidates are rejected by source support. The length-weighted control restores
compounds such as `remote control`, but also elevates old session titles as topics.
YAKE yields some useful compounds but still ranks conversational approvals and
session selectors. Neither is promoted as a topic-quality solution. These are
exposed qualitative observations, not independent semantic precision estimates.

Two fresh runs reproduce non-measurement fields. Source features take 4–6 ms and
combined ranking, YAKE and support checks 176–202 ms for 18 subjects; final elapsed
after imports is 691 ms including corpus load. The verified report includes the
baseline contiguity audit counts. No inference occurs. Full ingestion
overhead and online label maintenance remain unmeasured. Sixty-five lab tests
pass, including stopword/punctuation boundaries, identifiers and source-support
checks that cannot combine records or skip words. Next improve source topical
units and metadata/endorsement handling before evaluating automatic discovery.

## Oracle resolution-aware label inputs

`contextual_labels.py` holds the source-adjacent c-TF-IDF extractor and 18 reviewed
subject supports fixed while comparing all admitted user supports, content-only
supports, and content plus bounded exchanges for oracle-resolved task references.
Metadata selectors, interaction prompts and unresolved references are excluded
from the last control. Previous user framing and referential records do not enter
its label units; intact preceding assistant prose does. The current task-reference
anchor remains present, preserving corrections rather than deleting their text.
Sources are deduplicated by thread/ordinal within subjects and in each background.
The shared `bounded_exchange` helper preserves the embedding control's whole
exchange/omission/record-cap behavior. There are no model calls.

Run with database, partition, boundary/source-review, contexts and reference-review
inputs as above and a fresh output. Oracle resolution labels gate source selection
in this diagnostic; no automatic classifier is claimed. Reviewer names/rationales
never become label text. The all-user baseline reproduces the preceding
source-adjacent labels and scores exactly.

| Label inputs | Retained anchors / 68 | Unique source units | Assistant units | Vocabulary | Keywords |
| --- | ---: | ---: | ---: | ---: | ---: |
| All admitted user support | 68 | 68 | 0 | 796 | 135 |
| Content only | 39 | 39 | 0 | 761 | 129 |
| Content plus resolved-task exchange | 58 | 86 | 28 | 2,094 | 139 |

Content-only changes five top-eight keyword sets; task context changes nine.
Focused qualitative review finds useful recovery of witness persistence,
cleanup/acquisition evidence and package-evaluation terms. It also imports a
rejected project name into the remote-control topic despite keeping the user's
correction. Some controls still return broad or conversational terms. Oracle task
resolution establishes a referent, not endorsement of all preceding text. No
method is selected or promoted; topical spans and endorsement/correction handling
remain necessary. Shared undivided support still cannot distinguish two subjects
within one source record.

Repeated non-timing fields are identical. Selection, fitting and labels take
roughly 7–21 ms per control in the first run; elapsed after imports is 517 ms,
including corpus load. These measurements do not establish ingestion overhead
or automatic semantic quality. Sixty-nine lab tests pass, including metadata
exclusion, source preservation, framing exclusion and incomplete-context abstention.

## Conservative lexical borrowing veto

The fourth `contextual_labels.py` control retains the current user anchor and
borrows no preceding text when an oracle-resolved task reference contains a
whole-word `no`, `not`, `never`, `instead`, `rather`, or an ASCII/curly `n't`
contraction. Matching is case-insensitive; identifier substrings such as `NoSQL`
or `notebook` do not trigger it. The marker set is a fixed exploratory control
with prior failure exposure, not a correction classifier. Other controls and
their keyword outputs remain unchanged. No phrase-specific continuation exception
is added after inspecting outcomes.

Two of 20 resolved-task anchors trigger the veto: one known project correction,
and one valid continuation beginning with `no`. The rejected project disappears
from that topic's top keywords, but the rule also withholds useful proposal
context from the continuation. Three annotated preceding evidence records are
not borrowed at the two anchors; this counts per-anchor selection, not loss of
all topic evidence, since another supporting anchor can retain a record. The
guard keeps 58 anchors, with 82 unique units including 24 assistants, versus
86/28 without it. Vocabulary falls from 2,094 to 2,029; both controls emit 139
keywords. Lexical polarity cannot establish which proposition the user rejects.
The rule is rejected as a robust endorsement solution; it is not promoted or
retuned around these examples. These exposed observations do not estimate
independent correction precision or downstream topic quality.

The private `contextual-labels-veto.json` capture reproduces all non-timing fields.
In the first run the guard's selection/fitting/labeling takes 20 ms; total elapsed
after imports is 527 ms for all four controls, including corpus load. No inference
occurs and the ingestion overhead gate remains unmeasured. Seventy-one lab tests
pass, including correction anchor preservation and contraction/identifier matching.

## Exact source structural spans

`source_spans.py` captures offset-based units for the 68 admitted user anchors and
the original reference review's intact preceding evidence. Identities are merged
by thread/ordinal only when hashes and text agree. The frozen scope contains
91 records: 68 users and 23 assistants. The report stores source identities,
Unicode code-point offsets, kinds and exact UTF-8 substring hashes, without
duplicating source bodies. No text is rewritten or interpreted as a topic.

The fixed structural splitter emits paragraphs, list items, Markdown table body
rows and headings. Table headers/separators are framing. Fenced code, including
an unclosed fence, remains a distinct indivisible block, so table/list syntax
inside code does not create false units. Boundary whitespace is trimmed with
offsets preserved. This is a limited structural splitter, not general Markdown
parsing or semantic segmentation: list continuations can remain separate prose,
inline lists/sentences are not split, and related units may need grouping.

Run with database, partition, boundary/source-review, contexts and reference-review
inputs as above. The 91 records produce 257 candidates: 166 paragraphs, 66 list
items, 18 table rows, 4 code blocks and 3 headings. In the accepted six-family fix
proposal, all six table rows are distinct candidates; the legacy-import proposal
has six paragraphs plus two list items. The wrong-project response has three
paragraphs, separating the project statement from session/access metadata. Its
project statement still contains the rejected entity: structural narrowing alone
does not establish endorsement or remove that contamination. These are integrity
and structural observations, not independent topic-quality measurements.

Two fresh captures reproduce all non-timing fields. Segmentation plus span hashes
and metadata take 4.4–4.6 ms; final elapsed after imports is 481 ms including corpus
loading. No inference occurs, and ingestion overhead remains unmeasured.
Seventy-six lab tests pass, including Unicode/CRLF offsets, table framing,
non-overlap, empty input and fenced-code preservation. Next annotate source-span
relevance/endorsement separately and test automatic selection and grouping before
any production topic promotion.

## Source-span similarity selection pilot

`span_selection.py` tests top-1/top-3 selection against keeping every structural
unit in the nearest intact causal assistant response. The three deliberately
selected hard cases above contain 25 candidates: eight legacy-import spans,
three project-correction spans and fourteen accepted-fix spans. A private,
source-pinned review assigns one closed relation per candidate: topical support
for the current reply, background, procedure, metadata, mixed rejection or a
rejected proposal. Mixed spans contain relevant subject matter and rejected
details; they are not clean evidence to borrow. These are exposed single
coding-assistant judgments, not independent ground truth or a representative
quality test. The accepted-fix review marks the six detailed rows and their
summary as seven support spans, not seven separate semantic topics.

Three source-only queries rank candidates with the existing offline MiniLM:
the current user reply, the preceding user turn, and the equal-weight normalized
mean of the captured preceding user turns plus the current reply. The chain uses
at most three preceding users and abstains on missing/redacted/edited/noncausal
source. Every input must fit the 512-token model window; there is no truncation
or partial ranking. Scores use cosine and source-order tie-breaking. Review
relations are used only after ranking, never as encoder inputs or selection rules.
The validator pins context/capture bytes and exact candidate hashes, rejects
missing/reordered/unknown judgments and extra fields such as reviewer summaries,
and rebuilds source offsets against intact causal records. The CLI takes the
usual frozen database/partition/boundary/source/context/reference inputs plus
`--spans`, `--pilot`, explicit installed `--snapshot`, and fresh `--output`.

The simple selector is rejected. For the legacy-import correction, none of the
three top-3 queries selects the clean flag-status paragraph. The current query
selects the rejected proposal, mixed rejection and background; the preceding
query selects only background; the chain selects mixed rejection, the rejected
proposal and background. For the project correction, current top-1 selects access
metadata while preceding top-1 selects the mixed wrong-project statement. The
chain abstains because an older user record is redacted. For the accepted fixes,
every top-3 consists of the family summary and two procedural units, omitting all
six detailed rows. This is a span-selection failure diagnostic, not measured
downstream label or topic recall: the summary itself mentions the six families.

Two fresh runs reproduce every non-timing field, including all rankings and
abstention. The shared cache requires 35 unique singleton encodes, 51 hits and
at most 79 tokens per encoded input. Inference takes 94–97 ms across all queries;
elapsed after imports is 679–690 ms including source loading and model setup;
peak process RSS is 223–228 MiB. These shared-cache lab measurements do not prove
standalone query costs, production model parity, or the ingestion gate.
Eighty-two lab tests pass. Keep this negative control rather than tuning top-k or
adding phrase exceptions on these examples. The next decision needs wider
source-reviewed evidence for grouping and topic relevance separately from
endorsement; similarity alone does not supply either.

## Native source fidelity audit

`native_audit.py` checks the same frozen development anchors against their current
native files without printing or storing source bodies in its report. It
reconstructs the application's BLAKE3 message IDs from Claude thread/UUID pairs
or explicit Codex raw-message and completed turn/item IDs. It requires exact role
and canonical-text SHA256 equality; a text-only match is insufficient. Unsupported
canonical tool/thinking forms and conflicting replay identities are explicit
failures. The completed-message subset ignores image blocks according to the
typed ingestion schema; it does not guess image contents or reconcile mirrors.

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/native_audit.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --annotations "$PRIVATE_ANNOTATIONS" --output "$PRIVATE_NATIVE_AUDIT"
```

Each source is limited to 64 MiB, 100,000 records and 8 MiB per record. Re-reading
exact bytes catches changes during capture. Reports pin whole-file/record hashes,
source IDs, annotation/code/lock hashes and recognized origin-field presence.
Claude anchor parent links are checked through at most 64 native metadata edges
to the closest represented message or root. Missing or conflicting edges are
reported. Codex linear ancestry is explicitly unverified. Current native bytes
need not equal the historical whole file used to create the snapshot; individual
matched records are the evidence. Native origin metadata does not establish
substantive intent, and source fidelity does not establish semantic topic quality.

All 84 expanded-review anchors match native identity, role and canonical text;
one requires a completed Codex attachment event. All 40 Claude anchor edges match
(36 parents, four roots). The report is byte-identical on a fresh repeat. The
existing admission and boundary judgments remain unchanged. Full preceding-chain
content verification, source-supported referent resolution and production
eligibility rules remain subsequent work, tracked in Beads and the paper.

## Lexical boundary ranking control

`lexical_boundaries.py` compares adjacent dense cosine, disjoint two-turn dense
means, adjacent word TF-IDF cosine, disjoint two-turn lexical means and a fixed
equal dense/lexical adjacent combination. Scores are dissimilarities; higher
means a more likely change. It does not tune a decision threshold or emit topic
tags. The lexical-cohesion motivation is related to TextTiling, but these controls
do not reproduce its token-sequence, smoothing, depth-score or paragraph policy.

```bash
PYTHONDONTWRITEBYTECODE=1 OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/lexical_boundaries.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --annotations "$PRIVATE_ANNOTATIONS" --source-review "$PRIVATE_SOURCE_REVIEW" \
  --output "$PRIVATE_LEXICAL_REPORT"
```

Frozen source-reviewed admission is an **oracle diagnostic**, not a production
classifier: only content/referential records enter TF-IDF fitting. All original
boundary positions remain; an inadmissible block member or missing/cancelling
vector causes abstention rather than bridging framing. The sidecar must match
every source ID, ordinal and text hash. TF-IDF is fitted transductively to the 68
admitted development records with unigrams/bigrams, lowercase, sublinear TF,
smooth IDF, L2 normalization, no stopword list, an explicit technical-token regex
and an 8,192-feature cap. This is not a safe incremental IDF-coordinate policy.

Reports give non-interpolated average precision, ROC AUC, prevalence and coverage
on known boundaries, plus common-support and provider breakdowns. Uncertain
judgments are excluded; abstentions are counted explicitly. Common support avoids
comparing rankings over different scored subsets. Two-turn blocks use one future
user turn and are user-only linear representations, not native-branch context.
No LLM or model inference is invoked. Source/annotation/code/lock/feature hashes
are pinned; vocabulary/source text is withheld. Fit/scoring costs and full-process
RSS do not measure complete ingestion overhead. Results and limitations are in
the living paper; no production method is selected from this small diagnostic.

## Group-aware dense cutoff calibration

`calibrate_boundaries.py` tests one fixed operating rule: maximize training
balanced accuracy (mean change recall and continuation specificity), breaking exact
ties toward the higher dissimilarity cutoff/fewer splits. Candidates are training
score midpoints and endpoints only. A split requires dissimilarity strictly above
the selected cutoff. Every canonical development group is excluded in turn from
all training scores/labels, including any alias thread; insufficient training
classes cause abstention. Uncertain judgments and missing scores never choose a
cutoff. The comparator uses dissimilarity >0.45 (cosine <0.55).

```bash
PYTHONDONTWRITEBYTECODE=1 OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/calibrate_boundaries.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --annotations "$PRIVATE_ANNOTATIONS" --source-review "$PRIVATE_SOURCE_REVIEW" \
  --output "$PRIVATE_CALIBRATION_REPORT"
```

This uses existing dense vectors and the frozen oracle-admission diagnostic only;
no lexical vocabulary/IDF is fitted and no inference is invoked. Report each
training/test group, cutoff, available training classes, out-of-group predictions,
provider metrics and abstentions. Do not replace fold results with a cutoff fitted
to all labels. Leave-group-out development results are still exploratory because
the cohort and single-assistant annotations have informed method choice. This is
cutoff selection, not probability calibration, semantic-topic validation or the
complete-ingestion performance gate. The operating rule is fixed before this
experiment's predictions; any later alternative needs a separately recorded trial.

On the expanded development review, out-of-group calibration reduces false splits
from 26 to eight but detects only five of twelve changes, missing seven in Codex.
This tradeoff is rejected; no cutoff is promoted or retuned after the result.
The paper records fold concentration, source-label limits and the next unit/context
review. Reserved groups remain untouched.

## Initial controls

`run.py` reads an existing consistent SQLite snapshot in read-only mode. It uses
stored compatible 384-dimensional quantized **user** embeddings, normalizes each
vector and averages unique searchable user texts per thread. Canonical duplicate
threads are represented once. A frozen Rust report supplies generation and source
group identities only: its cleaned topic vectors are **not** reused. Groups whose
first identity byte modulo five is zero form the reserved partition.

These are raw geometry controls. They retain conversational scaffolding and do
not perform inference, source cleaning, episode segmentation, overlapping topic
assignment, or semantic parent construction. BIRCH is scikit-learn's reference,
not a reproduction of Rust routing. HDBSCAN and average linkage are offline
comparators. HDBSCAN's `-1` fit labels remain noise. A common cosine-centroid
assignment diagnostic at 0.7 is neither HDBSCAN prediction nor calibrated
confidence. The label-only unigram/bigram c-TF-IDF control uses training texts;
stop words never rewrite embedding inputs. Initial parameters are controls, not
validated defaults or a selected winner.

Each fresh process emits a private JSON report with native member IDs, terms,
input/reference SHA256 fingerprints, versions, parameters, timing and RSS.
Keep reports and source material outside Git. Fingerprints include the exact
reference report bytes; use the same artifact across a comparison. Timing for
fit and labels excludes loading and interpreter startup; peak RSS includes them.

## Online hierarchy controls

`online_tree.py` provides an independent GRINCH-style Python control based on
the paper and [pinned authors' Scala reference](https://github.com/iesl/grinch/tree/a2747d3c5d3405cf27afc839bc28352b99c3e309).
It is not an execution of the reference or a byte-parity reproduction. Compare
greedy nearest-leaf insertion, insertion with rotations, and insertion with
rotations/grafting/post-graft restructuring. Neighbor search is exact over leaves;
graft queries exclude the current subtree. Linkage is cosine between sums of
normalized input vectors. Rotations use pre-insertion ascent, grafts require
strict mutual preference, and restructuring stays below the post-graft LCA.
Ties preserve arrival order in leaf search and serial order in restructuring;
equal graft preferences ascend instead of stalling. Float64 and an epsilon-sized
zero-sum direction are explicit differences from the Scala float32 implementation.
There is no ANN, graft height/size cap or single-elimination approximation. A
2,000-leaf lab bound fails closed; it is not a production scalability result.

`grinch_controls.py` compares these three trees with online BIRCH
(`partial_fit` one point at a time, radius 0.4, branching factor 64, no global
clustering). Use `--source sessions` with the frozen database and partition: all
538 development thread means are inserted and 126 reserved groups excluded.
No source text is rewritten and no new inference occurs. The three arrival orders
are forward, reverse and fixed shuffle seed 17. A separate synthetic control has
96 normalized path vectors in six disjoint coordinate blocks, with class labels
used only in evaluation. Synthetic purity does not establish real topic quality
or prove that the paper's separation assumptions hold for our linkage.

Fit timings include incremental updates; whole-tree validation, cuts, signatures
and evaluation are reported separately. Every live node is checked for binary
shape, exact membership/parent links, reachable identity and cached vector/count
consistency. Tests validate after each insertion, including random inputs,
identical/canceling vectors, five synthetic path arrival orders and a deliberate
post-graft restructuring fixture. Internal nodes are geometric clades, not
verified semantic parents or inherited topic tags. The prototype is insert-only;
edits/deletes, restart persistence, multi-topic membership and source-unit quality
remain unimplemented.

For order comparison, captures include nontrivial clade hashes and the closest
pair in 4,096 fixed canonical triplets (seed 20261001). Native tree cuts require
every internal merge in an accepted clade to have cosine at least 0.7. This
handles linkage inversions explicitly but is an uncalibrated operating point,
different from BIRCH's radius; neither cluster count nor cut agreement measures
semantic quality. Synthetic dendrogram purity uses all same-class distinct pairs,
without consulting class labels during insertion. Real-session purity remains
unset because there are no corresponding independent single-class labels.

Run each method/order in a fresh process with one numerical thread and explicit
`--reference-manifest` pointing to the privately captured authors' source hashes:

```bash
OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/grinch_controls.py \
  --source sessions --method grinch --order forward \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --reference-manifest "$PRIVATE_REFERENCE_MANIFEST" --output "$PRIVATE_REPORT"
```

The manifest is pinned to the reviewed authors' commit and its file hashes are
rechecked; that source is never executed. Reports pin implementation/lock hashes,
generation, canonical partition, arrival identities and library versions. Keep
source identities and memberships in private artifacts outside Git. No method
may be promoted from structural correctness, synthetic purity or fit-only speed.

The [48-capture experiment](../../benchmarks/2026-10-01-grinch-online-controls.md)
finds stronger GRINCH-style order diagnostics than rotation alone and perfect
purity on the shuffled synthetic control, at higher exact-search cost: 641–1,392 ms
for 538 threads versus 197–232 ms for BIRCH. No real-topic purity or method winner
is established. All 24 fresh-run pairs reproduce non-timing fields. Run
`compare_grouping.py --captures "$PRIVATE_CAPTURE_DIRECTORY" --output "$FRESH_PRIVATE_SUMMARY"`
to validate paired results and recompute descriptive order diagnostics; source,
code, parameter, partition, sampling and membership mismatches fail closed.

## Reproduction

Install the locked lab environment outside the repository:

```bash
UV_CACHE_DIR=/home/james/scratch/mash-artifacts/uv-cache \
UV_PROJECT_ENVIRONMENT=/home/james/scratch/mash-artifacts/topic-python-env \
uv sync --locked --project research/topics
```

Use explicit frozen input paths and a fresh output path:

```bash
OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python research/topics/run.py \
  --database "$FROZEN_DATABASE" --partition-report "$FROZEN_PARTITION_REPORT" \
  --method birch --holdout --output "$PRIVATE_REPORT"
```

Compare `birch`, `hdbscan` and `average`, with/without `--holdout`, and with/without
`--reverse`: twelve separate-process controls. Keep CPU thread limits fixed.
The loader fails on generation/dimension mismatch, missing canonical identities,
invalid vectors or a live WAL. It never creates or modifies the source database.

The first twelve controls and their limitations are recorded in the
[results](../../benchmarks/2026-10-01-python-topic-controls.md). They do not select
a winner; independently reviewed dialogue units and topic labels come next.

The [matched source-unit experiment](../../benchmarks/2026-10-01-matched-source-unit-grouping.md)
compares exact spans with pooled record/thread directions across dense, fixed
lexical and hybrid geometry. `span_grouping.py` requires explicit frozen database,
partition, annotation, source-review, context, reference-review, structural-span,
pair-review and installed snapshot paths, plus a fresh private output path.
`compare_spans.py --first ... --second ... --output ...` verifies the complete
72-control grid and every nonmeasurement field. Reviewer text never enters
features. This is exposed development evidence; fixed clustering cuts still
miss most reviewed overlaps and no method is promoted.

The [bounded-fragment experiment](../../benchmarks/2026-10-01-bounded-fragment-aspects.md)
uses the same source/pair inputs in `fragment_experiment.py` to compare original,
sentence/semicolon and overlapping-window vectors. `compare_fragments.py` verifies
all24 controls and nonmeasurement fields in two fresh captures. Mechanical
splitting lowers pair-ranking quality and is not promoted. Offset-preserving
fragments do not infer endorsement or grammatical negation scope.

The [Rust port contract](PORT_CONTRACT.md) defines the required reference/parity
gates before implementation. A candidate has not been selected, and the
production differential CI gate is not implemented yet.

The [fresh review/payload pilot](../../benchmarks/2026-10-01-feedback-payload-controls.md)
samples four unseen development source groups, preserving prior eligibility and
excluding original/v2 review aliases. `review_packets.py` supports an explicit
`--seed`, bounded `--per-stratum`, and repeated `--also-exclude-annotations` paths;
it pins every exclusion, partition and source-module hash. Do not resample based
on model outputs. `packet_review.py` validates complete source-origin judgments.
`payload_experiment.py` compares nine oracle-reviewed UI tasks' intact text versus
an exact known feedback envelope, on equal model-window support.
`compare_payloads.py` verifies both modes/all16 controls and nonmeasurement fields.
It reduces false merging at fixed cuts but loses genuine overlaps and does not
improve pair ranking. No general context classifier or topic method is selected.

The [fixed-membership field-label controls](../../benchmarks/2026-10-01-feedback-field-labels.md)
use `field_labels.py --packets ... --review ... --memberships ... --output ...`
and `compare_field_labels.py --first ... --second ... --output ...`.
The prior four memberships/eight sources stay fixed across full-payload,
target-inclusive and feedback-only labels. Full-payload baseline terms/scores
must reproduce exactly. Field filtering removes metadata candidates on this
exposed schema; feedback-only loses target referents. This is not independent
semantic-quality evidence. Captures retain both Linux VmHWM and OS rusage peaks
because the counters disagree in this environment; historical rusage-only
memory attribution requires remeasurement before a performance gate.
The [memory audit](../../benchmarks/2026-10-01-topic-memory-accounting.md)
reproduces pre-exec rusage retention and externally remeasures field/payload
experiments without changing algorithm outputs. Run `memory_audit.py --output
PRIVATE.json -- PYTHON research/topics/RUNNER.py ...` for Linux target-only
20 ms RSS/PSS sampling. Sampled peaks are lower bounds; timings may be perturbed.
This does not replace complete-ingestion or child-process measurements.

The [fresh referential context audit](../../benchmarks/2026-10-01-fresh-referential-context-audit.md)
uses `packet_contexts.py --database ... --partition-report ... --packets ...
--review ... --output ...` to validate complete original source groups, capture
prior reviewed referential anchors and separately audit current native identities.
The original three-user/64-record limits remain unchanged. Eighteen of21 intervals
hit the raw-record limit despite small displayed text; this motivates a separate
tool-aware retrieval experiment. Native identity verification is not full causal
order or semantic reference verification, and no resolution judgments are claimed.

The [visible-dialogue budget experiment](../../benchmarks/2026-10-01-visible-dialogue-budget.md)
keeps the raw64 control exact and compares a separate16-visible/4096-raw traversal
ceiling. `dialogue_budget.py` requires the original capture as `--baseline`;
`compare_dialogue_budget.py` verifies complete repeats. `packet_reference_review.py`
validates source-backed judgments with the shared legacy evidence checker.
All20 reviewed task references already have cited evidence in raw64; the larger
window adds65% displayed bytes without an oracle evidence-availability gain.
Do not promote it as better topic quality. Automatic source selection is next.

The [stored-vector record-selection diagnostic](../../benchmarks/2026-10-01-stored-reference-record-selection.md)
uses `record_selection.py` and `compare_record_selection.py` to compare recency
with three cosine query controls on unchanged raw64 contexts, without inference.
Token-window support and abstentions are explicit. Recency wins cited-record
retrieval but fails some narrowing/deferred references; retrieving a rejected
proposal is not endorsement. Fine accepted-passage selection remains unimplemented.

The [two-axis passage diagnostic](../../benchmarks/2026-10-01-two-axis-passage-scope.md)
uses `passage_scope.py` for exact source/gap coverage and Unicode/UTF-8 coordinates,
then `passage_ranking.py` and `compare_passage_ranking.py` for repeatable controls.
It separates topicality from requested scope: rejected work can remain topical,
and mixed paragraphs cannot become fabricated accepted evidence. Six source pairs
from four known failures are reviewed completely; other candidate records remain
unreviewed. Current-user cosine helps some top3 results but misses concrete details
at top1; no automatic selector or production topic method is selected.

The [bounded section-association control](../../benchmarks/2026-10-01-section-associations.md)
uses `section_links.py` and `section_experiment.py` to reuse verified passage ranks
without new inference. Explicit layout introductions link to at most four original
list/table children, with source provenance, caps, deduplication and fixed unique-unit
budgets. Included-unit counts improve for current-user queries but greedy expansion
loses other requested aspects and sometimes retrieves generic criteria. Wider
coverage and section-diverse allocation remain open; no production method is promoted.

The [section-allocation/target-coverage diagnostic](../../benchmarks/2026-10-01-section-target-coverage.md)
adds opt-in `section_experiment.py --one-child` and evaluation-only
`target_coverage.py`. It preserves exact original controls and distinguishes
included-unit counts from20named target aspects. Both section controls cover8/20,
but distribute coverage differently and miss all10reorganization contracts.
No selector is promoted. [External evaluation preparation](EXTERNAL_EVALUATION.md)
pins a MultiWOZ development shard and records metadata/provenance traps before
using its state/service annotations as explicit proxies. The
[external context comparison](../../benchmarks/2026-10-01-external-dialogue-context.md)
uses `multiwoz.py` and `external_context.py` for exact typed source validation,
label-separated inputs and repeatable cross-dialogue service-proxy evaluation.
Bounded preceding context improves proxy AP on common support; topic-label quality,
semantic hierarchy and production performance remain unverified.
The [intent-specificity follow-up](../../benchmarks/2026-10-01-external-intent-specificity.md)
uses `external_analysis.py` to revalidate archived vectors and compute paired
whole-dialogue resampling ranges without inference. Context hurts find/book
separation within shared services; broad-domain gains alone cannot select a
topic representation. Declared intent labels remain evaluation-only.
The [multi-view/length-gate experiment](../../benchmarks/2026-10-01-multiview-context.md)
uses `multiview.py` and the same archived vectors/draws. Fixed concatenations and
token-count gating reduce fine-intent loss but do not eliminate it; no method is
promoted. The shared evaluation summary accepts explicit controls and verifies
every original baseline metric/paired replicate without new inference.
The [lexical-context follow-up](../../benchmarks/2026-10-01-lexical-context.md)
uses `lexical_context.py` with disjoint unlabelled development background and
frozen TF-IDF/current/joined cosine fusion. Current fusion improves the fine-intent
point score slightly but broad/fine tradeoffs remain; no production method is
selected. Background text/group exclusions, sparse features and IDF are captured.
The [actual online grouping diagnostic](../../benchmarks/2026-10-01-external-online-clustering.md)
uses `external_clustering.py` with five frozen representations, singleton BIRCH
and GRINCH-style insertion, and three arrival orders. Existing uncalibrated cuts
produce mostly singletons and very low cross-dialogue proxy recall; perfect order
stability in some views is therefore weak evidence. No method is selected.
The tree retains its default1024D cap and permits an explicit bounded8192D setting
for these lexical controls. Production sparse costs and lifecycle remain untested.
The [separate-sample calibration](../../benchmarks/2026-10-01-cluster-calibration.md)
uses `cluster_calibration.py` in two phases:16 new calibration groups select
frozen settings, then16 reserved groups evaluate them without tuning. Calibration
selects joined BIRCH .8 and fixes fragmentation, but reserved forward intent
precision is37% and order stability is weak. No method is promoted; reserved
outcomes remain development proxies, not independent coding-topic gold.
The [cut-free hierarchy diagnostic](../../benchmarks/2026-10-01-cut-free-hierarchy.md)
uses `external_hierarchy.py` on those now-exposed samples. Smaller-LCA ranking
generally loses similarity information; grafting improves triplet stability but
exact branches remain unstable. Capture all canonical clades/pair LCAs and test
JSON round-trips. No semantic ancestor claim; linkage comparison is tracked in
`mash-c0d.19.2.32` before any Rust or MCP topic promotion.
The [average-cosine linkage ablation](../../benchmarks/2026-10-01-average-linkage.md)
uses `linkage_comparison.py` with explicit centroid-default/average options.
All90 old centroid decisions reproduce exactly;180-tree repeats match. Average
improves several AP/stability controls but has counterexamples and still weak
branch stability. Fresh independent validation/labels/lifecycle are required.
`Node.link_vector` supports both meanings; hierarchy captures now explicitly
record linkage in schema version2. No compatibility alias or production change.
The [precision-constrained calibration](../../benchmarks/2026-10-02-precision-calibration.md)
uses `precision_calibration.py` with another16 calibration/16 reserved groups,
excluding all64 previous groups. Per-linkage cuts maximize recall subject to
observed proxy precision≥.70 and20 links, with an explicit unsupported result.
Reserved execution requires the frozen calibration SHA and revalidates the full
contract. Average intent cuts are steadier across arrival orders; centroid has
higher forward recall. Secondary multiple-direct-tag thread scores approach an
all-connected baseline, exposing weak discrimination. No labels, semantic
ancestors, guided-search gain or production method are established.
The [programming-title transfer diagnostic](../../benchmarks/2026-10-02-programming-title-transfer.md)
uses `stackoverflow_source.py` to validate200 pinned public questions and separate
source inputs from community tag references. An annotation-independent64/64/72
split has sparse shared-tag pairs. `question_transfer.py` verifies the old frozen
calibration hash/algorithm pins and tests both intent cuts without tuning on new
outcomes. Both fragment badly despite raw cosine ranking signal; singleton-heavy
ARI and three-match perfect precision do not justify promotion. Bodies/answers
are unused; question tags are not full agent-session topic gold. Repeats and all
pair/count/ranking invariants are verified; source/captures stay outside Git.
The [source-grounded body comparison](../../benchmarks/2026-10-02-question-body-representations.md)
uses `question_html.py` for complete source/UTF-8/displayed-text provenance and
`question_representations.py` for full-body and fixed lexical/fusion controls.
All200 bodies extract, but51/128 selected joined inputs exceed the model window;
small matched ranking gains cannot establish whole-cohort quality. The separately
disclosed post-hoc `question_body_support.py` reuses saved sparse/title features
on all64 questions per phase without inference or refitting. Fusion gains are
small development evidence; no method is selected. Bounded units, compatible
vector reuse, fresh outcomes and full ingestion/semantic-quality gates remain.

The [bounded complete-unit diagnostic](../../benchmarks/2026-10-02-bounded-question-units.md)
uses `bounded_question_units.py` and `question_unit_experiment.py` to retain all
128 questions, with exact reuse of 77 cached vectors. Mean/max ranking gains
are mixed against cheap lexical fusion and require 407 additional calls.
Repeats and complete source/schema checks pass; no production method is selected.
Additional inference must earn its cost on fresh outcomes and full ingestion.

The [zero-inference stored-vector diagnostic](../../benchmarks/2026-10-02-stored-vector-reuse.md)
uses `stored_vector_reuse.py` on 128 actual coding threads with source-only
selection and a separate project-scope proxy. Recurring identical messages
confound maximum matching. A disclosed post-hoc `stored_frequency.py` control
penalizes exact-input document frequency without dropping source records and
improves exploratory ranking. Coverage is only 20.8% of original user records;
no independent semantic topic quality or production ingestion gate is established.
The subsequent [projection coverage audit](../../benchmarks/2026-10-02-projection-coverage.md)
corrects that denominator: all admitted nonempty projections have vectors;
canonical user-role counts include thousands of Claude tool results. Separate
`projection_coverage.py` ledger and `native_projection_audit.py` corroboration
retain exclusions and readiness state without source text output or index mutation.

The [complete-unit topic and label diagnostic](../../benchmarks/2026-10-02-question-unit-topics.md)
uses `question_topics.py` and `question_topic_labels.py` to compare saved unit
means with native multi-unit topic unions, using fixed transferred cuts and no
new inference. All 24 trees still fragment; overlapping thread memberships and
semantic ancestors are not established. A preregistered post-hoc
`question_label_balance.py` control fixes memberships and compares title-only
versus equal title/body label mass. Exact tag naming improves under a matched
eight-term budget, but is candidate-limited and not semantic label precision.
All source counts, partitions, scores, labels and repeat outcomes are verified;
252 tests pass. Label work dominates small tree fits in these controls. Full
ingestion and Rust/Python parity gates remain open; no method is selected.

The [label-ranking optimization](../../benchmarks/2026-10-02-label-ranking-performance.md)
profiles the frozen baseline before changing `subject_labels.ranked_terms`.
Positive-only bounded top-k selection preserves all original term/score outputs;
diversity pruning still scans all positive candidates. The explicit
`label_ranking_benchmark.py` audit reads the pinned committed reference function,
validates source/count contracts, and compares 40 frozen workloads. Matched
count/scoring/ranking work is about 7.5 times faster; this is not production
search or complete ingestion. Older capture guards stay strict. All 255 lab
tests pass; semantic quality and Rust parity remain open.

The [representation-specific question calibration](../../benchmarks/2026-10-02-question-representation-calibration.md)
uses `question_calibration.py` on only the original exposed calibration cohort.
Six source-only views, both linkages and nine cuts produce 108 candidates/324
partitions. None reaches the unchanged .70 precision/20-link gate, even forward
only. Do not relax the gate or evaluate a fallback winner on old reserved data.
Source/lexical/unit reconstruction, all pair counts, exact repeats and 259 tests
pass. This negative development calibration motivates a different representation
or discovery approach; it does not establish semantic quality or select a method.

The [overlapping lexical research and control](../../benchmarks/2026-10-02-overlapping-lexical-topics.md)
reviews TopicMapping, hierarchical document-word SBM and online LDA as distinct
non-LLM candidates. `lexical_memberships.py` tests literal source-term memberships
before graph inference: 24 frozen configurations fail the unchanged shared-tag
gate. Sources/counts/formulas, all pair decisions and exact repeats are verified;
263 tests pass. This baseline is not a paper-method replication, semantic
hierarchy or production online contract. Graph/mixed-membership discovery needs
its own experiment and independent semantic evaluation in `mash-c0d.19.2.35`.

Run lab input-boundary tests:

The [word-graph/mixture comparison](../../benchmarks/2026-10-02-word-graph-topics.md)
implements hypergeometric edge filtering, actual Infomap word communities and
bounded sparse PLSA refinement in `word_graph.py`/`graph_questions.py`. All 72
frozen settings fail the exposed calibration gate. Two processes repeat exactly
apart from timing; combinatorial probability and dense EM audits agree. This is
not TopicMapping replication, validated semantic hierarchy or an online contract.
Install its optional isolated research extra with `uv sync --frozen --extra graph
--project research/topics`, setting `UV_PROJECT_ENVIRONMENT` outside the repo.
Run `graph_questions.py --root ARTIFACT_ROOT --output FRESH_OUTSIDE_GIT_JSON`,
then repeat with `--compare FIRST_JSON`. Older experiments keep strict original
lock pins and require their recorded Git revisions. The new runner verifies the
archived baseline lock plus unchanged science packages; it does not relax those
older guards. No additional model inference occurs.

The [local-window comparison](../../benchmarks/2026-10-02-local-window-topics.md)
keeps the graph method and original question inputs fixed, changing only word
co-occurrence contexts to complete source spans of 16/32/64 pre-filter tokens.
`source_windows.py` retains Unicode/UTF-8 offsets and empty-evidence windows;
`window_questions.py` freezes all 216 settings and requires the whole-question
capture/code pins. None qualifies. Independent audits expose predominantly
single-parent edge support, even though each word occurs in multiple parents;
correlated windows do not create independent evidence. No inference occurs.
Run `window_questions.py` with the same `--root`, fresh `--output` and repeat
`--compare` arguments. This is not semantic episode segmentation or WNTM.

The [distinct-parent graph control](../../benchmarks/2026-10-02-replicated-topic-edges.md)
uses `parent_support.py` to deduplicate local-pair evidence within a question;
`replicated_questions.py` requires two distinct supporting parents and compares
original window weights against capped parent votes. All 432 frozen settings
fail the unchanged gate. Repeated runs, all support/weight/pair audits and six
independent dense-mixture replays agree; coverage loss is substantial. Run it
with the same fresh-output/repeat arguments. This is an exposed representation
diagnostic, not a production rare-topic exclusion rule or a new statistical null.

The [source-channel foundation](../../benchmarks/2026-10-02-source-code-prose-channels.md)
adds `question_channels.py` and `channel_audit.py`: explicit block-code versus
prose, inline identifiers preserved, original rendered text/ledger unchanged,
complete code-point/byte spans and unigram conservation. All 200 observed sources
pass; 104 boundary bigram omissions are recorded. No reference-quality outcomes
or model inference are used. Run the audit with the same fresh-output/repeat
arguments. Channel weighting and topic quality are still untested.

The [channel-weight comparison](../../benchmarks/2026-10-02-channel-weighted-topics.md)
uses `channel_weights.py` and `weighted_questions.py` for fixed title/body,
title/prose/code and title/prose mass controls. Counts, masks and denominator
changes are explicit; none of 216 settings passes the unchanged gate. Repeats,
all probability/pair audits and three independent dense-mixture replays agree.
Run it with the same fresh-output/repeat arguments. Source separation is useful
provenance but has not established useful topics. Prioritize independent topic
judgments/proxy validity before extending graph tuning.

The [duplicate retrieval preparation](../../benchmarks/2026-10-02-duplicate-retrieval-preparation.md)
adds `cqadup_capture.py`, `cqadup_source.py` and `cqadup_prepare.py` for bounded
official BEIR programmers capture and prospective duplicate-family cohorts.
Strict source/reference separation excludes opaque metadata from features.
Complete SHA/CRC audits and independent BFS grouping agree; no retrieval or
topic-quality outcomes have been measured. Raw sources stay outside Git.

The [development retrieval controls](../../benchmarks/2026-10-02-duplicate-lexical-retrieval.md)
use `lexical_retrieval.py` and `cqadup_retrieval.py` for full-corpus word/character
TF-IDF and word BM25. Vocabulary never sees query text or references; reserved
queries stay unscored. Complete matrices, deterministic ties and judged ranks
are captured outside Git. None establishes semantic topic quality or a selected
production method. Use the runner's fresh `--output-directory` and repeat with
`--compare FIRST_DIRECTORY/results.json`.

The [semantic window/fidelity prerequisite](../../benchmarks/2026-10-02-semantic-window-fidelity.md)
uses `embedding_windows.py`, `cqadup_window_preflight.py`, `window_inference.py`
and `cqadup_batch_probe.py` to preserve complete source/token coverage and compare
singleton/batch inference. Mixed quantized batches differ from singleton input
directions; no equivalence is assumed. `cqadup_semantic.py` prepares singleton
full-corpus title/balanced/fixed-RRF controls, with signed score domains and
explicit complete-vector captures. The [complete semantic retrieval control](../../benchmarks/2026-10-02-duplicate-semantic-retrieval.md)
reports balanced nDCG@10 .3519/Recall@100 .7397 versus BM25 .2525/.5025;
fixed RRF barely improves nDCG and lowers recall. Independent complete-vector,
score/rank/metric auditing and same-stage warm query checks pass. A fresh full
inference repeat matches every nontiming field and all eight vector/score files;
external sampled RSS/PSS maxima are about 1.54/1.53 GiB for this research process.
These are
development duplicate-retrieval outcomes, not topic gold or method selection.

The [complete-corpus BIRCH topic feasibility control](../../benchmarks/2026-10-02-topic-routing-birch-budget.md)
uses `topic_routing.py`, `topic_labels.py` and `cqadup_topics.py`. Both arrival
orders exceed the frozen 8,192-leaf cap at about 36% coverage; structured failures
retain unprocessed counts and no fabricated routing/label outcomes. Independent
source counting checks 4,879,216 nonzeros and prefix replay verifies CF statistics;
fresh captures match all nontiming fields. The prepared routing/label helpers
are tested research controls, not selected production behavior. Separate CF
compression from semantic topic grouping before another candidate.

The [fixed-bootstrap PCA25 control](../../benchmarks/2026-10-02-fixed-pca-topic-routing.md)
uses `topic_geometry.py` and `cqadup_topics.py --geometry fixed-bootstrap-pca25`.
It freezes candidate-only bootstrap coordinates before complete insertion,
separates raw radius geometry from normalized assignment and original-vector
reranking, and preserves zero-projection abstention. All 32,176 sources fit,
but forward/reverse produce 20/11 CF groups and routed Recall@100 .14–.24
versus direct semantic .74. Exact repeats and independent stage audits pass;
the configuration fails promotion on relevance/order quality. Posthoc diagnostics
separate topic-admission and candidate-packing losses without repairing settings.
This is neither a selected online startup policy nor Rust parity evidence.

The [query-relevant topic-filter retrieval control](../../benchmarks/2026-10-02-topic-filtered-retrieval.md)
uses `topic_filtered_retrieval.py`/`cqadup_filtered_topics.py` to keep the full
PCA25 topics/admission fixed while replacing centrality packing with original
cosine retrieval inside complete admitted unions. Recall@100 improves to .22–.31,
but direct .74 remains much better and vector work increases about 2.32 times.
Exact repeats and all union/score/rank audits pass; this is not equal-work speed
or intrinsic topic quality. Run with frozen topic/semantic first/repeat directories,
fresh `--output` and repeat `--compare FIRST_JSON`, externally monitored.

The [original-space topic-prototype control](../../benchmarks/2026-10-02-original-topic-prototypes.md)
uses `topic_prototypes.py`/`cqadup_topic_prototypes.py` with fixed document
memberships and equal-document-mass original-vector topic directions. Unchanged
query cutoffs abstain on 50–53 of 64 queries and Recall@100 falls to .06–.09.
Exact repeats and prototype/admission/union/score/rank audits pass, but this
configuration is rejected. No inference or threshold repair occurs; no online
prototype maintenance or intrinsic topic quality is established. The metadata
union scan is a research diagnostic, not production filtered-index performance.

The [frozen topic-label coherence diagnostics](../../benchmarks/2026-10-02-topic-label-coherence.md)
use `label_coherence.py`/`cqadup_label_coherence.py` on unchanged PCA25 labels.
Whole-document binary-presence NPMI and term reuse favor corpus-IDF, but they
are not human semantic quality: the highest-coherence list repeats testing
variants. All 36,100 joint count cells and 5,208 label pairs are independently
audited; exact repeats pass. No token/label intervention or inference occurs.
Run with frozen topic first/repeat directories, fresh `--output`, repeat
`--compare FIRST_JSON`, and the external memory monitor.

The [bounded semantic MMR label control](../../benchmarks/2026-10-02-semantic-mmr-labels.md)
uses `semantic_mmr.py`/`cqadup_semantic_labels.py` on unchanged PCA25 topics.
Top-32 lexical pools need 495 singleton phrase embeddings from explicitly
supplied installed assets, with no downloads or generative/paid calls. MMR
reduces pair cosine at lower lexical relevance/coherence and does not consistently
improve cross-topic term diversity; observed generic phrases remain. Fresh
full-inference repeats match every nontiming field and complete vector hash.
Source-supported labels and objective improvement are not semantic-quality gold.
Run with frozen topic directories, verified source root, explicit `--snapshot`,
fresh `--output-directory` and repeat `--compare FIRST_DIRECTORY/results.json`.

The [title-field evidence control](../../benchmarks/2026-10-02-title-field-topic-labels.md)
uses `topic_field_evidence.py`/`cqadup_field_labels.py` to separate complete
original title/body counts on the frozen vocabulary and topics. Exact field
conservation, fresh repeats and independent source/count/label/support audits
pass. Title-only labels increase term diversity but retain question phrasing
and testing synonyms; coherence is mixed. Thirteen zero-title-evidence sources
remain included. No inference occurs and public question titles do not establish
provider-thread title quality. Use verified topic/source directories, fresh
`--output-directory`, repeat `--compare FIRST_DIRECTORY/results.json`, and the
external memory monitor. No production topic method is selected.

The [original-space online cosine discovery control](../../benchmarks/2026-10-02-online-cosine-discovery.md)
uses `online_cosine.py`/`cqadup_online_cosine.py` without projection or inference.
At a frozen .55 cosine join cutoff, complete forward/reverse streams fit below
8,192 groups, but produce about 6,000 microgroups, many singletons and weak
insertion order agreement. Final centroid drift leaves 60/66 earlier members
below the cutoff. Full repeats and all-event alternate-reduction/source-sum
audits pass. This is neither fixed-K spherical k-means nor Links, and establishes
no semantic hierarchy or multi-topic quality. Use verified semantic directories,
fresh `--output-directory`, repeat `--compare FIRST_DIRECTORY/results.json`,
single-thread CPU settings and the external memory monitor.

The [budgeted Links reference trial](../../benchmarks/2026-10-02-links-budget-trial.md)
uses `links_reference.py`, `links_state.py`, `links_trial.py` and
`cqadup_links_trial.py`. Frozen defaults hit 8,192 subclusters after 8,351/8,403
sources, with about 99% singleton subclusters; graph components are not semantic
ancestors. Complete diagnostic repeats and independent event/graph/source audits
pass, but discovery remains explicitly incomplete and quality fields null.
Guarded timings include duplicate scans and periodic integrity checks, not full
ingestion. Do not repair thresholds/caps or promote prefix results.

The [complete source-window retrieval control](../../benchmarks/2026-10-02-source-window-retrieval.md)
uses `window_retrieval.py`/`cqadup_window_retrieval.py` on all original title/body
windows and frozen development query fields. Source/Unicode/UTF-8 coverage,
signed maxima and exact ties are validated; fresh repeats and independent
score/winner/rank/metric/composition audits pass. Body maxima improve first hits
but all-window Recall@100 falls versus balanced document directions. This is
a representation diagnostic, not episode/topic gold or a selected search policy.
No new inference occurs. Use verified semantic/source directories, complete
`--preflight`, fresh `--output-directory`, repeat `--compare FIRST_RESULTS_JSON`,
single-thread CPU settings and the external memory monitor.

The [bounded lexical/dense boundary classifier](../../benchmarks/2026-10-02-tiage-boundary-classifier.md)
uses `boundary_classifier.py` on verified complete frozen TIAGE sources/vectors.
Five dialogue-disjoint training folds fit their own scalers and logistic models;
the cutoff uses training OOF predictions only. Development is already exposed
and explicitly exploratory. No new inference occurs. F1 improves modestly, but
precision remains .35 and the paired resampling range includes zero. Exact
repeats and independent source/feature/fold/model/probability/calibration audits
pass; this is not a selected topic method or a Rust parity gate.

The [selective boundary diagnostic](../../benchmarks/2026-10-02-tiage-selective-boundaries.md)
uses `selective_boundaries.py`/`tiage_selective.py` on frozen classifier
probabilities. No training-only cutoff reaches observed precision .80 with at
least 20 accepted boundaries; the unsupported policy leaves all cases unresolved
and has no F1 fallback. Small fixed-bin calibration gaps do not supply reliable
high-precision coverage. Complete source/count/reliability audits and exact
fresh repeats pass; no new training/inference or reserved test access occurs.

The [QMSum complete training source gate](../../benchmarks/2026-10-02-qmsum-topic-span-preflight.md)
uses stdlib `qmsum_capture.py`/`qmsum.py` on pinned original author JSONL only.
All 162 meetings/88,336 turns, inclusive topic/query spans, source ordinals,
empty turns, unknown annotation gaps and overlaps remain intact. Exact repeats
and an independent sweep-line audit pass. Main-topic annotations do not supply
exhaustive coding-session boundaries or semantic ancestors; no model calls or
held-out downloads occur. [Production precedents](PRODUCTION_PATTERNS.md) inform
a proposed separation of fast assignment from background taxonomy maintenance,
with vendor-model limitations explicit. No topic algorithm is selected.

The [QMSum causal lexical context diagnostic](../../benchmarks/2026-10-02-qmsum-lexical-context.md)
compares original-turn and bounded causal raw-count evidence on complete training
source, without inference or annotation features. Context improves Hit@1/MRR but
reduces Hit@10; retain original evidence rather than promoting context-only
replacement. This is in-sample representation evidence, not algorithm selection.
The helper is `causal_lexical_context.py`; existing lexical background controls
remain in `lexical_context.py`.

The [fixed dual-view diagnostic](../../benchmarks/2026-10-02-qmsum-dual-evidence.md)
retains both original/context candidate views with full-list equal-weight RRF
and one original-source identity per result. It verifies both baseline rankings
on unchanged support before comparing fusion. No new inference or clustering
selection is involved.

The [complete fixed sparse-seed discovery control](../../benchmarks/2026-10-02-qmsum-seed-discovery.md)
tests original/context evidence under forward/reverse insertion on all common
nonempty QMSum turns. This is meeting-local insert-only grouping, not a production
multi-topic hierarchy. Context reduces singleton groups but fragments recurring
topics; known transcript markup and acknowledgements remain common descriptors.
Retrieval gains do not establish discovery or label quality.

The full suite with the extras and explicit author reference runs 449 tests.
Enable the six Links merge/split/repair/ownership/budget fixtures with
`MASH_TOPIC_LINKS_REFERENCE=/home/james/scratch/mash-artifacts/topic-links-author-20261002`;
without this variable they explicitly skip. Without the graph extra the two Infomap/runner
tests explicitly skip while mathematical and sparse-update tests still run.

```bash
PYTHONDONTWRITEBYTECODE=1 \
/home/james/scratch/mash-artifacts/topic-python-env/bin/python \
  -m unittest discover -s research/topics -p 'test_*.py'
```
