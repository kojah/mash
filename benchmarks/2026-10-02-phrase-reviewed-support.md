# Existing reviewed subjects expose missing secondary phrase evidence

## Source and conditional protocol

The existing eight-thread review has NO overlap with the 128-thread pilot.
It is a separate exposed development diagnostic, not validation of those
threads. Packet SHA256
`c76f8dc18e430e5dfb4215ec33749689608afd12213e33cd48392f14c8fa6df5`;
review SHA256
`85845858aee76cc33bec0da699614913f86ad0aa84650dbf6c4d8566f7982191`.
The annotator is a single coding assistant with prior corpus familiarity,
not independent human gold. Existing subject judgments are consumed unchanged;
no new judgments, source reselection, inference, downloads, LLM or paid calls.

The original 84 records have reviewed kinds: 39 content, 29 referential,
13 protocol, two attachment and one sensitive. Admit only the first two kinds,
68 records; this is **oracle admission**, not a deployed ingestion filter.
Three uncertain subjects are excluded from the 21 annotations, leaving nine
primary and nine secondary subjects with exact admitted support ordinals.
Subject names/rationales never enter discovery or vocabulary; names are only
used afterward for a disclosed lexical-overlap diagnostic.

An initial run rejected the hash of a deliberately redacted sensitive display.
That failure/log/memory sidecar is preserved. The corrected boundary verifies
all unredacted hashes and original identity/ordinal metadata. A redacted record
must be reviewed sensitive and remain excluded; its original text is unavailable
in this packet and is not claimed rehashed. The complete packet/review bytes
are pinned. A fixture verifies that redacted content cannot be admitted.

Keep the phrase prototype unchanged: at most eight terms/thread, deduplicated
input voting, sublinear thread c-TF-IDF times thread IDF, .4 thread DF filter
after three threads, atom containment pruning and stable namespace/phrase IDs.
The vocabulary/IDF frame is fitted on all eight admitted development threads.
This is transductive, conditional on reviewed admission, not held-out evaluation.

## Metrics and results

For each reviewed subject, measure (1) the fraction of its support records
containing ANY selected phrase; (2) the maximum single selected phrase's local
support-set F1, using only that thread's original source identities; and
(3) whether any selected phrase shares an atom with the subject name. The last
is a lexical proxy with no synonym/lemmatization handling. The first can count
a phrase unrelated to the intended subject on a multi-topic support record.
Support F1 measures evidence-set overlap, not label meaning or semantic accuracy.

| Conditional diagnostic | Primary | Secondary |
| --- | ---: | ---: |
| Reviewed subjects | 9 | 9 |
| Mean any-phrase support recall | .928571 | .481481 |
| Mean best single-phrase support F1 | .734512 | .385714 |
| Subject-name atom hits | 5 | 3 |
| Subjects with no literal evidence | 0 | 4 |

The snapshot has 63 phrase subjects (six corpus-recurring, 57 provisional),
791 vocabulary terms, tags for all eight threads and evidence for 33/68 records;
26 sources match multiple phrases. Thread coverage again hides missing subjects.
Four secondary subjects have no selected literal evidence under the current
contract. The top-eight whole-thread ranking can starve brief secondary
discussion; this is an actionable failure, not proof that all remaining phrases
are useful or that the selection cap alone caused it.

Do not select/promote the prototype on these results. Next compare bounded
source-unit candidate selection/coverage against the frozen whole-thread control,
without feeding subject names into discovery, output-driven threshold fitting
or silently borrowing unrelated context. Independent coding-session subject
quality, broader held-out evaluation, online publication/full-ingestion budgets
and selected-method Rust parity remain open.

## Verification and cost

Verified first/repeat runs match every non-runtime field. A no-producer-import
auditor independently checks admitted source hashes and literal memberships,
all 18 original subject support sets, recall, maximum support F1 and summaries.
It uses the independent scanner with shared sklearn static stoplist. Numeric
F1 tolerance is 1e-12; this does not independently validate discovery rankings,
lifecycle, native parsing or semantic label quality. The source review is a
shared dependent input, not independent judgments. Three new fixtures cover
support arithmetic/local scope and opaque-sensitive rejection. The full research
suite passes 651 tests in 4.623s.

After-load discovery/evaluation takes 46.03/18.80ms wall and 20.11/18.76ms CPU,
excluding imports/source loading and output serialization/comparison. This small
eight-thread workload is not a production speed estimate or full-ingestion gate.
External sampled target RSS is 120,556/120,548 KiB, PSS 110,550/107,010 KiB,
approximate status HWM 120,556/120,548 KiB and pre-serialization rusage
119,952/120,432 KiB. Whole target includes imports and source/review loading;
do not subtract counters or attribute it to an isolated worker. There are
147/73 samples, one vanished read each and maximum gaps 57.25/27.64ms.
Samples are lower bounds, counters approximate, observer/children excluded,
monitoring intrusive and launcher history may affect rusage.

Private verified result hashes:

- First: `96707458bd53e3e8b1b6c34feda231dc014e0473c0cf2154645e70ec7ccc5274`.
- Repeat: `c3ced242218d8844e7d9a4083189b5f86b4d5427e09c0df7106f3111ea44f82c`.

Artifacts use `phrase-review-verified-{first,repeat}-20261002`, matching memory
sidecars/logs and `phrase-review-audit-20261002.json` under mash-artifacts.
The failed `phrase-review-first-20261002` capture remains terminal and preserved.
Raw sources, sensitive values, labels and review content are not published in Git.
