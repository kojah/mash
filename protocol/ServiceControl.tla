------------------------- MODULE ServiceControl -------------------------
EXTENDS Naturals, FiniteSets
CONSTANT ReceiptBeforeSend
Requests == 1..2
VARIABLES phase, receipts, sent, finished, lock
vars == <<phase, receipts, sent, finished, lock>>
Init == /\ phase = [r \in Requests |-> "idle"]
        /\ receipts = {}
        /\ sent = [r \in Requests |-> 0]
        /\ finished = {}
        /\ lock = 0
Acquire(r) == /\ phase[r] = "idle" /\ lock = 0 /\ r \notin receipts
              /\ lock' = r
              /\ phase' = [phase EXCEPT ![r] = "locked"]
              /\ UNCHANGED <<receipts, sent, finished>>
Receipt(r) == /\ phase[r] = "locked"
              /\ receipts' = receipts \cup {r}
              /\ phase' = [phase EXCEPT ![r] = "recorded"]
              /\ UNCHANGED <<lock, sent, finished>>
Send(r) == /\ phase[r] \in (IF ReceiptBeforeSend THEN {"recorded"} ELSE {"locked", "recorded"})
           /\ sent[r] < 2
           /\ sent' = [sent EXCEPT ![r] = @ + 1]
           /\ phase' = [phase EXCEPT ![r] = "sent"]
           /\ UNCHANGED <<receipts, finished, lock>>
Finish(r) == /\ phase[r] = "sent"
             /\ finished' = finished \cup {r}
             /\ phase' = [phase EXCEPT ![r] = "done"]
             /\ lock' = 0
             /\ UNCHANGED <<receipts, sent>>
Crash(r) == /\ phase[r] \in {"locked", "recorded", "sent"}
            /\ phase' = [phase EXCEPT ![r] = "idle"]
            /\ lock' = 0
            /\ UNCHANGED <<receipts, sent, finished>>
\* Repeated requests with receipts only observe an outcome. A missing result
\* is unknown, never permission to resend. Browser disconnect does not cancel
\* the detached operation and is a stuttering action in this model.
Observe(r) == /\ phase[r] = "idle" /\ r \in receipts
              /\ phase' = [phase EXCEPT ![r] = "done"]
              /\ UNCHANGED <<receipts, sent, finished, lock>>
Next == \E r \in Requests : Acquire(r) \/ Receipt(r) \/ Send(r) \/ Finish(r) \/ Crash(r) \/ Observe(r)
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase \in [Requests -> {"idle", "locked", "recorded", "sent", "done"}]
          /\ receipts \subseteq Requests /\ finished \subseteq Requests
          /\ sent \in [Requests -> 0..2] /\ lock \in 0..2
AtMostOnce == \A r \in Requests : sent[r] <= 1
FinishedWasSent == \A r \in finished : sent[r] = 1
=============================================================================
