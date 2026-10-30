--------------------------- MODULE RuntimeUpgrade ---------------------------
EXTENDS Naturals, TLC
CONSTANT Guard
VARIABLES installed, phase, observed, admitted, completed, polls
vars == <<installed, phase, observed, admitted, completed, polls>>

\* One old watcher or MCP mutation queue; executable identity is abstracted by
\* a monotonic release number. Admission is AFTER the MCP gate is acquired.
Init == /\ installed = 1
        /\ phase = "idle"
        /\ observed = 1
        /\ admitted = 0
        /\ completed = 0
        /\ polls = 0

Upgrade == /\ installed = 1
           /\ installed' = 2
           /\ UNCHANGED <<phase, observed, admitted, completed, polls>>

Admit == /\ phase = "idle"
         /\ admitted < 2
         /\ observed' = installed
         /\ IF Guard /\ installed # 1
               THEN /\ phase' = "stopped"
                    /\ UNCHANGED admitted
               ELSE /\ phase' = "active"
                    /\ admitted' = admitted + 1
         /\ UNCHANGED <<installed, completed, polls>>

\* CLI's monitor may observe replacement while an operation is active. It
\* requests shutdown; the active operation is awaited, never aborted.
Poll == /\ installed = 2
        /\ phase \in {"idle", "active"}
        /\ phase' = IF phase = "active" THEN "stopping" ELSE "stopped"
        /\ polls' = polls + 1
        /\ UNCHANGED <<installed, observed, admitted, completed>>

Finish == /\ phase \in {"active", "stopping"}
          /\ completed' = completed + 1
          /\ phase' = IF phase = "stopping" THEN "stopped" ELSE "idle"
          /\ UNCHANGED <<installed, observed, admitted, polls>>

Next == Upgrade \/ Admit \/ Poll \/ Finish
Spec == Init /\ [][Next]_vars /\ WF_vars(Poll) /\ WF_vars(Finish)
TypeOK == /\ installed \in 1..2
          /\ phase \in {"idle", "active", "stopping", "stopped"}
          /\ observed \in 1..2
          /\ admitted \in 0..2
          /\ completed \in 0..2
          /\ polls \in 0..1
AdmissionChecked == phase \in {"active", "stopping"} => observed = 1
NoLostAdmittedWork == phase = "stopped" => completed = admitted
EventuallyStops == installed = 2 ~> phase = "stopped"
=============================================================================
