----------------------------- MODULE FleetJobs -----------------------------
EXTENDS Naturals, FiniteSets
CONSTANT ReceiptBeforeLaunch
Requests == 1..2
VARIABLES reserved, receipts, owners, launched, childRunning, results, phase
vars == <<reserved, receipts, owners, launched, childRunning, results, phase>>
Init == /\ reserved = {} /\ receipts = {} /\ owners = {}
        /\ launched = [r \in Requests |-> 0]
        /\ childRunning = {} /\ results = {}
        /\ phase = [r \in Requests |-> "idle"]
Reserve(r) == /\ r \notin reserved
              /\ reserved' = reserved \cup {r}
              /\ owners' = owners \cup {r}
              /\ phase' = [phase EXCEPT ![r] = "reserved"]
              /\ UNCHANGED <<receipts, launched, childRunning, results>>
Record(r) == /\ r \in owners /\ phase[r] = "reserved"
             /\ receipts' = receipts \cup {r}
             /\ phase' = [phase EXCEPT ![r] = "recorded"]
             /\ UNCHANGED <<reserved, owners, launched, childRunning, results>>
Launch(r) == /\ r \in owners
             /\ phase[r] \in (IF ReceiptBeforeLaunch THEN {"recorded"} ELSE {"reserved", "recorded"})
             /\ phase' = [phase EXCEPT ![r] = "launched"]
             /\ launched' = [launched EXCEPT ![r] = @ + 1]
             /\ childRunning' = childRunning \cup {r}
             /\ UNCHANGED <<reserved, receipts, owners, results>>
ChildExit(r) == /\ r \in childRunning
                /\ childRunning' = childRunning \ {r}
                /\ UNCHANGED <<reserved, receipts, owners, launched, results, phase>>
Finish(r) == /\ r \in owners /\ launched[r] = 1
             /\ r \notin childRunning
             /\ results' = results \cup {r}
             /\ owners' = owners \ {r}
             /\ phase' = [phase EXCEPT ![r] = "done"]
             /\ UNCHANGED <<reserved, receipts, launched, childRunning>>
\* A crash or timeout loses the owner; the child can continue independently.
LoseOwner(r) == /\ r \in owners
                /\ owners' = owners \ {r}
                /\ phase' = [phase EXCEPT ![r] = "abandoned"]
                /\ UNCHANGED <<reserved, receipts, launched, childRunning, results>>
\* Duplicate requests and browser disconnects cannot change execution state.
Next == \E r \in Requests : Reserve(r) \/ Record(r) \/ Launch(r)
                           \/ ChildExit(r) \/ Finish(r) \/ LoseOwner(r)
Spec == Init /\ [][Next]_vars
TypeOK == /\ reserved \subseteq Requests /\ receipts \subseteq reserved
          /\ phase \in [Requests -> {"idle", "reserved", "recorded", "launched", "done", "abandoned"}]
          /\ owners \subseteq reserved /\ results \subseteq reserved
          /\ childRunning \subseteq reserved
          /\ launched \in [Requests -> 0..2]
AtMostOnce == \A r \in Requests : launched[r] <= 1
LaunchRecorded == \A r \in Requests : launched[r] > 0 => r \in receipts
FinishedWasLaunched == \A r \in results : launched[r] = 1
=============================================================================
