---------------------------- MODULE SyncTransfer ----------------------------
EXTENDS Naturals, FiniteSets

\* Implemented publication transfer over the authenticated /v1/sync WebSocket.
\* The model abstracts protobuf/zstd parsing, fsync and Qdrant certification; see
\* protocol/README.md for the source mapping and recorded atomics gaps.
\* One immutable batch, two bounded pieces; hashes abstract exact identity.
CONSTANTS Pieces, EarlyCheckpoint, SplitReceipt, EarlyAck,
          ExpireCommitted, AcceptChangedRetry, SplitClose
VARIABLES received, durable, checkpoint, reported, queued, working,
          committed, receipt, certified, ack, connected, closed, stable
vars == <<received, durable, checkpoint, reported, queued, working,
          committed, receipt, certified, ack, connected, closed, stable>>
Indices == 1..Pieces
Empty == [i \in Indices |-> 0]
\* 0 = missing, 1 = original bytes, 2 = conflicting retry bytes.
Prefix(data) == CHOOSE n \in 0..Pieces:
    /\ \A i \in 1..n: data[i] = 1
    /\ n = Pieces \/ data[n + 1] # 1

Init ==
    /\ received = Empty /\ durable = Empty
    /\ checkpoint = 0 /\ reported = 0
    /\ queued = FALSE /\ working = FALSE
    /\ committed = FALSE /\ receipt = FALSE /\ certified = FALSE
    /\ ack = FALSE /\ connected = TRUE /\ closed = FALSE /\ stable = FALSE

Receive ==
    /\ connected /\ ~closed /\ ~queued
    /\ \E i \in Indices:
        /\ received[i] = 0
        /\ received' = [received EXCEPT ![i] = 1]
    /\ UNCHANGED <<durable, checkpoint, reported, queued, working,
                   committed, receipt, certified, ack, connected, closed, stable>>

\* A correct receiver rejects this input without changing state (stuttering).
ChangedRetry ==
    /\ AcceptChangedRetry /\ connected /\ ~closed /\ ~queued
    /\ \E i \in Indices:
        /\ received[i] = 1
        /\ received' = [received EXCEPT ![i] = 2]
    /\ UNCHANGED <<durable, checkpoint, reported, queued, working,
                   committed, receipt, certified, ack, connected, closed, stable>>

\* Completion of data/file/directory persistence, separate from metadata.
Persist ==
    /\ ~closed /\ ~queued
    /\ \E i \in Indices:
        /\ received[i] # 0 /\ received[i] # durable[i]
        /\ durable' = [durable EXCEPT ![i] = received[i]]
    /\ UNCHANGED <<received, checkpoint, reported, queued, working,
                   committed, receipt, certified, ack, connected, closed, stable>>

SaveCheckpoint ==
    /\ ~closed /\ ~queued
    /\ LET next == Prefix(IF EarlyCheckpoint THEN received ELSE durable)
       IN /\ next > checkpoint /\ checkpoint' = next
    /\ UNCHANGED <<received, durable, reported, queued, working,
                   committed, receipt, certified, ack, connected, closed, stable>>

ReportCheckpoint ==
    /\ connected /\ ~closed /\ checkpoint > reported
    /\ reported' = checkpoint
    /\ UNCHANGED <<received, durable, checkpoint, queued, working,
                   committed, receipt, certified, ack, connected, closed, stable>>

\* FinishUpload validation and durable queue insertion; no silver write yet.
Queue ==
    /\ connected /\ ~closed /\ ~queued
    /\ checkpoint = Pieces /\ Prefix(durable) = Pieces
    /\ queued' = TRUE
    /\ UNCHANGED <<received, durable, checkpoint, reported, working,
                   committed, receipt, certified, ack, connected, closed, stable>>

Start ==
    /\ queued /\ ~closed /\ ~committed /\ ~working
    /\ working' = TRUE
    /\ UNCHANGED <<received, durable, checkpoint, reported, queued,
                   committed, receipt, certified, ack, connected, closed, stable>>

\* Receipt and silver share a SQLite commit. Socket ownership is irrelevant.
Commit ==
    /\ working /\ ~committed /\ ~closed
    /\ committed' = TRUE /\ receipt' = ~SplitReceipt /\ working' = FALSE
    /\ UNCHANGED <<received, durable, checkpoint, reported, queued,
                   certified, ack, connected, closed, stable>>

\* Deliberately separate durable write, enabled only in the broken model.
LateReceipt ==
    /\ committed /\ ~receipt /\ receipt' = TRUE
    /\ UNCHANGED <<received, durable, checkpoint, reported, queued, working,
                   committed, certified, ack, connected, closed, stable>>

