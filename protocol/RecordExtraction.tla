-------------------------- MODULE RecordExtraction --------------------------
EXTENDS Integers, FiniteSets
CONSTANTS AtomicCheckpoint, SourceGuard
VARIABLES source, prepared, records, receipts, checkpoint, committedAt
vars == <<source, prepared, records, receipts, checkpoint, committedAt>>
Ops == {"one", "two"}
Init == /\ source = 0
        /\ prepared = [op \in Ops |-> -1]
        /\ records = {}
        /\ receipts = {}
        /\ checkpoint = {}
        /\ committedAt = [op \in Ops |-> -1]
Read(op) == /\ prepared[op] = -1
            /\ prepared' = [prepared EXCEPT ![op] = source]
            /\ UNCHANGED <<source, records, receipts, checkpoint, committedAt>>
Refresh == /\ source = 0 /\ source' = 1
           /\ UNCHANGED <<prepared, records, receipts, checkpoint, committedAt>>
Commit(op) == /\ prepared[op] >= 0 /\ op \notin receipts
              /\ (~SourceGuard \/ prepared[op] = source)
              /\ records' = records \cup {op}
              /\ receipts' = receipts \cup {op}
              /\ checkpoint' = {op}
              /\ committedAt' = [committedAt EXCEPT ![op] = source]
              /\ UNCHANGED <<source, prepared>>
\* Deliberate defect: a checkpoint written before the record transaction.
EarlyCheckpoint(op) == /\ ~AtomicCheckpoint /\ prepared[op] >= 0
                       /\ op \notin receipts
                       /\ checkpoint' = {op}
                       /\ UNCHANGED <<source, prepared, records, receipts, committedAt>>
\* Aborting SQL before commit leaves all durable values unchanged. Repeating a
\* committed request returns its receipt, also without changing durable state.
Next == Refresh \/ (\E op \in Ops: Read(op) \/ Commit(op) \/ EarlyCheckpoint(op))
Spec == Init /\ [][Next]_vars
TypeOK == /\ source \in 0..1 /\ prepared \in [Ops -> {-1, 0, 1}]
          /\ records \subseteq Ops /\ receipts \subseteq Ops
          /\ checkpoint \subseteq Ops /\ Cardinality(checkpoint) <= 1
          /\ committedAt \in [Ops -> {-1, 0, 1}]
CheckpointHasRecords == checkpoint \subseteq records
RecordsHaveReceipts == records = receipts
ReviewedVersionCommitted == \A op \in records: prepared[op] = committedAt[op]
=============================================================================
