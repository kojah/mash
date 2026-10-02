# Complete raw / marker-masked local semantic controls

The lexical seed control failed nonoverlapping topic recurrence; this study
changes the representation to the installed local MiniLM model while retaining
a fixed seed discovery control. It does not assume that lexical retrieval gains,
marker-free descriptors or reproducible inference imply semantic topic quality.

## Protocol frozen before semantic outcomes

Use the complete pinned QMSum training source:162 meetings/88,336 turns. Compare
raw text with exact case-sensitive braced `{disfmarker}`/`{vocalsound}` characters
replaced by equal-length ASCII spaces. Raw text remains immutable; replacement
preserves every original character and UTF-8 coordinate, with all exact marker
spans recorded. No stopword/filler removal, cropping, semantic rewriting or LLM.

Explicitly supplied installed model/tokenizer pins:

- Quantized MiniLM:
  `afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1`.
- Tokenizer:
  `da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0`.

Source assets live in the prior `mash models install` snapshot
`751bff37182d3f1213fa05d7196b954e230abad9`, under the existing
`ci-contracts-2ba4629/models/.mash-assets-oio3h5` artifact tree. No downloads occur.
Tokenize singleton, unpadded/untruncated, special tokens included; cap512 model
tokens. Empty/marker-only and over-window cases are explicit unsupported statuses,
never silently truncated or silently omitted.

Encode each exact ready input text once across both policies, using one model
session, one intra/inter CPU thread, batch1. Pool all attended tokens in float32;
scale by127/maxabs, round ties away from zero, clip to int8 and normalize float64.
Bound the encoder cache at1024, clearing it when full; reuse frozen stored vectors
for exact-text repeats, with first-origin lineage. Persist two88,336x384 float64
matrices. Unsupported rows are zero with explicit statuses, never searchable
zero embeddings. No reserved source, generative model or remote inference.

Before new discovery outcomes, freeze the same source-seed cosine0.55 control,
cutoff equality joins, first-created ties, immutable seeds, cap2048 per meeting,
forward/reverse source orders. Use all84,434 common-ready rows, including unknown
and multi-annotated inputs. Evaluate singleton-known pairs both all-distance and
original-distance>4; the latter is prespecified here. Labels use exact-marker
cleaned original unigram counts, fixed original whole-training IDF and eight-term
class score/lexical ties. No broad stoplist or threshold grid. Per-meeting reset,
insert-only single memberships and no semantic ancestry/lifecycle are explicit
control limitations, not the production design.

## Complete preflight

| Input | Ready | Empty / marker-only | Over512 |
|---|---:|---:|---:|
| Raw | 88,182 | 81 | 73 |
| Masked | 84,447 | 3,829 | 60 |

Common readiness is84,434. All original sources remain accounted for, including
3,748 newly empty masked turns and13 newly window-supported masked turns.
Exact unique input hashes across all statuses:59,304 raw/59,269 masked.
All50,128 exact marker spans (33,979/16,149) retain source coordinates.
The complete preflight repeats exactly outside its timing envelope. Independent
literal-search/source/tokenizer audit verifies all176,672 variant rows, hashes,
offsets, token counts and support. It shares the pinned tokenizer implementation;
it is not independent tokenization, model/Rust parity or semantic quality.

Preflight SHA256 first/repeat:
`be77327996bc0a517cadad8d12074470ad6fdecedb17f40b444c18bddaeb0620` /
`068a2270e34e60513aa77b9676a5c72d6ee3062e80de8002d6b88167708a8b02`.
Audit script SHA256:
`84b8ea4b669f9262c4239c5ab1f02f383ad64c896eecf445bae158a4af12b099`;
terminal report `topic-qmsum-semantic-preflight-audit-first-20261002.json`.

After-import first/repeat wall8.575/11.483 s, CPU8.486/9.780 s, rusage
301836/302692 KiB. Timers exclude JSON/repeat comparison and imports; repeat
overlaps encoding. External target RSS304232/590400, PSS296372/556931 and status
high water304232/592920 KiB include imports/comparison/JSON. Samples427/585,
zero vanished reads, median/max gaps26.50/31.85 and29.32/58.43 ms. Repeat
comparison loads another complete ledger after the timer; scopes differ.

## First frozen vectors and audit

There are83,272 unique inference calls and89,357 saved-vector reuses across
172,629 ready policy/source rows. Encoder cache clears81 times, internal cache
hits zero, maximum input512 tokens. ONNX Runtime1.30.0/tokenizers0.23.2; no new
model. First capture SHA256:
`05642c1a77dab33d2940c39979b0ac4e4b420420debcb30ca529d7bad99bdf98`.
Raw/masked matrix SHA256:
`86d67c9f1a0fd47fa1409633805951cdb9cfc2247a400cd8f8717e06f952238c` /
`dd2abc5f34930868aad13017bd31ffcdb21e439d1063c863c41b2e29bde76a1d`.

Independent audit reconstructs every original literal-mask input, status, hash,
first exact-text origin, unit/int8 direction and unsupported zero row. It verifies
complete call/reuse/cache-clear accounting. Direct independent pooling/quantizing
re-inference on32 smallest ready-input SHA256 and8 longest inputs (stable source/
policy ties) covers40 distinct texts and matches saved vectors bitwise. Numeric
structure and replay errors are zero. Same tokenizer/ONNX engine remains a
limitation; this is sampled model replay, not independent inference-engine or
Rust parity. Auditor SHA256:
`9663a9d6d1bd64f25345b1fa8d2d0f317834707bc32705d706cb2729a7307b91`;
terminal report `topic-qmsum-semantic-vector-audit-first-20261002.json`.

