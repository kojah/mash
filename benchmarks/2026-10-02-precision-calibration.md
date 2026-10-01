# Precision-constrained topic cuts and thread-tag diagnostics

The two frozen intent settings retain at least 70% observed unit-pair precision
on a separate sample in all three tested arrival orders. Average linkage is
steadier across orders, but centroid has higher forward recall. Neither setting
establishes useful thread navigation: thread proxies are so prevalent that an
all-connected baseline scores highly. No production method is selected.

Tracked by `mash-c0d.19.2.32`. Python-only, no generative calls, paid services,
downloads, application changes or Rust parity claims.

## Frozen procedure

Preregistered before outcomes in Beads. `research/topics/precision_calibration.py`
excludes all 64 groups from the preceding public diagnostics and selects 32 new
groups with seed `mash-topic-multiwoz-precision-v1-20261001`: first 16 calibration,
next 16 reserved. Retaining the seed's date makes the split reproducible; this
report was completed October 2. Source/model/dependency pins are unchanged from
the [earlier calibration](2026-10-01-cluster-calibration.md).

This is an outcome-separated development experiment, **not independent coding
topic gold**. The same public development shard has been structurally inspected;
new sample texts were previously eligible as lexical background. Service and
declared service/intent intersections are incomplete conversational topic proxies.
Templates, model pretraining overlap and cumulative-state effects remain possible.

Fit the fixed annotation-free TF-IDF policy on the remaining 416 groups, excluding
all 96 selected groups, exact selected USER strings and duplicate background
strings. There are 3,137 source USER turns, 24 exact-string exclusions and 26
duplicate exclusions: 3,087 unique strings, 3,458 vocabulary entries and 55,645
nonzero features. The identical vocabulary, IDF and background manifest are
required in both phases.

Use current, joined, lexical, current-75%/lexical-25%, and joined-75%/lexical-25%
representations. Installed MiniLM/tokenizer assets are hash-checked. Admission is
annotation-independent: root context, model window, exact calibration input and
zero lexical support cause explicit abstention. Inference is singleton CPU with
one thread, no padding/truncation, phase preflight cap of 1,024 unique strings.
Both dense views are required for matched support even for lexical candidates.

Fit ten forward GRINCH-style trees (five views × centroid/average linkage); reuse
each at cuts `.1,.2,.3,.4,.5,.6,.7,.8,.9`. For each linkage and objective, maximize
unit-pair recall subject to observed precision ≥.70 and at least 20 predicted
cross-group links; break ties by precision, then lexical candidate ID. Return
unsupported `None` if no candidate qualifies. This is a provisional calibration
criterion, **not a statistical precision guarantee or production acceptance bar**.
Full-intent is the primary proxy; service is a broad diagnostic.

Freeze the complete calibration artifact and hash before reserved execution.
The separate command requires that hash and verifies policy, splits, source/model,
code, dependencies, recomputed winners and lexical basis. Reserved execution
tests only four unique frozen winners plus the fixed current-centroid `.7`
baseline, in forward, reverse and seed-17 shuffled orders: 15 partitions from
nine reused trees. No reserved-based retuning or method selection.

## Support and unit outcomes

Calibration: 121 USER anchors minus 16 root-context abstentions = 105 fit units;
2,459 eligible cross-group pairs, 885 service and 526 intent positives. Reserved:
122 USER anchors minus 16 roots and one exact calibration input = 105 fit units;
3,129 eligible pairs, 1,161 service and 766 intent positives. Unknown/empty proxy
labels exclude pairs from scoring, never control tree admission.

| Frozen setting / objective | Calibration P | Calibration R | Reserved forward P | Reserved forward R |
| --- | ---: | ---: | ---: | ---: |
| Current average `.4` / intent | .7195 | .1122 | .7204 | .2624 |
| Current centroid `.6` / intent | .7143 | .0856 | .8862 | .3864 |
| Current average `.3` / service | .7465 | .2429 | .8200 | .3256 |
| Current+lexical centroid `.3` / service | .8537 | .1582 | .6489 | .4091 |

The service-selected lexical fusion fails the precision criterion on reserved
forward and shuffled arrivals (shuffled service precision .3969). Calibration
eligibility alone does not guarantee transfer, even on this related corpus.

| Intent setting | Arrival | TP | FP | FN | TN | P | R | F1 | ARI vs forward |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Average `.4` | Forward | 201 | 78 | 565 | 2285 | .7204 | .2624 | .3847 | 1.0000 |
| Average `.4` | Reverse | 196 | 59 | 570 | 2304 | .7686 | .2559 | .3839 | .9046 |
| Average `.4` | Shuffled | 198 | 57 | 568 | 2306 | .7765 | .2585 | .3879 | .8320 |
| Centroid `.6` | Forward | 296 | 38 | 470 | 2325 | .8862 | .3864 | .5382 | 1.0000 |
| Centroid `.6` | Reverse | 83 | 22 | 683 | 2341 | .7905 | .1084 | .1906 | .4756 |
| Centroid `.6` | Shuffled | 95 | 33 | 671 | 2330 | .7422 | .1240 | .2125 | .5284 |

