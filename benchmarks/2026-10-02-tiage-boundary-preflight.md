# Independent topic-boundary annotations: TIAGE training preflight

Bead `mash-c0d.19.2.20.1`. This establishes a verified source for the next
episode experiment, not a segmentation result or a selected topic method.

## Source choice and semantics

TIAGE annotates PersonaChat topic shifts. Its guidelines place the label on
the incoming response: `1` means a change relative to preceding context,
`0` continuation. Annotators could consider following turns, so their judgments
are not restricted to evidence available online. These are independent human
boundaries, not primary/secondary coding-topic or semantic-ancestor labels.
[Author paper and annotation guidelines](https://arxiv.org/html/2109.04562v1).

Capture author commit `08f98dfa066a5f2fba2bbd1d6ed6ee9a783a2b1d`
(author API root tree `846e50b06df557a25ed48b3a04761805c07d88e0`), only README,
MIT license, annotated **training** JSON and two classifier-source files.
The classifier script references a Wiki loader and does not establish the
annotated JSON schema; the paper and original training rows establish the
response-label convention. No author code is executed. Development/test,
original PersonaChat and weak-supervision data remain undownloaded. No models
are downloaded or invoked. Repository metadata inspection exposes filenames
and blob hashes, not reserved contents. [Pinned author repository](https://github.com/HuiyuanXie/tiage/tree/08f98dfa066a5f2fba2bbd1d6ed6ee9a783a2b1d).

## Verified source inventory

| Training inventory | Count |
| --- | ---: |
| Dialogues, original identities 1–300 | 300 |
| Complete original turns | 4,692 |
| Root sentinel `-1` | 300 |
| Continuation `0` | 3,505 |
| Change `1` | 887 |
| Scorable boundaries, excluding roots | 4,392 |
| Exact unique original turn texts | 4,613 |
| Repeated text occurrences beyond first | 79 |
| Minimum / maximum turns per dialogue | 14 / 26 |

Counts describe the pinned JSON, not a reconstruction of published task-instance
counts. Do not copy a paper's reported count into this artifact or fabricate
missing rows. Every root has `-1`; internal sentinels, unknown labels, empty
turns, ambiguous IDs, duplicate JSON keys and malformed row types fail closed.
Root sentinels never become negative examples. Complete original text, including
whitespace, Unicode and list order, remains unchanged; no repeated turns are
dropped. No user/assistant roles or shared topic identities are supplied, and
none are invented. `Shift`, `Turn` and `Dialogue` are typed models; labels must
stay out of embedding/context features in the subsequent experiment.

## Verification and reproduction

`tiage_capture.py` bounds each download to its exact author size and verifies
Git blob SHA1 plus capture SHA256. `tiage.py` revalidates all five original
files, rejects unexpected captured files (including reserved data), and parses
the already-verified source bytes. Its ledger contains complete original IDs,
ordinals, labels, source hashes, text hashes and Unicode/UTF-8 lengths. Seven
synthetic tests cover source edits, preserved Unicode/whitespace/order,
response alignment, sentinel handling, malformed/empty rows, duplicate keys
and author hash/size drift. The full enabled research suite passes 406 tests.

Two final complete preflights with Python 3.13.5 and explicit code hashes are
byte-identical. Earlier captures before code-provenance/verified-byte parsing
guards remain preserved; final captures supersede them. An independent direct-JSON checker
reconstructs every ledger row, per-dialogue source hash, label/count/length
summary and repeated-text accounting. An initial scratch checker incorrectly
used `list.index` for ordinal recovery and failed on repeated rows; replacing
that with direct enumeration fixes the checker, without changing the loader
or source. The persisted checker then verifies all 4,692 rows and 4,392 boundaries.
This proves extraction fidelity, not annotation accuracy or model performance.

Run `tiage_capture.py --output-directory FRESH_SOURCE_DIRECTORY`, then
`tiage.py --source SOURCE_DIRECTORY --output FRESH_JSON`. Repeat adds
`--compare FIRST_JSON`. Captures and audit stay outside Git:

- `topic-tiage-author-training-20261002/capture.json`, SHA256
  `8ab08a93ee2869ada80d2fda9b2307c9962d2eae3730b1b0af25eeb71feade03`.
- Original training JSON SHA256
  `f0e2c2d163aa1bcc452d0619494ed7a9bd982c46672985dbc8fa497b66d25199`.
- `topic-tiage-training-preflight-final-first-20261002.json` and
  `topic-tiage-training-preflight-final-second-20261002.json`, both SHA256
  `71762aa571039f30fc6188559ef1221c63f44e52b932ba1528c191615ee4e4a5`.
- Independent `topic-tiage-training-audit-20261002.py`, SHA256
  `786c238fad94b0b5db43c24e817155ae35e60ec4da5d6fcd39244fd7992c6e33`,
  and verified `topic-tiage-training-audit-final-20261002.json`.

## Next experiment protocol

Before inference or development-data capture, freeze a separate experiment
contract: complete installed-model singleton turn vectors, all original
boundaries, adjacent-turn versus previous-two-turn context-mean cosine,
explicit cancellation/unsupported-input abstentions, and no-change/all-change
baselines. Calibrate only on author training boundaries, record the objective,
threshold/tie rules and complete candidates, then pin the selected artifact
before a separate development evaluation. Never sweep using development
outcomes or consume test data to repair a failed setting. Context features
use original text/vectors only, never annotation labels.

Report boundary precision/recall/F1 and threshold-free rankings, full support,
duplicates across splits, per-dialogue uncertainty and complete scoped compute
cost. A retrospective comparator may see the current response; any future
lookahead control must be separately marked as delayed/offline. Model pretraining
overlap is unknown. Only after boundary results should we test episode topic
labels, multi-membership, ancestry and guided retrieval on independently judged
coding conversations. No method selection, ingestion-overhead, Rust parity or
production change follows from this preflight.
