----------------------------- MODULE RetryViews -----------------------------
EXTENDS Naturals

\* One native chat, with 0 = absent, 1 = retained content, 2 = changed retry.
\* Silver can commit a deletion before any view write. Retry reconstructs its
\* decisions from durable silver; the previous request's deletion set is lost.
CONSTANTS ValidatePayload, PurgeAbsent
VARIABLES silver, docs, mark, payload, phase
vars == <<silver, docs, mark, payload, phase>>

Init == /\ silver = 1 /\ docs = 1 /\ mark = 1
        /\ payload = 0 /\ phase = "idle"

CommitDelete == /\ phase = "idle" /\ silver = 1
                /\ silver' = 0
                /\ UNCHANGED <<docs, mark, payload, phase>>

BeginRetry ==
    /\ phase = "idle"
    /\ \E supplied \in (IF silver = 0 THEN {0} ELSE 0..2) :
         payload' = supplied
    /\ phase' = "validate"
    /\ UNCHANGED <<silver, docs, mark>>

\* Candidate rows live in an uncommitted SQLite transaction. They must never
\* replace durable silver merely because a retry reused its revision number.
Validate == /\ phase = "validate"
            /\ phase' = IF ~ValidatePayload \/ payload = silver THEN "rollback" ELSE "rejected"
            /\ UNCHANGED <<silver, docs, mark, payload>>

Rollback == /\ phase = "rollback" /\ phase' = "metadata"
            /\ UNCHANGED <<silver, docs, mark, payload>>

\* A separate tiny SQLite transaction updates only last_synced_at (abstracted
\* away here). The node writer lock spans validation, rollback and metadata.
Metadata == /\ phase = "metadata" /\ phase' = "ready"
            /\ UNCHANGED <<silver, docs, mark, payload>>

BeginViews == /\ phase = "ready" /\ mark' = 3 /\ phase' = "write"
              /\ UNCHANGED <<silver, docs, payload>>

CrashBeforeViews == /\ phase \in {"validate", "rollback", "metadata", "ready"}
                    /\ phase' = "idle"
                    /\ UNCHANGED <<silver, docs, mark, payload>>

Write == /\ phase = "write"
         /\ docs' = IF silver = 0 /\ ~PurgeAbsent THEN docs ELSE payload
         /\ phase' = "certify"
         /\ UNCHANGED <<silver, mark, payload>>

\* Actual certification derives its fingerprint from silver, not from checking
\* physical Qdrant contents. Validation and cleanup must establish agreement.
Certify == /\ phase = "certify" /\ mark' = silver /\ phase' = "done"
           /\ UNCHANGED <<silver, docs, payload>>

Next == CommitDelete \/ BeginRetry \/ Validate \/ Rollback \/ Metadata \/ BeginViews
        \/ CrashBeforeViews \/ Write \/ Certify
Spec == Init /\ [][Next]_vars
TypeOK == /\ silver \in 0..1 /\ docs \in 0..2 /\ mark \in 0..3
          /\ payload \in 0..2
          /\ phase \in {"idle", "validate", "rollback", "metadata", "ready", "rejected", "write", "certify", "done"}
CertifiedMatchesSilver == mark = silver => docs = silver
=============================================================================
