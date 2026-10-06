# Qdrant publication recovery and staging retention

> Superseded on 2026-10-06: shared silver moved to SQLite and Qdrant now holds
> only rebuildable gold. See [shared storage](shared-storage.md). The staging
> and conditional-publication behavior below no longer exists.

An ambiguous publication error now retains the candidate's immutable staging
data until completion is known. An immediate read of the old node head is not
proof that the pending conditional update cannot publish later.

Qdrant's [upsert API](https://api.qdrant.tech/api-reference/points/upsert-points)
distinguishes acknowledgment from completion. Its official
[client documentation](https://github.com/qdrant/qdrant-client/blob/master/qdrant_client/qdrant_client.py)
also describes a commit wait timeout as acceptance without application yet.
Network failures and unsuccessful responses cannot establish cancellation.
The mash client requires a completed operation response; other outcomes remain
ambiguous for the publication caller.

After staging completes, mash conditionally updates the node head and reads it
back. Recovery uses the candidate's unique immutable version:

| Conditional write | Observed head | Behavior |
| --- | --- | --- |
| Completed | Candidate version | Return success; clean the prior version. |
| Error or incomplete acknowledgment | Candidate version | Recover success from the observed publication; clean the prior version. |
| Completed | Other version | Clean the losing candidate and return a revision conflict. |
| Error or incomplete acknowledgment | Other version | Retain the candidate and return the original error. The request could still publish later. |
| Any result | Verification fails | Retain the candidate and return the verification error. |

Previously the fourth case deleted staging before returning the request error.
If the conditional head write then applied, its manifest referred to deleted
chunks. The regression fixture reproduced both deletion requests while the
candidate was queued. The corrected path emits a warning with the revision and
retains staging. It adds no extra request to successful publications.

`app/shared/qdrant_index/tests/publication.rs` drives the actual HTTP client,
`Store::commit`, head verification and `Store::load` through a controlled local
service. It exercises late completion after HTTP failure and incomplete
acknowledgment, unavailable verification followed by completion, already applied
publication with a failed response, and a completed loser beside another writer's
complete snapshot. The read checks manifest/chunk digest recovery. This controlled
fault service is not proof of Qdrant cluster failover, crash recovery or the
frequency of these errors in production. Existing disposable Qdrant tests cover
normal real-backend publication, read/search and schema behavior separately.

Ambiguous operations can retain hidden storage. Safe bounded reclamation remains
tracked in `mash-pz5`; it requires distributed writer fencing and published/read
version protection before abandoned versions can be deleted. Age alone, a single
head scan or a process-local lock is insufficient. This correction does not add
automatic garbage collection or mutate existing collection schemas.

Validation artifacts are outside Git under `/home/james/scratch/mash-artifacts`:
`qdrant-ambiguous-cas-original-20261002.log` preserves the regression failure;
`qdrant-ambiguous-cas-fixed-20261002.log` records the corrected fault tests.
The full Rust/CLI run passed 570 tests (`qdrant-ambiguous-cas-suite-20261002.log`).
All 19 disposable Qdrant 1.19.1 integration tests passed, including the installed
local-model relevance matrix (`qdrant-ambiguous-cas-real-configured-20261002.log`).
The earlier real-backend run stopped because the relevance test's required
model-directory environment setting was omitted; that failure remains recorded
in `qdrant-ambiguous-cas-real-20261002.log`. No model download was needed.
All-target Clippy with warnings denied, formatting, and the doctest also passed.
The initial Clippy report and corrected run are retained separately; the only
correction reverses a negated conditional in the controlled test service.
