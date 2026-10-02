# Title/prose/code weighting: negative discovery comparison

Bead `mash-c0d.19.2.36`. The audited source channels now support a prospective
weight/discovery comparison. **None of 216 settings passes the unchanged gate,
even individually.** No method is selected or evaluated on reserved outcomes.

## Frozen setup

Consume pinned `channels-first.json` and its exact extraction/source/module
hashes, plus the original graph and calibration contracts. Use the same 64
exposed questions, 2,016 natural pairs/97 shared-tag positives and frozen
629-word source-only unigram vocabulary. The original vocabulary is transductive;
no reference tags/names/rationales enter features. All raw title/prose/code
counts come from the source ledger. Unigram title+prose+code totals equal the
ledger's title+whole-body totals and, in these 629 columns, exactly match the
previous deduplicated-body-unit raw counts (zero differences).

Normalize each chosen source channel by its own L1 mass **within the frozen
629-word vocabulary**, then average nonempty channels:

- `title-body`: title and full prose+code body, equal mass.
- `title-prose-code`: title, prose and code, equal mass.
- `title-prose`: title and prose, code explicitly omitted from these features
  but preserved in the source ledger.

Empty channels do not contribute to the average; no admitted evidence leaves
an all-zero row. The matched full-source control also uses this vocabulary and
normalization. Earlier balanced controls normalized over 8,192 unigram/bigram
features, so comparison with those old scores is not a pure code-weight ablation.

Apply DF>=2 and DF<=.40 within each view, recording disabled words and retained
mass. Disabled columns are zeroed; keep the original 629-node unordered testing
family (197,506 pairs, including p=1 inactive pairs). The graph uses binary
parent presence, so positive rescaling alone cannot change topology.

Keep p05/BH05, seeds17/29/101, ten trials, one thread, hard mass/50-step sparse
PLSA, coverage scaling, thresholds .10/.20/.30, relative .5 and budgets3/8:
216 configurations. Hard/mixture mass denominators are the admitted eligible
view-word mass, including isolates. All seeds must achieve precision>=.70 and
>=20 links, followed by worst recall/precision/key selection. No fallback or
post-outcome cut repair. This is a shared-tag development proxy, not semantic
quality, hierarchy or an online contract.

## Outcomes

| View | Eligible words | Raw edges | Raw covered words | Raw topics (17/29/101) | Zero-input questions |
| --- | ---: | ---: | ---: | --- | ---: |
| Title/body | 629 | 2,787 | 592 | 37 / 35 / 36 | 0 |
| Title/prose/code | 629 | 2,787 | 592 | 37 / 35 / 36 | 0 |
| Title/prose | 433 | 1,039 | 373 | 35 / 34 / 33 | 0 |

All BH graphs have zero edges and explicitly unsupported models. All-source
views retain essentially all pre-filter mass; title/prose minimum retained mass
is .446 before the eligible-mass assignment denominator. Zero input rows are
different from no assigned topic: graph isolates/cutoffs can still cause
abstention.

Descriptive best precisions among configurations predicting >=20 links:

| View / assignment | TP / FP | Precision / recall | Unassigned / memberships | Threshold / seed |
| --- | --- | --- | --- | --- |
| Title/body / hard | 6 / 44 | .120 / .062 | 26 / 44 | .30 / 29 |
| Title/body / mixture | 8 / 27 | .229 / .082 | 0 / 65 | .10 / 17 |
| Title/prose/code / hard | 6 / 29 | .171 / .062 | 31 / 37 | .30 / 29 |
| Title/prose/code / mixture | 8 / 28 | .222 / .082 | 0 / 67 | .10 / 17 |
| Title/prose / hard | 6 / 27 | .182 / .062 | 30 / 37 | .30 / 101 |
| Title/prose / mixture | 10 / 35 | .222 / .103 | 2 / 66 | .20 / 101 |

All rows use budget3 and are post-outcome maxima, not winners or held-out gains.
The selector is null. Balancing channels changes memberships but leaves the
all-source communities unchanged; excluding code reduces coverage without
making the remaining representation adequate. These results do not prove that
code is noise or that weighting is generally useless.

## Verification and practical limits

Two fresh processes repeat every non-timing field. Independent ledger-to-matrix
reconstruction verifies source counts, scalar normalization, per-view DF/masks,
retained mass and zero-evidence lists. Exact combinatorial tails verify all
**1,185,036** probability/correction tests. Independent assignments/confusion
counts verify **435,456** pair decisions. All nonempty model hard-mass/coverage
invariants are checked; three raw seed17 fits (one per view) are independently
replayed with dense EM, including all 51 likelihoods, count conservation and
final parameters. This is not full independent replay of all 18 models.
All **293** lab tests pass with the optional graph extra; two unrelated graph
integrations skip without it. New tests exercise code volume, zero-channel
fallbacks, source immutability and unsupported/malformed count rejection.

Total after imports is 4.001/4.264s for the research comparison. It includes
capture loading, source reconstruction/checks and multiple graph/mixture fits;
imports and output serialization are excluded. Zero additional inference and
downloads; earlier model work remains charged. No complete ingestion, CPU/RSS,
p95, update/delete/restart or <=10% throughput gate is established. Source
coverage and repeatability do not establish useful topic labels or ancestors.

Outside-Git artifacts under `topic-public-stackoverflow-20261002`:

- `weighted-first.json`: `95d1bbbb298887322fc6de3566ff10328e9b5b33110748b3f22256a1a20a55ad`
- `weighted-second.json`: `d7908f45ece4f0bbfc79790821b0eb43921cb46e8f79e87d0fd24644badb0402`
- `verify-weighted.py`: independent counts/masks/probability/pair/dense-EM audit.

Before extending the graph parameter grid again, prioritize independent
source-grounded topic judgments and the validity/granularity of the shared-tag
proxy. Preserve all exposed failures. A blind review packet must separate model
predictions/tag metadata from original source context and record uncertain or
multi-topic relationships; no assistant-generated judgments are human gold.
Production selection still requires genuinely new groups, guided-search evidence,
semantic ancestry and complete maintenance/performance gates.
