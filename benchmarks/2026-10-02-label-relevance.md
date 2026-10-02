# Contrastive label relevance on unchanged lexical groups

P0 `mash-c0d.19.2.20.8.3.3.3.9`, Python-only research. No production method
selected. [Frozen protocol](../research/topics/LABEL_RELEVANCE_PROTOCOL.md).

## Prior art and adaptation

[Sievert and Shirley's LDAvis paper](https://aclanthology.org/W14-3110/)
balances within-topic term probability and corpus-relative lift. The authors'
[demo](https://www.kennyshirley.com/LDAvis/) documents relevance and the
interpretation-study weighting near0.6. Freeze that weighting before outcomes:
`0.6*ln(p(term|group))+0.4*ln(p(term|group)/p(term|corpus))`.
Here probabilities come from hard source-count groups, not an LDA model.
This is an adaptation, not a reproduction of their user study or evidence
that0.6 transfers to agent histories. No lambda sweep follows this result.

Keep the audited lexical-block average trees and fixed-eight display groups
unchanged. Compare existing source-supported c-TF-IDF*corpus-IDF against
adapted relevance with SAME static stoplist, letter rule, at-least-two eligible
member blocks and at-most40% corpus block DF. Top8 with lexical score ties;
signed relevance is valid, not dropped. Raw-fallback blocks supply no label
evidence. Both use original words/counts and all8,196 initial ready physical
blocks as background; no new inference, model/LLM/download, custom filler list,
gold feature, threshold/window grid or held-out read. Label-support examples
retain the first two supporting original block IDs, not invented descriptions
or claims of semantic representativeness.

Original162 training meetings,88,336 sources,84,434 inherited ready sources,
77,892 singleton-known sources and1,296 groups remain accounted for. No cuts
or memberships change. Unknown/mixed ready sources contribute words but stay
outside conditional name scoring. Native topic names enter only AFTER all
rankings are fixed. The name-hit proxy asks whether a source's group label
words exactly overlap its native name tokens. It is weak: paraphrases can
miss, generic words can falsely hit, and broad native names are not coding
label/multitag/ancestor quality judgments.

## Results

| Method | Named sources | Name-token hits | Rate | Empty groups | Groups with <2 terms |
|---|---:|---:|---:|---:|---:|
| Corpus-IDF c-TF-IDF | 77,892 | 21,680 | .278334 | 588 | 693 |
| Relevance0.6 | 77,892 | 17,635 | .226403 | 588 | 693 |

Both output4,183 terms;6,542 unknown/mixed ready sources remain explicit,
zero sources have empty native-name tokens. The adaptation changes rankings
but does not improve this proxy or label availability. Do not promote it;
this does not prove semantic inferiority from a weak lexical proxy alone.

First meeting's unchanged57-block group: baseline labels
yellow/gap/l_c_d_/bright/colour/say/criteria/display; adapted relevance
stolen/gap/bright/yellow/lotta/criteria/ugly/say. Rarity can highlight anecdotes
or style instead of useful names. The7-block group's project/ideas/pretty/new/
yes/right mostly reorder. Prior failed coherence cuts remain failed.

Separate structural diagnostic:516 groups contain one physical block, all516
necessarily empty under the >=2-block rule;359 of those blocks contain multiple
original ready sources. Word ranking cannot remove this eligibility ceiling.
Next P0 `mash-c0d.19.2.20.8.3.3.3.9.1` separates ORIGINAL source support
from pooled clustering units, under unchanged scoring/groups. Multiple sources
do not establish independent judgments or semantic quality.

## Validation

Artifacts under `/home/james/scratch/mash-artifacts`, outside Git:

- First `topic-qmsum-label-relevance-first-20261002.json` SHA256
  `3145ec5f2bec206a7bf724a11dd15bf61509ca00524af252c0724eb4697a0697`.
- Repeat `topic-qmsum-label-relevance-second-20261002.json` SHA256
  `c6f7ed5fdcba0e865156d4d9055be7e5e1a7953fd8b33ea393e9033863f29d2d`.
  Every non-timing field matches exactly; begin/end code pins unchanged.
- `topic-qmsum-label-relevance-audit-20261002.json` SHA256
  `4b5caa74a64638e5f25ae99d1910cf3a5773d16c9bb4f8b3e970d9b5845784c5`.
  Independent scalar auditor reconstructs original counts, global/member DF,
  filtered corpus probability, both scoring formulas/lexical ranks, fixed cuts,
  two-block support examples, native memberships/name tokens and summaries.
  Verifies8,366 label terms,8,366 support examples and168,868 source-proxy
  checks. Maximum numerical discrepancy8.88e-16, within frozen relative/absolute
  1e-12. Auditor SHA256
  `c526c0daef7fbc8782260110165a5cd7895cd2ba4710f09fb83a6c25b7869255`.
  Shares pinned original count ledger/NumPy/SciPy/sklearn stoplist; tokenizer
  previously audited. This is implementation evidence, not semantic quality.

Five new fixtures verify lift ordering, invalid probabilities, signed ranks,
source/fallback gates, name-proxy zero/unknown/empty accounting and complete
ownership. Full enabled lab suite552 tests. No Rust/app/deployment change.

## Cost scope and limits

First/repeat after-import wall48.106/31.113s, CPU17.048/16.188s,
setup43.201/26.932s. Background frequency/DF65.427/61.222ms. Across162
meetings baseline labels1.710/1.466s, relevance1.542/1.299s. These are label
research scopes; proxy/provenance/capture processing is also in the full
target. Large input validation/host load dominates and changes markedly; no
speed-win or production ingestion claim. Main timer excludes imports, final
JSON/repeat comparison; external sampling includes them. No complete ingestion,
online label maintenance or persistence measured.

Rusage peak2,076,524/2,076,356KiB. External RSS/PSS maxima
2,078,136/2,068,102 and2,077,784/2,070,004KiB; approximate status HWM
2,078,136/2,077,784KiB. Samples894/715, vanished2/1;
median/max gaps53.798/276.111 and43.503/185.880ms. One target excludes
observer/children; monitoring intrusive, sampled maxima lower bounds, status
approximate, launcher rusage may retain high water. No subtraction or
production-worker attribution. Scientific runs sequential; brief first-result
inspection overlaps repeat setup, uncontrolled host load.

Independent semantic/coding labels, multitag membership, semantic ancestry,
guided-search gains, shared global frame/lifecycle, edits/deletes/restart,
complete ingestion<=10% overhead and selected Python/Rust executable parity
remain open. Improving an endogenous score or name proxy cannot waive them.