First after-import wall354.796 s, CPU287.839 s; setup2.463 s, encode stage351.661 s,
flush/hash .671 s. Recorded model wall287.806 s and tokenization17.697 s are
sub-scopes, not additive CPU measurements. Rusage1310032 KiB; external target
RSS1311676/PSS1303875/status high water1311676 KiB,7300 samples, two vanished
reads, median/max gaps43.08/300.74 ms. Retained raw source ledgers/mapped output
arrays/model share this offline workspace; it is not production model memory.

The full fresh embedding repeat completed successfully: all nontiming fields,
both vector-file hashes, support, origin lineage and accounting match exactly.
Repeat capture SHA256:
`473b1b00672c50d6b152b36af34c7ed4a695c298d739fc71572bfca549f7dad8`.
Repeat wall369.906 s, CPU282.873 s, setup2.479 s, encoding366.618 s,
flush/hash .810 s; model wall301.693/tokenization19.191 s. Rusage1309020 KiB;
external RSS1310296/PSS1276021/status1310296 KiB,7155 samples,two vanished,
median/max gaps44.17/497.51 ms. It overlaps semantic discovery/audit, so these
are reproducibility observations rather than a matched production speed test.

## Complete semantic seed discovery

All84,434 common-ready inputs enter every policy;77,892 singleton-known rows
define27,346,484 all-distance and27,049,334 nonoverlap gold pairs.

| Policy | Groups | Singletons | Far precision | Far recall | Far F1 |
|---|---:|---:|---:|---:|---:|
| Raw forward | 30,328 | 20,794 | .320282 | .046487 | .081190 |
| Raw reverse | 30,396 | 20,739 | .329192 | .044663 | .078654 |
| Masked forward | 40,713 | 29,453 | .335820 | .029401 | .054068 |
| Masked reverse | 40,761 | 29,366 | .341648 | .028606 | .052791 |

Median group size is1 in every policy; largest groups356/288/243/229.
All-distance F1 .082737/.080182/.055675/.054393. Empty source-count labels
110/116/108/111 remain explicit; no semantic label-review gate passes.
Masking slightly raises precision but reduces recurrence and increases
fragmentation. Reject this fixed seed control as a production topic method;
do not adopt general embedding cleanup from the marker-free-label result.
The lexical comparison used different common support, so its pair metrics
cannot establish an isolated representation effect here. Broad meeting gold
is not exhaustive fine-grained coding-session gold; this control does not
compare BIRCH against GRINCH or prove that MiniLM cannot support better units
and discovery. No cutoff tuning is justified by these exploratory outcomes.

First discovery capture SHA256:
`86efcb57e3f9a546976e660e155513c64c35ab89254796d92e11245c9fdf116c`.
After-import wall289.017 s, CPU195.588 s, setup14.405 s,rusage2334564 KiB.
External target RSS2336200/PSS2303485/status2336200 KiB,3673 samples,zero
vanished,median/max gaps63.33/829.90 ms. Full four-policy ledgers and mapped
vectors are retained offline; this is not a production memory/cost measurement.
Fresh discovery repeat completed successfully: every nontiming field, assignment,
event, descriptor and pair contingency matches exactly. Repeat capture SHA256:
`97ac7c846153920f1c41a751ee291dac2abd4b05bb9f3b7cf562a6ba5b1ea261`.
Repeat wall254.075 s, CPU189.830 s,setup9.624 s,rusage2335960 KiB.
External VmRSS2308652/smaps RSS2316844/PSS2091885/status high water2335960 KiB,
3784 samples,one vanished,median/max gaps57.91/597.27 ms. It overlaps the
independent auditor and initially vector inference; repeat comparison is after
the timer. Differences in sampled peaks do not imply reduced algorithm memory.
Independent ownership/seed/label/pair audit completed successfully. It replays
all337,736 seed events, scalar label scores and complete known upper-triangle
pairs, enumerating original-distance>4 pairs separately from the driver's
nearby-pair subtraction. Maximum seed-score/label error is zero; complete
source ownership, exclusions, group sizes and all summary metrics match.
Auditor SHA256:
`6bd46dafa46333b9f7f101489def4b527afa9bef5e95f392cd19bd6ae71f774d`;
terminal report `topic-qmsum-semantic-discovery-audit-first-20261002.json`.
It shares scipy sparse dot; source/count/vector audits are separate prerequisites.
It supplies no independent coding-topic, label, multi-tag or ancestor judgments,
Rust parity or lifecycle evidence. No method is selected or promoted.

## Reproduction and limits

Drivers `marker_inputs.py`, `qmsum_semantic_preflight.py`,
`qmsum_semantic_vectors.py` and `qmsum_semantic_discovery.py`. Existing science
environment/assets only; output under `/home/james/scratch/mash-artifacts`.
Preflight accepts `--source --assets --output [--compare FIRST]`; vectors accept
`--source --assets --preflight --output-directory [--compare FIRST/results.json]`;
discovery accepts `--source --vectors --labels --baseline --output-directory`.
Wrap measured processes with existing `memory_audit.py --output MEMORY -- COMMAND`.
Five new edge tests bring the complete enabled lab suite to458.

Externally sampled peaks are lower bounds, status approximate and monitoring
intrusive; pre-exec launcher high water may persist. Observer/other-process
memory is excluded, overlapping processes perturb timings, and no peaks are
subtracted. No full-ingestion throughput, online label maintenance, multi-tags,
edit/delete, semantic ancestry or independent coding-topic-quality gate passes.
Bounded study bead `mash-c0d.19.2.20.8.2.2` is complete with a rejected control;
broader topic-method selection and semantic validation remain open.
P0 follow-up `mash-c0d.19.2.20.8.3` separates bounded contextual-unit evidence
from discovery algorithm comparisons, with matched support and cost before
changing production. Fundamental changes are authorized but require evidence;
this rejected control is not a BIRCH/GRINCH comparison.
