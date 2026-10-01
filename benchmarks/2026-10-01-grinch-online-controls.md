# Online hierarchy grouping controls

This is a geometry and runtime experiment, not a selected production topic
method. It uses no LLM and performs no new embedding inference. Topic label
quality, overlapping subjects, semantic ancestors and guided-search gains remain
unevaluated here.

The independent Python control follows the [GRINCH paper](https://arxiv.org/abs/2001.00076)
and reviews the [authors' pinned Scala source](https://github.com/iesl/grinch/tree/a2747d3c5d3405cf27afc839bc28352b99c3e309).
It implements greedy insertion, rotation ascent, grafting with strict mutual
preference and post-graft restructuring. It uses float64, exact leaf neighbors,
stable arrival-order ties and a zero direction for epsilon-sized aggregate sums.
These choices and LCA-bounded restructuring differ from the reference; this is
not reference execution, byte parity or a claim of the paper's theorem.

The frozen data supplies 538 development thread means in 384 dimensions; 126
reserved groups remain excluded. User vectors are the existing compatible
quantized generation, averaged after canonical duplicate removal. This is raw
whole-thread geometry, not admitted dialogue episodes or new text embeddings.
The unchanged source-group partition and dataset fingerprint bind the experiment.
Compare forward/reverse arrival and shuffle seed 17, each in two fresh processes.
The synthetic comparison uses 96 normalized two-coordinate path vectors across
six disjoint 17-coordinate blocks. Synthetic labels enter evaluation only.

Online BIRCH receives one point per `partial_fit`, with threshold 0.4, branching
factor 64 and no final global clustering. Tree cuts use cosine 0.7 and accept a
clade only if every internal merger meets it, handling linkage inversions. These
are different uncalibrated operating points. Cluster counts and cross-order
partition agreement are not semantic quality comparisons between methods.

Xenia exposes 16 x86-64 logical CPUs on an AMD EPYC 9645 host. Python is 3.11.15,
NumPy 2.4.6 and scikit-learn 1.9.1; numerical thread limits are one. Timings include
incremental tree/BIRCH updates; source loading, whole-tree validation, cuts,
signatures and evaluation are outside the fit interval. Ranges below cover all
three orders and both fresh processes, not confidence intervals.

| Session method | Fit time, ms | Per-insert p95, ms | Cut clusters: forward / reverse / shuffled |
| --- | ---: | ---: | --- |
| Greedy | 49–70 | 0.113–0.193 | 135 / 147 / 129 |
| Rotate | 85–111 | 0.270–0.338 | 75 / 85 / 78 |
| GRINCH-style | 641–1,392 | 2.492–5.765 | 78 / 88 / 97 |
| BIRCH | 197–232 | 0.536–0.588 | 141 / 137 / 137 |

Peak process RSS is 188–190 MiB across these session controls, including scientific
imports and corpus loading. These are Python process values, not Rust worker
memory. GRINCH-style grouping performs 74–169 grafts across arrival orders. No
post-graft restructuring swap occurs on these captures; that operation has a
separate adversarial fixture verifying aggregate/membership correctness. It is
included rather than silently ablated.

Order stability compares forward against reverse and shuffled respectively.
Triplet agreement uses the closest pair by LCA in 4,096 fixed canonical triples
(seed 20261001), independent of arrival identities. Clade Jaccard excludes root
and singleton leaves. Native-cut ARI uses [scikit-learn's adjusted Rand score](https://scikit-learn.org/stable/modules/generated/sklearn.metrics.adjusted_rand_score.html).

| Session method | Native-cut ARI | Triplet agreement | Nontrivial clade Jaccard |
| --- | --- | --- | --- |
| Greedy | 0.519 / 0.877 | 0.476 / 0.405 | 0.110 / 0.123 |
| Rotate | 0.649 / 0.717 | 0.679 / 0.663 | 0.324 / 0.301 |
| GRINCH-style | 0.987 / 0.797 | 0.810 / 0.890 | 0.360 / 0.315 |
| BIRCH | 0.963 / 0.713 | Not measured | Not measured |

GRINCH-style grafting improves the observed hierarchy-order diagnostics over
rotation alone, at higher exact-search/update cost. Clade overlap remains modest:
stronger triplet agreement does not establish stable topic identities or correct
semantic ancestry. BIRCH has strong native-cut reverse agreement at its own radius
and substantially lower cost than this GRINCH-style control. These results do not
select a winner or justify changing application defaults.

| Synthetic method | Dendrogram purity: forward / reverse / shuffled |
| --- | --- |
| Greedy | 0.983 / 0.983 / 0.335 |
| Rotate | 1.000 / 1.000 / 0.336 |
| GRINCH-style | 1.000 / 1.000 / 1.000 |
| BIRCH | No equivalent binary dendrogram measured |

The shuffled synthetic GRINCH-style run makes 35 grafts. Every native cut, including
BIRCH, produces 96 singleton clusters: ARI against the six true classes is zero
despite perfect cross-order cut agreement. This exposes why neither an arbitrary
cutoff nor agreement alone establishes a useful topic partition. Synthetic tree
purity is a sanity check; we do not verify model separation for all subsets or
claim semantic session purity.

All 24 matching fresh-run pairs reproduce every non-timing field, including
memberships, tree signatures, operations and provenance. Ninety-five lab tests
pass, including every-insertion tree integrity, invalid-input preservation,
canceling/identical vectors, a restructuring recovery fixture, five synthetic
orders, known purity values, canonical triplet identity and comparison rejection
for changed sources/code/sampling or incomplete partitions.

The 48 private captures and `summary.json` live under
`/home/james/scratch/mash-artifacts/grinch-controls-20261001/validated`.
Summary SHA256:
`bb46074e2865f7c1826c4a3e89b8e0cb95aaf6ed67bfd5c9cfc61b685b0bf07f`.
Each capture pins implementation/lock hashes, generation, canonical sources,
arrival identities, library versions and the verified source-reference manifest.
The summary pins every capture and comparator bytes.

Reproduce the individual controls with `research/topics/grinch_controls.py`, then:

```bash
python research/topics/compare_grouping.py \
  --captures "$PRIVATE_CAPTURE_DIRECTORY" --output "$FRESH_PRIVATE_SUMMARY"
```

Next use source-reviewed dialogue units and label/grouping judgments, measure
granularity and secondary-topic fidelity, and evaluate versioned edit/delete and
full topics-on/off ingestion. The prototype is insert-only and does not expose
topic MCP tools or imply that geometric parent nodes are valid inherited tags.
