# Overlapping lexical discovery: prior art and source-term diagnostic

The [representation calibration](2026-10-02-question-representation-calibration.md)
found no viable dense/lexical tree cut. Review a different discovery family before
implementing a graph model. The first source-term multi-membership control is
also negative under the shared-tag gate; retain it as a cheap, reproducible
baseline rather than promoting it to a semantic topic system.

## Primary-source shortlist

[TopicMapping](https://amaral.northwestern.edu/media/publication_pdfs/PhysRevX.5.011007.pdf)
(Lancichinetti et al., 2015) filters word co-occurrence with a frequency-aware
null model, applies Infomap, then refines mixed memberships with a PLSA-like
model. A hard partition of words alone omits the refinement. This motivates
testing graph structure and memberships together; our literal-term control is
not a replication or equivalent statistical model.

[Hierarchical SBM topic modeling](https://arxiv.org/abs/1708.01677)
(Gerlach, Peixoto and Altmann, 2018) uses a document–word bipartite count network,
hierarchical priors and overlapping memberships. The authors' implementation
uses graph-tool. It is a potential offline structural comparator; we have not
verified an ingestion-cost, edit/delete or online topic-ID contract for Mash.

[Online LDA](https://papers.nips.cc/paper/3902-online-learning-for-latent-dirichlet-allocation.pdf)
(Hoffman, Blei and Bach, 2010) provides stochastic variational updates for count
mixtures with fixed K. It is a useful streaming comparator, with a topic-count
limitation. These are statistical count models; the experiment requires no LLM
generation or paid model calls. No paper's published quality/runtime results
transfer automatically to coding histories.

| Candidate | Useful comparison | Requirements still to verify |
| --- | --- | --- |
| Literal source-term memberships | Does explicit shared vocabulary provide a cheap selective overlap signal? | Meaning, rare aspects, synonyms, ancestors, online counts and persistence |
| Filtered word graph plus mixed-membership refinement | Does relational lexical structure improve over literal sharing? | Null model, connectedness/bridge contamination, stochastic stability, incremental refresh cost |
| Document–word hierarchical SBM | Does a principled overlapping graph model improve structure? | Dependency/runtime scope, scaling, source roles, semantic parents, lifecycle |
| Online count-mixture baseline | Does streaming probabilistic membership improve assignments? | Fixed K, new vocabulary, drift, edits/deletes and labels |

Source access is recorded on 2026-10-02. The paper adds corresponding references.
No graph/SBM/LDA implementation or selected production method is claimed here.

## Preregistered literal-term control

Use only the original exposed 64 calibration-source questions and natural
2,016 pairs, 97 positive/1,919 negative. Freeze the existing source-only
8,192-term vocabulary, fit transductively on 128 selected plus 72 background
questions. Original reserved references/outcomes do not enter calibration, but
the old vocabulary includes their source text; this is not held-out evaluation.
References are read only after source feature construction.

Reconstruct each parent's original title-once and distinct-body counts and equal
title/body mass. Retain the original c-TF-IDF scoring formula, including its
denominator floors, and disjoint-background IDF. Verify the original committed
scoring function AST and frozen singleton score/rank outputs; use the optimized
ranker without changing tie semantics. Preserve complete source/unit plans.

Eligible terms must occur in at least two or three questions, no more than .20
or .40 of the 64 questions, and contain at least one alphabetic code point.
DF gives one vote per parent regardless of term frequency. Numeric-only
candidate exclusion is an explicit exploratory policy, not a safe general rule
for technical histories: error codes may be meaningful. Original text remains
available. No reference tag names, aliases or example-specific recognizers enter
features. Term identity is the literal extracted source string.

For each scorer, rank eligible positive scores with lexical ties and assign
top 1, 3 or 8 terms per question. Sharing any assigned term predicts a pair
link. Empty eligible sets remain explicitly unassigned. This gives 24 frozen
configurations; choose maximum recall, then precision, then lexical key among
those meeting precision ≥.70 and at least 20 predicted links. Return no winner
otherwise. No cutoff refinements, extra stopwords or candidate-policy changes
are introduced after outcomes.

## Outcomes and limitations

No configuration passes. Every question receives at least one membership, but
coverage does not prove semantic relevance. One-membership controls predict
only 5–13 links; higher budgets add many false links.

| Budget | Best precision with ≥20 links | TP / FP | Recall | Emitted topics | Exact tag hits / eligible source tags |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | No eligible link count | — | — | 54–60 across controls | Not a winner |
| 3 | .200 | 9 / 36 | .0928 | 158 | 16 / 33 |
| 8 | .111 | 25 / 200 | .2577 | 352 | 22 / 33 |

These best-precision rows are descriptive rejected settings, not selected
winners. Budget-three uses minimum DF2, maximum .40 and background IDF;
budget-eight uses DF2, maximum .20 and background IDF. They emit 192 and 512
memberships respectively. Topic counts across all budget-three/eight controls
are 129–160 and 240–352. DF2 leaves 649/671 candidates under the two maximum
fractions; DF3 leaves 315/337. Exact source-supported eligible reference tags are
33/208 for DF2 and 20/208 for DF3, regardless of these maximum fractions.
Reference tags are incomplete lexical proxies; exact naming is not semantic
precision, and non-shared tags can still describe related subjects.

Frequently emitted terms in the rejected budget-eight row include `content`,
`div`, `class`, `const`, `getting`, `button`, `form`, `google`, `properties`,
`server`, `table` and `type`. Some are meaningful technical vocabulary; literal
presence alone does not establish a selective concrete subject. Synonyms remain
separate, words with multiple senses remain conflated, and words present in only
one source cannot form eligible seeds. A term's source DF is not the number of
parents selecting it; singleton emitted topics remain possible. There is no
semantic topic hierarchy or ancestor inheritance in this control.

The negative result does not evaluate graph communities or mixed-membership
refinement. It supports testing those mechanisms explicitly, with matched
sources/budgets and independent semantic judgments. Avoid interpreting the
partial tag gate as exhaustive semantic truth or solving contamination by
adding stopword exceptions for these examples.

## Validation, cost and next experiment

Fresh processes match every non-timing field exactly. Diagnostic work takes
1.143/1.065 seconds; total after imports is 1.862/1.661 seconds including source
feature checks. Zero new inference. Previous unit inference remains charged to
the retained source representation. No peak-memory, persistence, incremental
update, whole-ingestion throughput or production p95 gate is measured here.

Independent reconstruction from frozen sparse source records checks raw/equal-mass
count hashes, per-parent DF, scoring formulas, eligibility masks, full-sort terms
and scores, whole-original-source support, memberships/topic maps, all 48,384
pair decisions, metrics/naming counts and absence of a winner. All 263 lab tests
pass, including DF frequency invariance, numeric/alphabetic eligibility, upper
DF boundary, invalid counts and selection ties. This is Python optimization/
diagnostic evidence, not author-method equivalence or Rust differential CI.

Artifacts under `topic-public-stackoverflow-20261002`, outside Git:

- `lexical-memberships-first.json`: `b5b399caad99c1b7d077521c6918d55623f6aee8e1142f1c9e23d20e353ceeb9`.
- `lexical-memberships-second.json`: `05e1ed2fdde0b67c697ff834b7260f988115023ac8acbdcdcb3a75e55d935ecd`.
- `lexical-memberships-tests.log`, `lexical-memberships-independent-verification.log`
  and the independent reconstruction script preserve checks.

Invoke `lexical_memberships.py --root <original-capture-directory> --output
<fresh-outside-Git-path>`; repeat with `--compare <first-output>`. The pinned
baseline Git commit must be available. Captures retain all configurations,
source count hashes, DF, masks' policies, memberships, metrics and source pins.

Next preregister a genuinely graph-based comparator with mixed memberships,
explicit source-role/episode scope and source-supported labels. Preserve the
literal and dense controls. A viable development setting needs new groups and
independent primary/secondary/ancestor evaluation before promotion. Graph
statistics and vocabulary updates must have bounded initial/incremental,
edit/delete and persistence costs. Updating local counts does not prove that
global topic assignments remain stable or cheap to refresh. Track the candidate
in `mash-c0d.19.2.35`; quality, full-ingestion and Rust parity gates stay open.
