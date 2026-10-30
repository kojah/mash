------------------------- MODULE ProgressiveBackfill -------------------------
EXTENDS Naturals, FiniteSets

\* PROPOSED protocol. Frozen baseline R=1 has two complete conversations.
\* Live revision 2 edits a; revision 3 deletes a. Every batch is independent.
\* Source revisions are application ordering, not timestamps or silver revisions.
CONSTANTS GuardVersions, KeepTombstones, EarlyFinish, SparseAck
Chats == {"a", "b"}
Batches == {"base-a", "base-b", "live-2", "live-3"}
Version == [b \in Batches |-> CASE b = "live-2" -> 2
                              [] b = "live-3" -> 3 [] OTHER -> 1]
Chat == [b \in Batches |-> IF b = "base-b" THEN "b" ELSE "a"]
Empty == [c \in Chats |-> 0]
Max(s) == CHOOSE n \in s: \A m \in s: m <= n
\* State values: 0 missing, 1 baseline body, 2 updated body, 3 tombstone.
VARIABLES local, silver, receipts, published, covered, docs, certified,
          pending, phase, complete, ack, journal, stable
vars == <<local, silver, receipts, published, covered, docs, certified,
          pending, phase, complete, ack, journal, stable>>

Init ==
    /\ local = 1 /\ silver = Empty /\ receipts = {} /\ published = {}
    /\ covered = {} /\ docs = Empty /\ certified = 0
    /\ pending = "none" /\ phase = "idle" /\ complete = FALSE
    /\ ack = 0 /\ journal = {1} /\ stable = FALSE

Edit ==
    /\ ~stable /\ local < 3 /\ local' = local + 1
    /\ journal' = journal \cup {local + 1}
    /\ UNCHANGED <<silver, receipts, published, covered, docs, certified,
                   pending, phase, complete, ack, stable>>

\* Completed, validated transfers may be scheduled in any order. Recovery can
\* select a previously committed batch whose publication reply was lost.
Prepare ==
    /\ phase = "idle"
    /\ \E b \in Batches \ published:
        /\ Version[b] <= local /\ pending' = b
    /\ phase' = "apply"
    /\ UNCHANGED <<local, silver, receipts, published, covered, docs,
                   certified, complete, ack, journal, stable>>

Replacement(old, incoming) ==
    IF old = 3 THEN IF KeepTombstones THEN old ELSE incoming
    ELSE IF GuardVersions THEN Max({old, incoming}) ELSE incoming

\* One SQLite commit: body/tombstone, ordering evidence, receipt and coverage.
\* A newer live record is valid evidence superseding its baseline member.
Commit ==
    /\ phase = "apply"
    /\ IF pending \in receipts
       THEN UNCHANGED <<silver, receipts, covered>>
       ELSE /\ silver' = [silver EXCEPT
                    ![Chat[pending]] = Replacement(@, Version[pending])]
            /\ receipts' = receipts \cup {pending}
            /\ covered' = covered \cup {Chat[pending]}
    /\ phase' = "repair"
    /\ UNCHANGED <<local, published, docs, certified, pending,
                   complete, ack, journal, stable>>

\* Separate durable Qdrant writes; deletion becomes a tombstone marker in this
\* abstraction (no searchable document). The certificate must match all silver.
WriteDocument ==
    /\ phase = "repair"
    /\ \E c \in Chats:
        /\ docs[c] # silver[c]
        /\ docs' = [docs EXCEPT ![c] = silver[c]]
    /\ UNCHANGED <<local, silver, receipts, published, covered, certified,
                   pending, phase, complete, ack, journal, stable>>
Certify ==
    /\ phase = "repair" /\ docs = silver
    /\ certified' = Cardinality(receipts) /\ phase' = "reply"
    /\ UNCHANGED <<local, silver, receipts, published, covered, docs,
                   pending, complete, ack, journal, stable>>
Current == certified = Cardinality(receipts)
Publish ==
    /\ phase = "reply" /\ Current
    /\ published' = published \cup {pending}
    /\ phase' = "idle" /\ pending' = "none"
    /\ UNCHANGED <<local, silver, receipts, covered, docs, certified,
                   complete, ack, journal, stable>>

\* Crash/lost reply preserves silver, receipts, coverage, point writes and
\* certificate, but loses the active worker. A replay repairs from silver.
Crash ==
    /\ ~stable /\ phase # "idle"
    /\ phase' = "idle" /\ pending' = "none"
    /\ UNCHANGED <<local, silver, receipts, published, covered, docs,
                   certified, complete, ack, journal, stable>>

FinishBackfill ==
    /\ ~complete /\ phase = "idle" /\ Current
    /\ (covered = Chats \/ EarlyFinish)
    /\ complete' = TRUE
    /\ UNCHANGED <<local, silver, receipts, published, covered, docs,
                   certified, pending, phase, ack, journal, stable>>

\* Baseline coverage and contiguous live coverage are distinct from the
\* highest per-conversation version. Retain all source changes across holes.
CoveredThrough(r) == complete /\
    \A b \in {"live-2", "live-3"}: Version[b] <= r => b \in published
Acknowledge ==
    /\ \E r \in (ack + 1)..local:
        /\ IF SparseAck
           THEN r = Max({0} \cup {Version[b] : b \in published})
           ELSE CoveredThrough(r)
        /\ ack' = r /\ journal' = {v \in journal: v > r}
    /\ UNCHANGED <<local, silver, receipts, published, covered, docs,
                   certified, pending, phase, complete, stable>>

Stabilize ==
    /\ ~stable /\ stable' = TRUE
    /\ UNCHANGED <<local, silver, receipts, published, covered, docs,
                   certified, pending, phase, complete, ack, journal>>
Next == Edit \/ Prepare \/ Commit \/ WriteDocument \/ Certify \/ Publish
        \/ Crash \/ FinishBackfill \/ Acknowledge \/ Stabilize
Spec == Init /\ [][Next]_vars
        /\ WF_vars(Prepare) /\ WF_vars(Commit) /\ WF_vars(WriteDocument)
        /\ WF_vars(Certify) /\ WF_vars(Publish)
        /\ WF_vars(FinishBackfill) /\ WF_vars(Acknowledge)

TypeOK ==
    /\ local \in 1..3 /\ silver \in [Chats -> 0..3]
    /\ receipts \subseteq Batches /\ published \subseteq Batches
    /\ covered \subseteq Chats /\ docs \in [Chats -> 0..3]
    /\ certified \in 0..4 /\ pending \in Batches \cup {"none"}
    /\ phase \in {"idle", "apply", "repair", "reply"}
    /\ complete \in BOOLEAN /\ ack \in 0..3
    /\ journal \subseteq 1..3 /\ stable \in BOOLEAN
NewestCommittedWins == \A c \in Chats:
    silver[c] = Max({0} \cup {Version[b]: b \in {x \in receipts: Chat[x] = c}})
NoResurrection == "live-3" \in receipts => silver["a"] = 3
CoverageHasEvidence == covered = {Chat[b]: b \in receipts}
CompleteHasCoverage == complete => covered = Chats
NeverFalselyCurrent == Current => docs = silver
ReplyHasReceipt == published \subseteq receipts
AckHasCoverage == ack = 0 \/ CoveredThrough(ack)
JournalRetainsGaps == {r \in 1..local: ~CoveredThrough(r)} \subseteq journal
EventuallyCaughtUp == stable ~> (complete /\ ack = local /\ Current)
=============================================================================
