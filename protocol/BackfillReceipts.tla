------------------------ MODULE BackfillReceipts ------------------------
\* Local source identity and server receipts occupy different revision domains.
\* A receipt can arrive out of order or be lost before its file is replaced.
EXTENDS Naturals, FiniteSets
CONSTANTS Separate, Monotonic
VARIABLES committed, received, saved, source, server, diskSource, diskServer
vars == <<committed, received, saved, source, server, diskSource, diskServer>>
Batches == 1..3
Max(S) == CHOOSE n \in S : \A m \in S : n >= m
Init ==
    /\ committed = {} /\ received = {} /\ saved = {}
    /\ source = 10 /\ diskSource = 10
    /\ server = IF Separate THEN 0 ELSE 10
    /\ diskServer = server
Commit ==
    /\ Cardinality(committed) < 3
    /\ committed' = committed \cup {Cardinality(committed) + 1}
    /\ UNCHANGED <<received, saved, source, server, diskSource, diskServer>>
Receive(b) ==
    /\ b \in committed \ received
    /\ received' = received \cup {b}
    /\ server' = IF Monotonic THEN Max({server, b}) ELSE b
    /\ source' = IF Separate THEN source ELSE b
    /\ UNCHANGED <<committed, saved, diskSource, diskServer>>
\* Atomic rename of a receipt file, separate from remote commit and response.
Save ==
    /\ saved' = received /\ diskSource' = source /\ diskServer' = server
    /\ UNCHANGED <<committed, received, source, server>>
\* Restart matches the frozen source revision before reusing durable receipts.
Restart ==
    /\ received' = IF diskSource = 10 THEN saved ELSE {}
    /\ server' = IF diskSource = 10 THEN diskServer ELSE IF Separate THEN 0 ELSE 10
    /\ source' = 10
    /\ UNCHANGED <<committed, saved, diskSource, diskServer>>
Next == Commit \/ (\E b \in Batches : Receive(b)) \/ Save \/ Restart
TypeOK ==
    /\ committed \subseteq Batches /\ received \subseteq committed
    /\ saved \subseteq committed
    /\ source \in Batches \cup {10} /\ diskSource \in Batches \cup {10}
    /\ server \in 0..10 /\ diskServer \in 0..10
SourceIdentityStable == source = 10 /\ diskSource = 10
ServerProgressIsReceipt == server = Max(received \cup {0})
DurableProgressIsReceipt == diskServer = Max(saved \cup {0})
=============================================================================
