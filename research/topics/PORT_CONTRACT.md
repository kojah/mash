# Python reference and Rust port guardrails

Status: required promotion contract, tracked in `mash-c0d.19.2.26`. No topic
algorithm is selected, and no production Rust parity CI gate exists yet.
Research candidates and their private captures are not the final contract.

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
| Tags/tree | Same multi-topic memberships, ancestry and inherited tags under canonical identities |
| Lifecycle | Defined edits/deletes/late-arrival transitions, persisted state and restart recovery before those features ship |

Offsets require special care: Python research spans use Unicode code points,
Rust strings normally index UTF-8 bytes. Fixtures must include both coordinate
forms derived from the same source. Test accents, combining marks, emoji, CRLF,
non-ASCII whitespace and embedded code. Convert only at valid character
boundaries; reject offsets inside a UTF-8 character. Do not confuse either form
with UTF-16 code units, graphemes or tokenizer offsets.

The reference runner and Rust runner must emit the same versioned stage schema.
The differential comparator reports the first mismatching stage, source fixture,
expected/actual identities and bounded numeric differences. Compare memberships
as sets of stable canonical source identities, not incidental cluster numbers.
For ties and topology decisions, retain explicit canonical tie rules and event
traces; do not canonicalize away a genuinely different parent relationship.

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
