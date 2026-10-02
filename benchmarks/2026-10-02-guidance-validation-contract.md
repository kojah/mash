# Frozen held-out source and evaluation contract

P0 `mash-c0d.19.2.20.8.3.3.3.9.4.4` source preparation;
actual scoring tracked separately in `.9.4.5` and not performed yet.
[Frozen protocol](../research/topics/GUIDANCE_VALIDATION_PROTOCOL.md).
No production change, new inference, LLM, paid API or model download.

## Provenance and isolation

Before downloading validation text, inspect only the author's immutable Git tree
metadata: [pinned tree](https://api.github.com/repos/Yale-LILY/QMSum/git/trees/159791fb5fae5ef625bd0ce4d300fd3d53aca9c8?recursive=1).
Validation file Git blob `f6d2b619c21fea5d1b6e35d697af483dd2802d24`,
2,714,748 bytes, commit `83d7768c1f2b4dfeb091385d3dc7e239b8e5bb7e`.
Freeze the protocol, then capture only val.jsonl plus pinned README/LICENSE in
`/home/james/scratch/mash-artifacts/topic-qmsum-author-validation-20261002`.
Do not add it to the immutable training capture. Test split remains absent.

The separate capture/loader verifies strict original blob/size/SHA/URL, protocol
hash, exact file inventory/no symlinks, exact metadata fields and typed original
schema through the shared parser. Invalid splits/protocols/bytes/extras fail,
not convert. Whole ordered original-turn-text hashes exclude speaker aliases
and annotations: changed queries/labels cannot hide a duplicate transcript.
Reject duplicate validation meetings or cross-training complete transcripts;
shared individual text is counted, not removed. Repository licensing does not
establish underlying corpus redistribution rights; transcript artifacts stay
outside Git and are not released with this report.

Actual source check passes: 35 validation meetings/20,123 original turns,
237 specific queries/248 spans, 35 general queries without native spans,
149 original topics. Whole transcripts disjoint from 162 training meetings.
899 unique turn texts shared; 6,855 validation turn occurrences have text also
present in training. This is not a claim of completely independent utterances
or semantic quality; no source text or relevance case is dropped.

Artifact fingerprints:

- Capture SHA256 `1c5e60b0d4e9df3f46849375623353e77d1607fc5dd88a978a4f9500cd722186`.
- Source SHA256 `fc7444e241bdbe6997c01720d07f1a58b7357284acb73f909766e5a4700db49f`.
- Protocol SHA256 `729e1bf96b57b37fd967a054297e38a37b29d6312928228df6dfb060fe65744f`.
- Structured source support: outside-Git
  `qmsum-validation-source-check-20261002.json`, quality_scored=false,
  inference_calls=0. This is source integrity evidence, not a performance gate.

## Frozen evaluation

Use frozen training vocabulary/IDF, installed pinned tokenizer readiness,
original physical blocks/lexical-average fixed-eight groups and whole-thread
parents. No validation vocabulary/DF/IDF refit, query/answer/topic-name features,
new stopwords, parameter grid or case removal. Retain OOV, unready and zero rows
with original query/span denominators. Build inventory from sources before
query scoring; test set stays untouched.

Primary hypothesis exact topic-parent versus direct known-meeting macro recall20:
10,000 whole-meeting bootstrap resamples, PCG64seed20261002, all queries retained
in each selected meeting, percentile95% lower endpoint>0. MRR noninferiority
lower endpoint>-.02 is a provisional engineering safety margin. Plain thread
context remains a baseline, exact-float primary arms and already frozen numerical
counterparts secondary diagnostics. Global native judgments incomplete outside
the query meeting; global scores cannot select shared-taxonomy readiness.
No arm selection/tuning from held-out outcomes. If outcomes inform a revision,
mark it exploratory and freeze a new protocol before untouched test evaluation.

Seven standard-library fixtures pass, including source capture/parser/Unicode span
identity, drift rejection, nonfresh/symlink/extra-test files, whole-transcript
duplicates despite changed speaker/query and partial-turn overlap accounting.
File manifest decoding uses a typed model and rejects float sizes even if equal
to the expected integer value. Added to CI fixture step, remote execution
unverified. Full enabled suite now 588 tests passes; no held-out
relevance/label/agent score claimed.
Tests/source checks/manuscript preparation briefly overlap the live training
numerical repeat and must be disclosed in timing analysis.

Training numerical repeat/audit now pass; actual validation scoring remains.
Require full
repeat, independent feature/unit/tree/query/span/order/formula/metric/bootstrap
audit and scoped cost/external RSS/PSS. Coding-session semantic labels/multitags/
ancestors, guided-agent utility, bounded shared online lifecycle/edit/delete/
restart, complete ingestion<=10% loss and selected executable Python/Rust parity
remain separate open gates. No production topic method selected.
