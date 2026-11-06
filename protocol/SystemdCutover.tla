--------------------------- MODULE SystemdCutover ---------------------------
EXTENDS Naturals
CONSTANTS CoupledRetirement, GuardRollback
VARIABLES phase, oldRunning, newRunning, history, receipts, backup, ack, marker
vars == <<phase, oldRunning, newRunning, history, receipts, backup, ack, marker>>
Init == /\ phase = "idle" /\ oldRunning = TRUE /\ newRunning = FALSE
        /\ history = "old" /\ receipts = "old" /\ backup = FALSE
        /\ ack = FALSE /\ marker = FALSE
Prepare == /\ phase = "idle" /\ phase' = "prepared" /\ marker' = TRUE
           /\ UNCHANGED <<oldRunning, newRunning, history, receipts, backup, ack>>
Stop == /\ phase = "prepared" /\ phase' = "stopped" /\ oldRunning' = FALSE
        /\ UNCHANGED <<newRunning, history, receipts, backup, ack, marker>>
Retire == /\ phase = "stopped" /\ phase' = "retired" /\ backup' = TRUE
          /\ history' = "new"
          /\ receipts' = IF CoupledRetirement THEN "new" ELSE receipts
          /\ UNCHANGED <<oldRunning, newRunning, ack, marker>>
Activate == /\ phase = "retired" /\ phase' = "activating"
            /\ UNCHANGED <<oldRunning, newRunning, history, receipts, backup, ack, marker>>
Start == /\ phase = "activating" /\ phase' = "started" /\ newRunning' = TRUE
         /\ UNCHANGED <<oldRunning, history, receipts, backup, ack, marker>>
Accept == /\ newRunning /\ ~ack /\ ack' = TRUE
          /\ UNCHANGED <<phase, oldRunning, newRunning, history, receipts, backup, marker>>
Finish == /\ phase = "started" /\ phase' = "complete" /\ marker' = FALSE
          /\ UNCHANGED <<oldRunning, newRunning, history, receipts, backup, ack>>
Rollback == /\ phase \in {"retired", "activating", "started"}
            /\ (~GuardRollback \/ phase = "retired")
            /\ phase' = "restored" /\ history' = "old" /\ receipts' = "old"
            /\ oldRunning' = TRUE /\ newRunning' = FALSE /\ marker' = FALSE
            /\ UNCHANGED <<backup, ack>>
Crash == /\ phase \in {"prepared", "stopped", "retired", "activating", "started"}
         /\ phase' = "interrupted"
         /\ UNCHANGED <<oldRunning, newRunning, history, receipts, backup, ack, marker>>
Next == Prepare \/ Stop \/ Retire \/ Activate \/ Start \/ Accept \/ Finish \/ Rollback \/ Crash
Spec == Init /\ [][Next]_vars
NoLostAcknowledgements == ack => history = "new"
ReceiptsMatchHistory == history = receipts
RetirementStopsOwner == backup => ~oldRunning \/ phase = "restored"
InterruptedNeedsRecovery == phase = "interrupted" => marker
=============================================================================
