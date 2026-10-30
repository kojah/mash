-------------------------- MODULE MutationAdmission --------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC
CONSTANTS Capacity, QueueLimit, KeepWorkerPermit, FIFO
Requests == 1..4
VARIABLES phase, queue, owners, connected, overtook
vars == <<phase, queue, owners, connected, overtook>>

Init == /\ phase = [r \in Requests |-> "new"]
        /\ queue = <<>>
        /\ owners = {}
        /\ connected = Requests
        /\ overtook = FALSE

Arrive(r) == /\ phase[r] = "new"
             /\ IF queue = <<>> /\ Cardinality(owners) < Capacity
                   THEN /\ phase' = [phase EXCEPT ![r] = "receiving"]
                        /\ owners' = owners \cup {r}
                        /\ UNCHANGED queue
                   ELSE IF Len(queue) < QueueLimit
                        THEN /\ phase' = [phase EXCEPT ![r] = "waiting"]
                             /\ queue' = Append(queue, r)
                             /\ UNCHANGED owners
                        ELSE /\ phase' = [phase EXCEPT ![r] = "done"]
                             /\ UNCHANGED <<queue, owners>>
             /\ UNCHANGED <<connected, overtook>>

Remove(q, r) == SelectSeq(q, LAMBDA x: x # r)

Grant == /\ queue # <<>>
         /\ Cardinality(owners) < Capacity
         /\ \E r \in {queue[i] : i \in 1..Len(queue)}:
               /\ (~FIFO \/ r = Head(queue))
               /\ phase' = [phase EXCEPT ![r] = "receiving"]
               /\ queue' = Remove(queue, r)
               /\ owners' = owners \cup {r}
               /\ overtook' = (overtook \/ r # Head(queue))
         /\ UNCHANGED connected

\* Expiry or cancellation before work: remove the queue position or release
\* an assigned permit. Receiving includes a semaphore wake not yet polled.
CancelBeforeWork(r) == /\ phase[r] \in {"waiting", "receiving"}
                       /\ phase' = [phase EXCEPT ![r] = "done"]
                       /\ queue' = Remove(queue, r)
                       /\ owners' = owners \ {r}
                       /\ connected' = connected \ {r}
                       /\ UNCHANGED overtook

StartWork == /\ \E r \in Requests:
                  /\ phase[r] = "receiving"
                  /\ phase' = [phase EXCEPT ![r] = "working"]
             /\ UNCHANGED <<queue, owners, connected, overtook>>

Disconnect(r) == /\ phase[r] = "working"
                 /\ r \in connected
                 /\ connected' = connected \ {r}
                 /\ owners' = IF KeepWorkerPermit THEN owners ELSE owners \ {r}
                 /\ UNCHANGED <<phase, queue, overtook>>

Finish == /\ \E r \in Requests:
               /\ phase[r] = "working"
               /\ phase' = [phase EXCEPT ![r] = "done"]
               /\ owners' = owners \ {r}
          /\ UNCHANGED <<queue, connected, overtook>>

Next == (\E r \in Requests: Arrive(r) \/ CancelBeforeWork(r) \/ Disconnect(r))
        \/ Grant \/ StartWork \/ Finish
Spec == Init /\ [][Next]_vars /\ WF_vars(Grant) /\ WF_vars(StartWork) /\ WF_vars(Finish)
TypeOK == /\ phase \in [Requests -> {"new", "waiting", "receiving", "working", "done"}]
          /\ owners \subseteq Requests
          /\ connected \subseteq Requests
          /\ Len(queue) <= QueueLimit
          /\ \A i \in 1..Len(queue): queue[i] \in Requests /\ phase[queue[i]] = "waiting"
BoundedWorkers == Cardinality(owners) <= Capacity
WorkOwnsCapacity == \A r \in Requests: phase[r] = "working" => r \in owners
FairOrder == ~overtook
QueuedResolves == \A r \in Requests: phase[r] = "waiting" ~> phase[r] = "done"
=============================================================================
