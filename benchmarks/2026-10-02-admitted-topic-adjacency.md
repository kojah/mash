# Admitted-text adjacency does not change this cohort's topic units

The paired control reuses frozen vectors from the complete-window experiment
(`31e2f6a9e61791e57535ebddf82172775b5f140bc6a63875f0164d5d44bbdc94`)
and original source baseline
(`367d53bff53eb4186123992b3fbad84fd8ea900bce6829d701dfe58783125cc7`).
It makes no inference calls and changes no source/embedding/label eligibility.
The same 149 canonical user-role records include 74 admitted texts and 75
excluded empty projections. They are not 75 uncompleted embeddings.

The `Adjacency` enum explicitly distinguishes the existing canonical-break
policy from admitted-text adjacency. In the latter, only literal empty text
with absent geometry is retained as a separate singleton without ending a
pending topical unit. Nonempty text lacking geometry still ends the unit;
there is no fallback context borrowing or embedding of excluded canonical
content. Both policies enforce provider/thread boundaries, eight-turn/8192-byte
join limits and the unchanged .55 cosine threshold. Pending units are emitted
in completion order; excluded records are still preserved, not removed from the
capture. Reverse arrival reverses formed units, not native unit formation.

## Observations

In both first-window and complete-window representations, the two policies
produce exactly the same source partition: 144 units, three multi-turn units,
maximum four members. Forward/reverse co-membership statistics are unchanged:
first-window 59 leaves and .875 pair Jaccard; complete-window 58 leaves and
.961538. Complete-window has 45 singleton leaves and 47 weak-label leaves.
Every source remains accounted for, and excluded records remain unembedded.
The canonical arm exactly reproduces both stored unit snapshots from the
preceding experiment, despite the new optional policy.

A scoped independent check finds 18 adjacent admitted-text pairs separated by
one or more excluded records. Their maximum scalar cosine is .391520 in both
representations, and none reaches .55. This checks adjacent point scores, not
every pooled-centroid join/rejection or semantic relevance. The lack of new
units therefore does not establish a universal policy equivalence or disprove
segmentation. It rules out forced exclusion boundaries as the fragmentation
correction on this particular small exposed cohort. No thresholds were lowered.

Two sequential runs match all non-runtime outputs. Three new fixtures cover
skipping only explicit exclusions, preserving unknown missing geometry as a
boundary, typed policy validation and provider scope. The existing fixtures
continue to verify canonical defaults and source/budget conservation.
Private audit `topic-admitted-adjacency-audit-20261002.json` records source
conservation, unit-partition equality and scalar opportunity counts; it does
not reconstruct model inference, labels or semantic quality.

Four-arm/two-order computation takes 2.023/5.566s wall and 2.013/2.712s CPU.
Host contention clearly limits timing comparisons; arm ordering is fixed.
External target RSS maxima are 145956/143972 KiB and PSS 136072/134255 KiB.
Status HWM matches RSS here; rusage is retained separately at 145472/142880 KiB
without subtraction or launcher attribution. Sampling has 160/315 observations
and zero/two vanished reads. Target scope includes imports, input loading and
all arms/orders; child and observer memory are excluded, monitoring intrusive
and sampled maxima lower bounds. These are not production full-ingestion gates.

First/repeat outputs and memory sidecars have prefix
`topic-admitted-adjacency-*-20261002` under mash-artifacts. Dependency hashes are
captured; production default behavior and model assets are unchanged. Next move
the end-to-end pipeline evaluation to the existing frozen 128-thread cohort
and its 1,850 admitted user-message occurrences, restoring within-thread exact
duplicates and original identities from SQLite rather than treating the older
1,812 deduplicated vectors as all source occurrences. Measure capacity, labels,
coverage, order sensitivity and scoped runtime before more small-cohort tweaks.
Independent coding quality, full ingestion and selected-method Rust gates stay
open; no method is selected from these structural checks.