\* Abstract successful views repair/certification; details are in SharedViews.
Certify ==
    /\ committed /\ ~closed /\ ~certified
    /\ certified' = TRUE
    /\ UNCHANGED <<received, durable, checkpoint, reported, queued, working,
                   committed, receipt, ack, connected, closed, stable>>

Acknowledge ==
    /\ connected /\ receipt /\ ~ack /\ ~closed
    /\ (certified \/ EarlyAck)
    /\ ack' = TRUE
    /\ UNCHANGED <<received, durable, checkpoint, reported, queued, working,
                   committed, receipt, certified, connected, closed, stable>>

Disconnect ==
    /\ ~stable /\ connected /\ connected' = FALSE
    /\ UNCHANGED <<received, durable, checkpoint, reported, queued, working,
                   committed, receipt, certified, ack, closed, stable>>
Reconnect ==
    /\ ~connected /\ connected' = TRUE
    /\ UNCHANGED <<received, durable, checkpoint, reported, queued, working,
                   committed, receipt, certified, ack, closed, stable>>

\* Retain only the durable prefix on restart. Queued work and commit evidence
\* survive; active workers do not. Restart resumes queued work or views repair.
Crash ==
    /\ ~stable /\ ~closed
    /\ durable' = [i \in Indices |-> IF i <= checkpoint THEN durable[i] ELSE 0]
    /\ received' = durable' /\ working' = FALSE /\ connected' = FALSE
    /\ UNCHANGED <<checkpoint, reported, queued, committed, receipt,
                   certified, ack, closed, stable>>

\* Explicit terminal cancellation/expiry ends the checkpoint retention promise.
Close ==
    /\ ~SplitClose
    /\ ~stable /\ ~closed
    /\ ((~committed /\ ~working) \/ ExpireCommitted)
    /\ closed' = TRUE /\ received' = Empty /\ durable' = Empty
    /\ checkpoint' = 0 /\ reported' = 0 /\ queued' = FALSE
    /\ working' = FALSE
    /\ UNCHANGED <<committed, receipt, certified, ack, connected, stable>>

\* Broken implementation: expiry releases retained bytes before the row is
\* marked closed. The gap between these two steps is the hazard this negative
\* configuration exposes; the atomic Close above is the contract.
CloseClear ==
    /\ SplitClose /\ ~stable /\ ~closed
    /\ ((~committed /\ ~working) \/ ExpireCommitted)
    /\ durable' = Empty /\ received' = Empty
    /\ UNCHANGED <<checkpoint, reported, queued, working, committed, receipt,
                   certified, ack, closed, connected, stable>>

CloseMark ==
    /\ SplitClose /\ ~stable /\ ~closed
    /\ ((~committed /\ ~working) \/ ExpireCommitted)
    /\ closed' = TRUE /\ checkpoint' = 0 /\ reported' = 0
    /\ queued' = FALSE /\ working' = FALSE
    /\ UNCHANGED <<received, durable, committed, receipt, certified, ack,
                   connected, stable>>

Stabilize ==
    /\ ~stable /\ stable' = TRUE
    /\ UNCHANGED <<received, durable, checkpoint, reported, queued, working,
                   committed, receipt, certified, ack, connected, closed>>

Next == Receive \/ ChangedRetry \/ Persist \/ SaveCheckpoint \/ ReportCheckpoint
        \/ Queue \/ Start \/ Commit \/ LateReceipt \/ Certify \/ Acknowledge
        \/ Disconnect \/ Reconnect \/ Crash \/ Close
        \/ CloseClear \/ CloseMark \/ Stabilize
Spec == Init /\ [][Next]_vars
        /\ WF_vars(Receive) /\ WF_vars(Persist) /\ WF_vars(SaveCheckpoint)
        /\ WF_vars(Queue) /\ WF_vars(Start) /\ WF_vars(Commit)
        /\ WF_vars(Certify) /\ WF_vars(Acknowledge) /\ WF_vars(Reconnect)

TypeOK ==
    /\ received \in [Indices -> 0..2] /\ durable \in [Indices -> 0..2]
    /\ checkpoint \in 0..Pieces /\ reported \in 0..Pieces
    /\ <<queued, working, committed, receipt, certified, ack,
          connected, closed, stable>> \in [1..9 -> BOOLEAN]
TransferIdentity == \A i \in Indices: received[i] # 2 /\ durable[i] # 2
CheckpointIsDurable == reported <= checkpoint /\ checkpoint <= Prefix(durable)
CommitHasReceipt == committed = receipt
PublishedIsReady == ack => committed /\ receipt /\ certified
CommittedCannotExpire == closed => ~committed
WorkerHasQueuedData == working => queued /\ ~closed /\ Prefix(durable) = Pieces
EventuallyResolved == stable ~> (ack \/ closed)
=============================================================================
