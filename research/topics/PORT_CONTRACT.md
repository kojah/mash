# Python reference and Rust port guardrails

Status: required promotion contract, tracked in `mash-c0d.19.2.26`. No topic
algorithm is selected, and no production Rust parity CI gate exists yet.
Research candidates and their private captures are not the final contract.

The first milestone is flat, multi-label topics (decision 2026-10-02).
Semantic ancestry, parent/child links and inherited tags are not requirements
or promotion gates. Historical hierarchy examples below apply only to an
explicitly optional future hierarchy experiment, not this milestone.

Use the exact scientific reference environment described in
[SCIENCE_ENVIRONMENT.md](SCIENCE_ENVIRONMENT.md), rather than assuming the
broader research lock reproduces historical captures. A matching environment
is necessary evidence, not selected-method Rust parity or semantic quality.

The development-only [stage comparator](STAGE_PARITY.md) and adversarial
standard-library tests now implement the comparison boundary and run in CI.
They enforce pinned fixture/policy provenance, ordered stage coverage, exact
discrete decisions and explicitly named finite numeric tolerances. They do not
implement the selected algorithm's typed schemas, Python/Rust runners or a
complete port gate; those requirements remain open.

Once quality and ingestion gates select a method, pin the executable Python
reference, dependencies, algorithm parameters and fixture schema to a reviewed
commit. Changes to the contract must update fixtures and both implementations
together, with a recorded reason and quality/performance regression results.
Keep Python a development/CI dependency; production execution stays Rust.

| Stage | Required evidence |
| --- | --- |
| Source units | Exact identities, source hashes, role, text, boundaries and complete coverage/explicit exclusions |
| Context | Same selected original source identities or same abstention and reason; no fabricated approval |
| Inference | Pinned model/tokenizer/pooling/truncation/quantization; measured vector tolerance and downstream decision checks |
| Clustering | Identical fixed input vectors; defined normalization, precision, tie order, thresholds, insertion decisions and abstentions |
| Labels | Same source-adjacent token counts, background statistics, scoring formula, candidate eligibility and deterministic ties |
| Thread tags | Same multi-topic memberships and original evidence under scoped canonical identities |
| Lifecycle | Defined edits/deletes/late-arrival transitions, persisted state and restart recovery before those features ship |

Offsets require special care: Python research spans use Unicode code points,
Rust strings normally index UTF-8 bytes. Fixtures must include both coordinate
forms derived from the same source. Test accents, combining marks, emoji, CRLF,
non-ASCII whitespace and embedded code. Convert only at valid character
boundaries; reject offsets inside a UTF-8 character. Do not confuse either form
with UTF-16 code units, graphemes or tokenizer offsets.

Keep topical relevance separate from requested scope/stance wherever the selected
method represents them. Rejected proposals can still be topic evidence, and mixed
units must not become implicitly accepted passages. Fixtures must preserve both
decisions, including exclusions and abstentions; matching a source record alone
does not prove equivalent context selection. The two-axis development diagnostic
is not a selected production classifier or a substitute for independent judgments.

The reference runner and Rust runner must emit the same versioned stage schema.
Exercise serialization round-trips before differential comparison: the emitted
Python object must have the same schema after JSON decoding. For example, a
library's tuple-valued n-gram setting must be emitted as the schema's array,
otherwise a repeat can fail on representation despite identical algorithm results.
Keep that conversion explicit at the output boundary; do not broadly coerce
types or normalize away real source/model/policy drift. Include schema-type checks
for numeric, boolean, enum and array fields in the selected fixture contract.
The differential comparator reports the first mismatching stage, source fixture,
expected/actual identities and bounded numeric differences. Compare memberships
as sets of stable canonical source identities, not incidental cluster numbers.
For ties and assignment decisions, retain explicit canonical tie rules and event
traces. If comparing an optional hierarchy experiment, do not canonicalize away
a genuinely different parent relationship.
Freeze the linkage formula and aggregation weights explicitly. Normalized-centroid
cosine differs from the dot product of unnormalized vector means (average
cross-leaf cosine for unit inputs); silently normalizing a mean changes the method.
Include unequal cluster sizes, diffuse/canceling sums and score/order ties in
shared fixtures. Record the selected linkage in the versioned stage schema;
the exposed Python ablation is not a selected Rust port or parity CI gate.

Compare corresponding stages, including single-unit fields. Payload weighting
and field normalization can change floating-point bits even when a field has
only one already-normalized unit. The complete-source semantic experiment found
a maximum component difference of 2.78e-17 for its first query between the raw
unit and aggregated field; both match exactly when compared with the same stage.
Do not silently bypass singleton aggregation or compare a raw unit against a
field vector. Freeze the order and precision of weighting, division and every
normalization. Include non-axis-aligned singletons and unequal-weight multi-unit
fixtures. Also freeze inference batch shape/composition or explicitly validate
its allowed drift: the installed quantized encoder's padded-batch probe changed
directions despite correct attention-masked pooling. Neither observation is
evidence of a production Rust defect or permission to relax discrete decisions.

Freeze synthetic/non-sensitive golden fixtures in Git. Include empty inputs,
duplicates, zero and canceling vectors, nonfinite/malformed input rejection,
equidistant candidates, near-threshold values on both sides, insertion-order
permutations, multi-topic units and unassigned noise. Deterministic generated
cases supplement golden cases and verify aggregate/source invariants after
every transition. Edits/deletes/restarts join this suite once their intended
lifecycle semantics are defined; do not invent unsupported operations for parity.

Separate two gates:

1. Fixed-vector algorithm parity isolates extraction/selection, clustering,
   labels and tags from inference. Require exact discrete decisions and
   justified, explicitly bounded numeric differences in intermediate scores.
2. Full-path inference parity pins assets/preprocessing and measures vector
   drift per supported platform. A blanket epsilon must not excuse changed
   memberships near a cutoff. Exercise near-threshold fixtures, report any
   discrete drift and resolve its policy before promotion.

CI must run both reference and Rust runners, validate complete fixture coverage,
reject schema/source/model drift, and fail on unexplained differences. A
successful Python self-repeat is a prerequisite, not evidence of Rust parity.
Private held-out corpora need a separate authorized runner with pinned manifests;
do not commit histories or depend on private credentials for public fixture CI.

Parity, semantic quality and performance are independent gates. Require matched
held-out assignments/labels and guided-search evaluation, plus complete initial
and incremental ingestion measurements (including edits/deletes, persistence,
CPU/RSS and p95), against the provisional ≤10% throughput-loss target. A faithful
port of the current weak candidates must not pass production selection merely
because it matches Python.
