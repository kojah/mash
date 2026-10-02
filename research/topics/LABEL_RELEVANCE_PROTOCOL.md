# Fixed-group contrastive label relevance

Frozen before corpus outcomes, 2026-10-02. Keep the audited offline lexical-block
average tree and fixed-eight display groups unchanged. Prior coherence cuts stay
rejected; this stage changes word ranking only, not cuts or memberships.

Prior art: Sievert and Shirley (2014), LDAvis,
https://aclanthology.org/W14-3110/, balances within-topic probability with lift
against corpus background. Adapt its relevance formula to hard source-count
groups: .6*ln(p(term|group))+.4*ln(p(term|group)/p(term|corpus)). Weight .6 is
fixed from the published interpretation study, not tuned here. This is not LDA
inference, a reproduction of that study, or proof the weight fits agent chats.

Use unchanged source-word counts, static stoplist, eligible ready blocks,
letter-containing terms, >=2 member-block support and <=40% global block DF.
Apply these SAME candidates to existing source-supported c-TF-IDF*corpus-IDF
versus adapted relevance. Corpus word frequencies use all8196 initial ready
physical blocks after static noise filtering, not gold topic names or queries.
Top8, descending score/lexical ties. Signed relevance is valid; do not drop
negative scores. Raw-fallback blocks grant no label evidence. Empty proposals
are explicit, unchanged source identities/support required. No additional
inference, model/LLM/download, custom filler list or threshold/grid.

Evaluate exact token overlap with independently supplied QMSum topic NAMES only
AFTER both label sets are fixed. Tokenize names with the existing2-character
word regex/lowercase/static stoplist/letter rule. For each singleton-known ready
source, ask if its group's label terms overlap its native name tokens; preserve
all source denominators and report unnamed/empty-name exclusions explicitly.
This is a weak lexical name-hit proxy, not semantic label accuracy; paraphrases
can miss and generic words can falsely hit. Unknown/mixed sources still supply
word evidence, outside conditional name-hit scoring. No evaluator feedback is
used for ranking/selection. Capture complete labels, scores, source support and
concrete examples, exact non-timing repeat, stage costs/external memory, source
pins and independent formula/count/evaluation audit. No production selection
from name hits or visually appealing examples.

Quality independent of these proxies, multi-topic membership, semantic ancestry,
guided search, global online calibration/lifecycle and complete ingestion<=10%
overhead plus selected executable Python/Rust differential CI remain gates.