Average's reserved partitions have 26/29/27 clusters, with 9/13/10 singletons;
centroid has 47/54/50 clusters, with 33/37/33 singletons. Average recall varies
only .2559–.2624; centroid varies .1084–.3864. Exact branches and semantic parents
are not measured by partition ARI. Differences across three dependent orders
are descriptive, not statistical evidence of superiority.

The fixed `.7` baseline has forward intent precision .9211 but recall .0457,
F1 .0871 and 82 clusters/70 singletons. All-one has intent P .2448, R 1 and
F1 .3933; singleton has R/F1 zero and undefined precision. Average `.4` forward
F1 is below all-one despite much higher precision: report both objectives rather
than infer quality from a single aggregate score.

## Multi-tag thread diagnostic and its ceiling

Each original dialogue receives the **set of direct cut-cluster IDs** represented
by all its fit units. This gives multiple tags per thread; individual units still
have one cut membership. It is not soft multi-membership, labeled topics, ancestor
inheritance or a validated semantic tree. Two threads are predicted related if
their direct tag sets intersect. Proxy truth is the union of known, nonempty
labels from the same matched fit units, not full-thread human judgments.

Calibration excludes one group without eligible proxy evidence, leaving 15
groups/105 pairs, 74 intent positives. Reserved has 16 groups/120 pairs, 95 intent
and 102 service positives. Predicted tags are retained even for groups excluded
from scoring. No thread outcomes influence calibration.

| Reserved forward thread intent | TP | FP | FN | TN | P | R | F1 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Average intent `.4` | 86 | 13 | 9 | 12 | .8687 | .9053 | .8866 |
| Centroid intent `.6` | 75 | 9 | 20 | 16 | .8929 | .7895 | .8380 |
| Fixed centroid `.7` | 33 | 1 | 62 | 24 | .9706 | .3474 | .5116 |
| All connected | 95 | 25 | 0 | 0 | .7917 | 1.0000 | .8837 |

Average `.4` threads have 4–7 direct tags; centroid `.6` has 3–8. Average `.4`
rejects only 12 of 25 unrelated proxy pairs (specificity .48); centroid `.6`
rejects 16 (specificity .64). Average's F1 gain over all-connected is just .0029.
Both service-selected forward settings connect **all 120 thread pairs**, exactly
matching the all-connected baseline. High overlap recall/F1 is therefore weak
evidence of navigational value. Need granular coding/aspect judgments, harder
unrelated thread pairs and equal-budget guided-search tasks before MCP promotion.

## Costs, verification and captures

| Phase | Calls | Max tokens | Total seconds (two runs) | Inference ms (two runs) | All tree fits wall ms (two runs) |
| --- | ---: | ---: | --- | --- | --- |
| Calibration | 226 | 63 | 5.011 / 4.071 | 772.09 / 614.95 | 1061.72 / 764.98 |
| Reserved | 227 | 73 | 2.178 / 1.915 | 475.69 / 424.33 | 584.59 / 481.37 |

Measured on Xenia, pinned Python lab with OMP/OpenBLAS/MKL threads set to one.
These totals include source loading, model construction/inference, background
features, multiple trees, cuts and proxy evaluation; output serialization occurs
after the total timer. They are not matched production ingestion measurements.
No RSS audit, edits/deletes, persistence, growing-corpus scaling or ≤10% overhead
gate is established by this experiment.

Both phases repeat exactly in every field except explicitly excluded top-level
timings, including vectors, splits, code/model/source pins, winners, partitions,
operation counts and thread tags. Separate recomputation verifies all 105
partitions' source coverage and unit/thread confusion counts, 96-group/background
separation and absence of calibration hashes in evaluated model inputs. A bad
calibration hash fails before model work and writes no capture. The lab suite
passes 204 tests; no Rust implementation changed.

Private captures under `topic-public-multiwoz-20261001`, outside Git:

- Calibration first: `c0dcfd2f4db61702af8692db98ec2a61763f0ab11f4755962944f6833f17140f`.
- Calibration repeat: `0e017e6b638242a33e09b40c7989334c946b2226b590438c1d16b6875c9ecd38`.
- Reserved first: `a67015fd9f2d192913886e74dd9c55411b9c98f781b96f5ed744fde7d27adba3`.
- Reserved repeat: `00a2d17909fb5541da94ea5be3a741854345ec8635bb826dcfafbc118799aaba`.

Next assess source-grounded granular coding units, independent label/ancestor
quality and thread discrimination; preserve these outcomes without further tuning.
Parity CI and complete initial/incremental ingestion gates remain required.
