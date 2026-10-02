# Independent duplicate-question retrieval preparation

Bead `mash-c0d.19.2.30.1`. This prepares a new retrieval signal after the
shared-tag representation controls failed. No ranking outcomes, topic selection,
encoder calls or production changes occur in this step.

## Research and scope

[CQADupStack](https://github.com/D1Doris/CQADupStack) provides community duplicate
links. Its [annotation-quality study](https://eltimster.github.io/www/pubs/sigir2016-webqa.pdf)
reports incomplete links and broad answer-hub questions: duplicates are useful
retrieval judgments, not necessarily semantic equivalence. They cannot establish
primary/secondary topic tags, semantic parents or guided-agent success.
We use the [BEIR](https://arxiv.org/abs/2104.08663) programmers packaging.
Our prospective group split is not CQADupStack's original chronological split.
This remains one programming forum, not a multi-provider dialogue evaluation.

## Capture and prospective protocol

The official BEIR README at commit
`ef83d29307061c65d04b035b4f4e7c18bd8374af` names the archive and its MD5.
`cqadup_capture.py` reads only the three programmers members from the
5,343,728,040-byte archive, using exact HTTP 206 ranges and conditional strong
ETag `"607ee440-13e82d1a8"`. It refuses a whole-archive HTTP 200 response before
reading its body. Caps are 8 MiB per range, 384 MiB compressed transfer and
1 GiB complete selected members. No truncation or path extraction is used.
Actual transfer is 292,668,346 bytes. Complete member sizes, CRCs and SHA256s
are verified; the official whole-archive MD5 is explicitly **not verified**.
ETag/CRC/member hashes provide identity and integrity checks, not an authenticated
full-archive digest. Raw public data stays outside Git; redistribution permissions
have not been established from the scripts' license.

The large query file contains repeated opaque metadata, not long query text.
The source-size budget was amended before full capture or outcomes, after inspecting
declared member sizes and initial records. Features use only corpus `title`/`text`
and query `text`; metadata, including tags and dictionaries, is excluded.
Source fields and Unicode are preserved without cleaning or truncation.
Strict schemas reject unknown fields, duplicate JSON keys, nonfinite JSON,
duplicate identities, malformed references and source/hash drift.

Before outcomes, freeze seed `mash-cqadup-programmers-v1-20261002`. Join query
and document nodes by explicit positive links, identical original IDs and exact
title/text hashes, solely to prevent cross-cohort leakage. Canonical component
identities and seeded hash order choose the first 64 query-bearing components
for development and next 64 for reserved evaluation; the rest are recorded unused.
Do not refill or silently shrink. **Do not expand relevance by transitivity.**
Keep the complete original candidate corpus, without gold-based negative sampling.
Unsupervised full-corpus indexing is transductive; reserved query outcomes cannot
tune representation or ranking. Unjudged candidates are not proven irrelevant.

## Verified preparation

| Item | Count / size |
| --- | --- |
| Corpus | 32,176 records; 37,360,424 raw bytes |
| Queries | 876 records; 774,354,135 raw bytes |
| Declared query text | 48,345 bytes total; maximum 149 bytes |
| Explicit positive links | 1,675; all 876 queries supported |
| Development / reserved / unused | 64 / 64 / 748 query families |
| Largest development / reserved family | 33 / 150 nodes |

There are 31,376 total components and no shared query/corpus original IDs.
Each query-bearing family contains one query here; large families reflect many
linked documents, not established semantic cliques. Exact content overlap with
the previous 200-source Stack Overflow cohort is zero. This is not evidence
against near-duplicates, model pretraining overlap or domain similarity.

Fresh-process preparations match every nontiming field exactly, taking
5.522 and 4.564 seconds after imports. These include raw metadata decoding/hash
validation and partitioning; they are not ingestion or search timings.
An outside-Git verifier independently checks complete member SHA256/CRC/size,
text-size summaries and adjacency/BFS components against the union-find
partition. It also checks exact old-source overlap. The hash serialization
helper is shared, so this is not wholly independent source canonicalization.
Eight new synthetic tests cover range refusal/budgets, schemas, references,
source tampering, hash joins and order-independent disjoint cohorts. The full
research suite passes 331 tests. Synthetic tests are not semantic annotations.

## Reproduction and next experiment

Artifacts: `/home/james/scratch/mash-artifacts/topic-cqadup-programmers-source-20261002`.
Run the capture with `--output-directory FRESH_OUTSIDE_GIT_DIRECTORY`; use
`--list-only` to inspect members without capturing bodies. Source ETag changes
must fail rather than silently changing the benchmark.
Then run `cqadup_prepare.py --root ROOT --capture-sha256 CAPTURE_PIN --output FIRST`
and repeat with a fresh output and `--compare FIRST`. Dependencies remain pinned
in `research/topics/uv.lock`; no BEIR package or model download is required.

- Capture SHA256: `6ed3409aaeb3734e5fd2c89d6d7d0412dca45ca0828f7f7ea654450bd513782f`
- First cohort SHA256: `1f190cfcc154af6881f9b115a69d8ed9a0fc01ba93961c44be4f7ce25b8e9fbf`
- Repeat cohort SHA256: `4656252ad39008d391c8c2321504c1dbe4125725680b18985cb95b138c7eca69`

Next preregister matched lexical retrieval controls on development queries,
with complete-corpus ranking, explicit deterministic ties and ranking metrics.
Inspect failures before considering topic-derived retrieval. Reserved outcomes
remain unopened. Independent dialogue/topic judgments and lifecycle/performance
gates remain necessary before selecting any production topic method.
