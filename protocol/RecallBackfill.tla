--------------------------- MODULE RecallBackfill ---------------------------
EXTENDS Naturals, TLC
CONSTANT RequireRecordReceipt
VARIABLES chats, records, response, acknowledged
vars == <<chats, records, response, acknowledged>>
Init == /\ chats = FALSE /\ records = FALSE
        /\ response = FALSE /\ acknowledged = FALSE
ChatBaseline == /\ ~chats /\ chats' = TRUE
                /\ UNCHANGED <<records, response, acknowledged>>
\* A separate shared transaction commits the record batch after chat coverage.
RecordCommit == /\ chats /\ ~records /\ records' = TRUE
                /\ UNCHANGED <<chats, response, acknowledged>>
\* The durable synchronization receipt permits an identical retry after loss.
Receipt == /\ records /\ response' = TRUE
           /\ UNCHANGED <<chats, records, acknowledged>>
LoseResponse == /\ response' = FALSE
                /\ UNCHANGED <<chats, records, acknowledged>>
LocalAck == /\ chats /\ (~RequireRecordReceipt \/ response)
            /\ acknowledged' = TRUE /\ UNCHANGED <<chats, records, response>>
Next == ChatBaseline \/ RecordCommit \/ Receipt \/ LoseResponse \/ LocalAck
Spec == Init /\ [][Next]_vars
TypeOK == /\ chats \in BOOLEAN /\ records \in BOOLEAN
          /\ response \in BOOLEAN /\ acknowledged \in BOOLEAN
AcknowledgedIncludesRecords == acknowledged => (chats /\ records)
=============================================================================
