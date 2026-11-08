--------------------------- MODULE SystemdCutover ---------------------------
EXTENDS Naturals
CONSTANTS CoupledRetirement, GuardRollback
VARIABLES phase, oldRunning, newRunning, history, receipts, backup, ack, marker,
          binaryReady, unitReady
vars == <<phase, oldRunning, newRunning, history, receipts, backup, ack, marker,
          binaryReady, unitReady>>
Init == /\ phase = "idle" /\ oldRunning = TRUE /\ newRunning = FALSE
        /\ history = "old" /\ receipts = "old" /\ backup = FALSE
        /\ ack = FALSE /\ marker = FALSE /\ binaryReady = FALSE /\ unitReady = FALSE
Prepare == /\ phase = "idle" /\ phase' = "prepared" /\ marker' = TRUE
           /\ UNCHANGED <<oldRunning, newRunning, history, receipts, backup, ack, binaryReady, unitReady>>
Stop == /\ phase = "prepared" /\ phase' = "stopped" /\ oldRunning' = FALSE
        /\ UNCHANGED <<newRunning, history, receipts, backup, ack, marker, binaryReady, unitReady>>
Retire == /\ phase = "stopped" /\ phase' = "retired" /\ backup' = TRUE
          /\ history' = "new"
          /\ receipts' = IF CoupledRetirement THEN "new" ELSE receipts
          /\ UNCHANGED <<oldRunning, newRunning, ack, marker, binaryReady, unitReady>>
InstallBinary == /\ phase = "retired" /\ phase' = "installed" /\ binaryReady' = TRUE
                 /\ UNCHANGED <<oldRunning, newRunning, history, receipts, backup, ack, marker, unitReady>>
ConfigureUnit == /\ phase = "installed" /\ phase' = "configured" /\ unitReady' = TRUE
                 /\ UNCHANGED <<oldRunning, newRunning, history, receipts, backup, ack, marker, binaryReady>>
Activate == /\ phase = "configured" /\ phase' = "activating"
            /\ UNCHANGED <<oldRunning, newRunning, history, receipts, backup, ack, marker, binaryReady, unitReady>>
Start == /\ phase = "activating" /\ phase' = "started" /\ newRunning' = TRUE
         /\ UNCHANGED <<oldRunning, history, receipts, backup, ack, marker, binaryReady, unitReady>>
Accept == /\ newRunning /\ ~ack /\ ack' = TRUE
          /\ UNCHANGED <<phase, oldRunning, newRunning, history, receipts, backup, marker, binaryReady, unitReady>>
Finish == /\ phase = "started" /\ phase' = "complete" /\ marker' = FALSE
          /\ UNCHANGED <<oldRunning, newRunning, history, receipts, backup, ack, binaryReady, unitReady>>
Rollback == /\ phase \in {"retired", "installed", "configured", "activating", "started"}
            /\ (~GuardRollback \/ phase \in {"retired", "installed", "configured"})
            /\ phase' = "restored" /\ history' = "old" /\ receipts' = "old"
            /\ oldRunning' = TRUE /\ newRunning' = FALSE /\ marker' = FALSE
            /\ binaryReady' = FALSE /\ unitReady' = FALSE
            /\ UNCHANGED <<backup, ack>>
Crash == /\ phase \in {"prepared", "stopped", "retired", "installed", "configured", "activating", "started"}
         /\ phase' = "interrupted"
         /\ UNCHANGED <<oldRunning, newRunning, history, receipts, backup, ack, marker, binaryReady, unitReady>>
Next == Prepare \/ Stop \/ Retire \/ InstallBinary \/ ConfigureUnit \/ Activate \/ Start \/ Accept \/ Finish \/ Rollback \/ Crash
Spec == Init /\ [][Next]_vars
NoLostAcknowledgements == ack => history = "new"
ReceiptsMatchHistory == history = receipts
RetirementStopsOwner == backup => ~oldRunning \/ phase = "restored"
InterruptedNeedsRecovery == phase = "interrupted" => marker
ServingUsesPreparedRelease == newRunning => binaryReady /\ unitReady
RestoredUsesOriginalUnit == phase = "restored" => ~unitReady
=============================================================================
