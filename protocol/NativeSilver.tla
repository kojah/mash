---------------------------- MODULE NativeSilver ---------------------------
EXTENDS Naturals

\* Validated native uploads, with revision guards and permanent tombstones.
\* Requests may arrive out of order and be retried after losing a response.
CONSTANT EnforceTombstones
Chats == {1, 2}
VARIABLES head, records, tombstones, accepted, request, phase
vars == <<head, records, tombstones, accepted, request, phase>>
EmptyRequest == [revision |-> 1, base |-> 0, snapshot |-> TRUE,
                 records |-> {}, forgets |-> {}, owner |-> TRUE]

Init ==
    /\ head = 0 /\ records = {} /\ tombstones = {} /\ accepted = {0}
    /\ request = EmptyRequest
    /\ phase = "idle"

Prepare ==
    /\ phase = "idle" /\ phase' = "transaction"
    /\ request' \in [revision : 1..3, base : 0..3, snapshot : BOOLEAN,
                     records : SUBSET Chats, forgets : SUBSET Chats,
                     owner : BOOLEAN]
    /\ UNCHANGED <<head, records, tombstones, accepted>>

Admissible == request.owner /\
    IF request.snapshot
    THEN request.revision >= head /\ request.forgets = {}
    ELSE request.base = head /\ request.revision > head

Commit ==
    /\ phase = "transaction" /\ Admissible
    /\ IF request.snapshot /\ request.revision = head
       THEN UNCHANGED <<head, records, tombstones, accepted>>
       ELSE /\ head' = request.revision
            /\ tombstones' = tombstones \cup request.forgets
            /\ LET supplied == IF request.snapshot THEN request.records
                               ELSE records \cup request.records
               IN records' = IF EnforceTombstones
                             THEN supplied \ tombstones'
                             ELSE supplied \ request.forgets
            /\ accepted' = accepted \cup {request.revision}
    /\ phase' = "idle"
    /\ request' = EmptyRequest

\* Invalid upload or crash before commit leaves every durable field unchanged.
Abort ==
    /\ phase = "transaction" /\ phase' = "idle"
    /\ request' = EmptyRequest
    /\ UNCHANGED <<head, records, tombstones, accepted>>

Next == Prepare \/ Commit \/ Abort
Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ head \in 0..3 /\ records \subseteq Chats /\ tombstones \subseteq Chats
    /\ accepted \subseteq 0..3 /\ phase \in {"idle", "transaction"}
    /\ request \in [revision : 1..3, base : 0..3, snapshot : BOOLEAN,
                    records : SUBSET Chats, forgets : SUBSET Chats, owner : BOOLEAN]
NoResurrection == records \cap tombstones = {}
HeadNeverRegresses == \A revision \in accepted : revision <= head
=============================================================================
