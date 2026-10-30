--------------------------- MODULE ConnectorViews ---------------------------
EXTENDS Naturals
\* One connector anchor; 0 means absent, 1 and 2 are distinct contents.
\* Evidence is the persisted digest, independently of physical point contents.
\* certified abstracts the persisted silver marker used by freshness.
\* The old writer cleared a separate flag but retained this marker.
CONSTANTS InvalidateEvidence, PurgeAbsent
VARIABLES silver, docs, known, certified, changed, phase, commits
vars == <<silver, docs, known, certified, changed, phase, commits>>
Init == /\ silver = 0 /\ docs = 0 /\ known = 0 /\ certified = TRUE
        /\ changed = FALSE /\ phase = "idle" /\ commits = 0
Commit == /\ phase = "idle" /\ commits < 3
          /\ silver' \in 0..2 /\ commits' = commits + 1 /\ phase' = "plan"
          /\ UNCHANGED <<docs, known, certified, changed>>
Plan == /\ phase = "plan" /\ changed' = (known # silver)
        /\ known' = IF InvalidateEvidence /\ known # silver THEN 0 ELSE known
        /\ certified' = IF InvalidateEvidence THEN FALSE ELSE certified
        /\ phase' = "write"
        /\ UNCHANGED <<silver, docs, commits>>
Write == /\ phase = "write"
         /\ docs' = IF silver = 0 THEN
                         (IF PurgeAbsent \/ known # 0 THEN 0 ELSE docs)
                     ELSE IF changed THEN silver ELSE docs
         /\ phase' = "finish"
         /\ UNCHANGED <<silver, known, certified, changed, commits>>
Finish == /\ phase = "finish" /\ known' = silver
          /\ certified' = TRUE /\ phase' = "idle"
          /\ UNCHANGED <<silver, docs, changed, commits>>
Crash == /\ phase \in {"write", "finish"} /\ phase' = "idle"
         /\ UNCHANGED <<silver, docs, known, certified, changed, commits>>
Next == Commit \/ Plan \/ Write \/ Finish \/ Crash
Spec == Init /\ [][Next]_vars
TypeOK == /\ silver \in 0..2 /\ docs \in 0..2 /\ known \in 0..2
          /\ certified \in BOOLEAN /\ changed \in BOOLEAN /\ commits \in 0..3
          /\ phase \in {"idle", "plan", "write", "finish"}
CurrentMatchesSilver == certified /\ known = silver => docs = silver
=============================================================================
