# Coding-chat submissions: typed source-backed validation

P0 `mash-afc.1.2`. This validates incoming development review structure and
provenance; **no human judgments have been received and no topic method is
selected**. It does not score topic quality, agent utility or reviewer agreement.

`chat_review.py` and `chat_review_models.py` form a standard-library executable
boundary. Known packet/reviewer/subject/task/evidence/constraints schemas parse
into frozen dataclasses and enums. JSON remains at parsing/output boundaries.
Require literal booleans/version, complete unique document IDs, original turn
ordering/source hashes, exact quotes from unredacted turns in the same document,
known importance/task/metadata enums, and bounded text/arrays/JSON structure.
Reject duplicate keys, malformed JSON, nonfinite literals, invalid timestamps,
unknown keys, wrong packet hash and unanswered/null judgments. Canonicalize
review document order to packet order. Do not treat an empty list as missing:
it is an explicit judgment requiring explanatory notes. Insufficient context
requires notes/subject uncertainty and forbids purportedly answerable tasks.

The blind packet has no authenticated provider/time/node metadata. Requested
scopes must be explicitly unavailable, not asserted as source facts. Time ranges
have valid timezone timestamps and ordered bounds. Scored scoped retrieval
remains impossible until metadata is independently available. Corpus-wide
negative tasks cannot be derived from this single-chat packet. Full details:
[submission contract](../research/topics/CHAT_REVIEW_SUBMISSION.md).

Human attestation and exposure declarations are self-reports. Exact evidence
proves that a quote exists, not that a subject interpretation is correct or an
assistant suggestion was accepted. Aggregate output reports only structure/
context/metadata-gap counts and always marks independent gold, semantic quality
and agent utility unproven. Neither source identities/labels/requests/quotes nor
raw private values appear in output or schema errors. Literal free-text subject
equality is not semantic agreement; aligned/adjudicated actual human reviews
and fresh group-separated tasks remain required. No LLM/inference/API calls.

Thirteen synthetic fixtures cover typed parsing, literal declarations, null/
duplicate/missing identities, changed/cross-chat/redacted/empty evidence,
malformed enums/ancestor proposals, explicit empty/context uncertainty,
missing metadata, timezone/range correctness, original packet hash/order,
duplicate/nonfinite/deep JSON, review-order invariance and aggregate-only CLI
success/error output. These fixtures do not populate actual review answers or
count as human annotations. Python depth behavior differed between installed
versions; an explicit64-level/250,000-node guard now prevents reliance on an
interpreter-specific recursion failure. Files are capped at16MiB.

Actual packet `coding-chat-review-first-20261002/review.json` (SHA256
`cb8c2aa9e737127c48f90fada40a1a8383dcea94dac17c276287f2820780e682`)
loads all7 documents/281 indexed turns; original blank answers are rejected.
Final preflight artifact outside Git:
`/home/james/scratch/mash-artifacts/chat-review-blank-validation-final-20261002.json`.
Validator code SHA256
`bd2f2095417d49e3f89889904f713510c64c33219f7ab16ce06be120b88e2783`;
model code SHA256
`c646d32b14341efe27792b64b514427954b82273d41fe30d4cb72a2a7e4934a2`.
An earlier preflight also rejected blanks; final pins include the corrected
explicit structural bound. No actual complete submission was synthesized.

Standard-library fixture run13 tests passes; full enabled scientific reference
suite610 tests passes in3.691s. Add the stdlib fixtures to existing CI without
models/scientific dependencies/private data; remote CI remains unverified here.
No Rust/application/deployment change, selected-port parity or ingestion gate.

Next: await actual independent reviews while progressing other authorized work;
do not treat pending reviewer input as a reason to stop the overall goal.
Keep the already exposed pilot exploratory. Independent semantic/agent evidence,
full online lifecycle/ingestion overhead and selected Rust parity remain open.
