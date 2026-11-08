--------------------------- MODULE BackfillTarget ---------------------------
EXTENDS Naturals, TLC
CONSTANT SkipPreparation
VARIABLES target, revision, acknowledged, captured, remote, phase
vars == <<target, revision, acknowledged, captured, remote, phase>>

Init == /\ target \in {"unset", "previous"}
        /\ revision = 1
        /\ acknowledged = IF target = "previous" THEN 1 ELSE 0
        /\ captured = 0 /\ remote = 0 /\ phase = "idle"

\* One SQLite transaction, under the destination publication lock.
Prepare == /\ phase = "idle"
           /\ revision' = IF target # "current" /\ acknowledged > 0
                          THEN revision + 1 ELSE revision
           /\ target' = "current" /\ phase' = "prepared"
           /\ UNCHANGED <<acknowledged, captured, remote>>
Capture == /\ (phase = "prepared" \/ (SkipPreparation /\ phase = "idle"))
           /\ captured' = revision /\ phase' = "captured"
           /\ UNCHANGED <<target, revision, acknowledged, remote>>
\* Abstracts complete chat coverage and the separate record upload receipt.
Publish == /\ phase = "captured"
           /\ remote' = captured /\ phase' = "receipt"
           /\ UNCHANGED <<target, revision, acknowledged, captured>>
\* A mismatched destination makes the real conditional UPDATE affect zero rows.
Ack == /\ phase = "receipt"
       /\ acknowledged' = IF target = "current" THEN captured ELSE acknowledged
       /\ phase' = "done"
       /\ UNCHANGED <<target, revision, captured, remote>>
Crash == /\ phase \in {"prepared", "captured", "receipt"}
         /\ phase' = "idle"
         /\ UNCHANGED <<target, revision, acknowledged, captured, remote>>
Next == Prepare \/ Capture \/ Publish \/ Ack \/ Crash
Spec == Init /\ [][Next]_vars
TypeOK == /\ target \in {"unset", "previous", "current"}
          /\ revision \in 1..2 /\ acknowledged \in 0..2
          /\ captured \in 0..2 /\ remote \in 0..2
          /\ phase \in {"idle", "prepared", "captured", "receipt", "done"}
CompletedIsAcknowledged == phase = "done" =>
    (target = "current" /\ acknowledged = captured /\ remote = captured)
=============================================================================
