--------------------------- MODULE LegacyActivation ---------------------------
EXTENDS Naturals
CONSTANT GuardRollback
VARIABLES phase, oldRunning, candidateRunning, changed, marker
vars == <<phase, oldRunning, candidateRunning, changed, marker>>
Init == /\ phase = "idle" /\ oldRunning = TRUE
        /\ candidateRunning = FALSE /\ changed = FALSE /\ marker = FALSE
Prepare == /\ phase = "idle" /\ phase' = "prepared" /\ marker' = TRUE
           /\ UNCHANGED <<oldRunning, candidateRunning, changed>>
Stop == /\ phase = "prepared" /\ phase' = "stopped" /\ oldRunning' = FALSE
        /\ UNCHANGED <<candidateRunning, changed, marker>>
Activate == /\ phase = "stopped" /\ phase' = "activating"
            /\ UNCHANGED <<oldRunning, candidateRunning, changed, marker>>
Start == /\ phase = "activating" /\ phase' = "started" /\ candidateRunning' = TRUE
         /\ UNCHANGED <<oldRunning, changed, marker>>
Change == /\ candidateRunning /\ ~changed /\ changed' = TRUE
          /\ UNCHANGED <<phase, oldRunning, candidateRunning, marker>>
Exit == /\ candidateRunning /\ candidateRunning' = FALSE
        /\ UNCHANGED <<phase, oldRunning, changed, marker>>
Finish == /\ phase = "started" /\ candidateRunning
          /\ phase' = "complete" /\ marker' = FALSE
          /\ UNCHANGED <<oldRunning, candidateRunning, changed>>
Rollback == /\ phase \in {"stopped", "activating", "started"}
            /\ (~GuardRollback \/ phase = "stopped")
            /\ phase' = "restored" /\ oldRunning' = TRUE
            /\ candidateRunning' = FALSE /\ marker' = FALSE
            /\ UNCHANGED changed
Crash == /\ phase \in {"prepared", "stopped", "activating", "started"}
         /\ phase' = "interrupted"
         /\ UNCHANGED <<oldRunning, candidateRunning, changed, marker>>
Next == Prepare \/ Stop \/ Activate \/ Start \/ Change \/ Exit \/ Finish \/ Rollback \/ Crash
Spec == Init /\ [][Next]_vars
NoOldBinaryAfterChange == changed => ~oldRunning
InterruptedNeedsRecovery == phase = "interrupted" => marker
=============================================================================
