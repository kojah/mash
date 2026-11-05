---------------------------- MODULE LocalWriter ----------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS KeepBlockingOwner, KeepAfterCommit
Processes == 1..2
VARIABLES phase, held, connected
vars == <<phase, held, connected>>
Init == /\ phase = [p \in Processes |-> "idle"]
        /\ held = {}
        /\ connected = Processes
Acquire(p) == /\ phase[p] = "idle"
              /\ held = {}
              /\ phase' = [phase EXCEPT ![p] = "admitted"]
              /\ held' = {p}
              /\ UNCHANGED connected
StartBlocking(p) == /\ phase[p] = "admitted"
                    /\ phase' = [phase EXCEPT ![p] = "blocking"]
                    /\ UNCHANGED <<held, connected>>
Cancel(p) == /\ p \in connected
             /\ phase[p] \in {"admitted", "blocking", "refresh"}
             /\ connected' = connected \ {p}
             /\ IF phase[p] = "blocking"
                   THEN /\ UNCHANGED phase
                        /\ held' = IF KeepBlockingOwner THEN held ELSE held \ {p}
                   ELSE /\ phase' = [phase EXCEPT ![p] = "done"]
                        /\ held' = held \ {p}
Commit(p) == /\ phase[p] = "blocking"
             /\ phase' = [phase EXCEPT ![p] = IF p \in connected THEN "refresh" ELSE "done"]
             /\ held' = IF p \in connected /\ KeepAfterCommit THEN held ELSE held \ {p}
             /\ UNCHANGED connected
Finish(p) == /\ phase[p] = "refresh"
             /\ phase' = [phase EXCEPT ![p] = "done"]
             /\ held' = held \ {p}
             /\ UNCHANGED connected
\* Process death releases the OS lock and terminates its local worker. This
\* deliberately says nothing about remote requests already sent to Qdrant.
Crash(p) == /\ phase[p] \in {"admitted", "blocking", "refresh"}
            /\ phase' = [phase EXCEPT ![p] = "done"]
            /\ held' = held \ {p}
            /\ connected' = connected \ {p}
Next == \E p \in Processes : Acquire(p) \/ StartBlocking(p) \/ Cancel(p) \/ Commit(p) \/ Finish(p) \/ Crash(p)
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase \in [Processes -> {"idle", "admitted", "blocking", "refresh", "done"}]
          /\ held \subseteq Processes
          /\ connected \subseteq Processes
WorkOwnsLock == \A p \in Processes : phase[p] \in {"admitted", "blocking", "refresh"} => p \in held
Exclusive == Cardinality({p \in Processes : phase[p] \in {"admitted", "blocking", "refresh"}}) <= 1
=============================================================================
