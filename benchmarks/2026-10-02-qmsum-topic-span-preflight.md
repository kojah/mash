# QMSum complete training topic-span source gate

Bead `mash-c0d.19.2.20.5`. No prediction, inference or production method selection.

The [author paper](https://aclanthology.org/2021.naacl-main.472/) asks annotators
for main topics and possibly discontinuous relevant turn spans. Its appendix
suggests 3–8 topics and includes sufficiently substantial chatting. This is
main-topic evidence rather than exhaustive fine-grained agent-session gold.
The [author repository](https://github.com/Yale-LILY/QMSum) supplies original
JSONL. Open [annotation](https://github.com/Yale-LILY/QMSum/issues/7) and
[index-semantics](https://github.com/Yale-LILY/QMSum/issues/14) reports reinforce
the need to preserve and audit source data; reports are not proof that our
training rows are semantically wrong. Held-out transcripts remain unread.

## Provenance and typed source contract

Pin author commit `83d7768c1f2b4dfeb091385d3dc7e239b8e5bb7e`, tree
`159791fb5fae5ef625bd0ce4d300fd3d53aca9c8`. Capture only README, LICENSE and
`data/ALL/jsonl/train.jsonl`, outside Git. Train blob
`612fb8ca3e2810c85af1821691b38b2961347528`, 11,940,700 bytes, SHA256
`55964a968e4b4c1c9da2434678f213f3b70c0c3d3cf609f5cd4ecd44cc8d2427`.
Capture manifest SHA256
`7991b3bcd8a3c9850b463bed85d8af50db893eb4357ba43bc286efb95a3c8808`.
Verify Git blob/size, independent SHA256, exact file inventory and source bytes
before parsing; refuse held-out additions or provenance drift.

`qmsum.py` uses frozen typed meeting/turn/topic/span/general-query/specific-query
records. Exact four-field meeting schema, duplicate-key rejection, original
string span indices and **inclusive** endpoints are enforced. Preserve speaker,
text, line order/hash and original turn/topic/query/span ordinals. Empty turns
stay in place. No native meeting IDs, domains or agent/user roles are supplied
by these JSONL rows: do not fabricate them. Human topic names, queries and
answers are evaluation-only, never source embeddings or lexical feature input.
Topic identity is meeting-local ordinal, not a cross-meeting label-name merge.

## Complete observed inventory

| Property | Count |
| --- | ---: |
| Meetings | 162 |
| Original turns | 88,336 |
| Topic records / inclusive spans | 725 / 771 |
| Topics with multiple source spans | 33 |
| General / specific queries | 162 / 1,095 |
| Specific-query spans | 1,206 |
| Turns without annotated topics | 6,434 |
| Turns with multiple annotated topics | 446 |
| Same-topic overlapping-span turn incidences | 0 |
| Empty transcript turns / speakers | 81 / 0 |
| Exact unique text / repeated occurrences | 59,304 / 29,032 |
| Exact unique topic-name strings | 688 |

All spans are structurally in range. Meetings have 91–1,832 turns; maximum
turn size is 8,733 UTF-8 bytes. This is **not** tokenizer/model support: no
tokenizer, encoder or inference preflight runs here. Empty/long units require
explicit representation handling before a later model experiment.

Every one of 88,174 adjacent pairs is classified structurally:

- 81,111 preserve a nonempty annotation membership set.
- 466 switch between different singleton topic sets.
- 42 change annotated membership with multi-topic involvement.
- 396 cross an annotation gap.
- 6,159 have no annotations on either turn.

Gaps are unknown, not no-change gold; overlap transitions do not imply a unique
hard cut. Annotated singleton switches are source membership events, not proof
of every semantic shift. Recurrence supports evaluating repeated topics;
overlap support is small. There is no ancestor hierarchy or universal topic ID.
Repository MIT text does not establish all upstream transcript redistribution
rights; raw data remain outside Git pending source-license review.

## Verification and scoped cost

Two final fresh runs match every field except the explicit top-level timing
envelope, including all 88,336 turn records/hashes, 725 topic records, spans,
query annotations and 88,174 transition records. A separate stdlib auditor
reads original JSONL and reconstructs annotation membership with an independent
sweep-line event algorithm, verifies hashes/interval endpoints/identities and
all counts. No source repair, silent deletion, rewriting or inference.

Seven tests exercise Unicode/CRLF, preserved empty turns, inclusive endpoints,
overlaps, recurrent/duplicate source spans, annotation-feature separation,
gap/overlap transition distinctions, malformed range/schema/duplicate keys and
invalid capture scope. Tests also run in dependency-free CI. Full enabled lab:
**433 tests pass**, with author reference and graph extras.

Final artifacts under `/home/james/scratch/mash-artifacts`:

- `topic-qmsum-author-training-20261002` (four files including manifest).
- `topic-qmsum-preflight-final-first-20261002.json`, SHA256
  `3a79fe865d7714ffb913e20123520a9fad6aad5244ae1b502146dabbc0b3371b`.
- `topic-qmsum-preflight-final-second-20261002.json`, SHA256
  `2a65df0a9b12ce591a6cad189f7b801edc55890226ea9d592a8cc8697fec4bf0`.
- `topic-qmsum-source-audit-20261002.py`, SHA256
  `2927a8ce2c2f141050c8d377d2d6a144adca40d7216b2ce13db5db966e8512b3`;
  output `topic-qmsum-source-audit-first-20261002.json`.
- `topic-qmsum-preflight-memory-final-{first,second}-20261002.json`.

Python 3.13.5 final stdlib source gate: after-import preflight wall
1.256/.886 s, process CPU .716/.817 s, rusage 161932/162268 KiB. That timer ends
after ledger construction, before repeat comparison and final JSON writing.
Each artifact is about 50.9 MB of verbose hash/annotation metadata. External
sampling covers the entire target process, including reading the first ledger
during repeat: RSS 162324/344764 KiB, PSS 153637/335667 KiB, status high water
162324/346296 KiB. There are 107/120 observations, no vanished reads, median/max
gaps 25.94/47.46 and 25.37/40.43 ms. The repeat's memory scope is larger than the
preflight timer's; do not attribute it to ingestion or infer constant model cost.
Observer/children excluded; sampled peaks lower bounds, status approximate,
monitoring perturbs runtime and rusage can preserve launcher peaks; no subtraction.

Initial science-environment reports `topic-qmsum-preflight-{first,second}-20261002.json`
remain as superseded captures. The final driver removes an unnecessary science
dependency from repeat comparison, compares only the explicit timing envelope
loosely, and adds stdlib CI coverage. No annotation/source change was made.
These runs do not satisfy the full-ingestion performance gate.

Reproduce capture with `qmsum_capture.py --output-directory FRESH_SOURCE`,
preflight with `qmsum.py --source SOURCE --output FRESH_JSON`, repeat
`--compare FIRST_JSON`. Use the external memory monitor; no model path is accepted.

## Next decision

Use this source as adjacent human-annotation evidence for **recurring topic
support**, not as a complete flat partition. Freeze a bounded representation and
source-family split policy before model experiments; prevent gold topic names,
questions or answers from entering features. Any semantic-label, overlap or
ancestor claim needs its own appropriate gold and validation. This source gate
does not select BIRCH, GRINCH or any production method.
