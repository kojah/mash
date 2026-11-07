--------------------------- MODULE RecallBackfill ---------------------------
EXTENDS Naturals, TLC
CONSTANT RequireRecordReceipt
VARIABLES chats, records, response, acknowledged, journal
vars == <<chats, records, response, acknowledged, journal>>
Init == /\ chats = FALSE /\ records = FALSE
        /\ response = FALSE /\ acknowledged = FALSE /\ journal = TRUE
ChatBaseline == /\ ~chats /\ chats' = TRUE
                /\ UNCHANGED <<records, response, acknowledged, journal>>
\* A separate shared transaction commits the record batch after chat coverage.
RecordCommit == /\ chats /\ ~records /\ records' = TRUE
                /\ UNCHANGED <<chats, response, acknowledged, journal>>
\* The durable synchronization receipt permits an identical retry after loss.
Receipt == /\ records /\ response' = TRUE
           /\ UNCHANGED <<chats, records, acknowledged, journal>>
LoseResponse == /\ response' = FALSE
                /\ UNCHANGED <<chats, records, acknowledged, journal>>
LocalAck == /\ chats /\ (~RequireRecordReceipt \/ response)
            /\ acknowledged' = TRUE /\ journal' = FALSE /\ UNCHANGED <<chats, records, response>>
Next == ChatBaseline \/ RecordCommit \/ Receipt \/ LoseResponse \/ LocalAck
Spec == Init /\ [][Next]_vars
TypeOK == /\ chats \in BOOLEAN /\ records \in BOOLEAN
          /\ response \in BOOLEAN /\ acknowledged \in BOOLEAN /\ journal \in BOOLEAN
AcknowledgedIncludesRecords == acknowledged => (chats /\ records)
PrunedIncludesRecords == ~journal => (acknowledged /\ records)
=============================================================================
