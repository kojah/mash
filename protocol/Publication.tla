---------------------------- MODULE Publication ----------------------------
EXTENDS Naturals, FiniteSets

\* One local instance and publication target. Revision numbers here are the
\* transport revision, NOT the content-derived shared silver revision.
CONSTANTS MaxRevision, AckLatest
VARIABLES local, remote, ack, floor, sent, phase, journal, stable
vars == <<local, remote, ack, floor, sent, phase, journal, stable>>

Init ==
    /\ local = 1 /\ remote = 0 /\ ack = 0 /\ floor = 0
    /\ sent = 0 /\ phase = "idle" /\ journal = {1} /\ stable = FALSE

Edit ==
    /\ ~stable /\ local < MaxRevision
    /\ local' = local + 1
    /\ journal' = journal \cup {local + 1}
    /\ UNCHANGED <<remote, ack, floor, sent, phase, stable>>

Capture ==
    /\ phase = "idle" /\ ack < local
    /\ sent' = local /\ phase' = "upload"
    /\ UNCHANGED <<local, remote, ack, floor, journal, stable>>

\* A complete validated snapshot commits atomically; a duplicate is harmless.
Commit ==
    /\ phase = "upload" /\ sent >= remote
    /\ remote' = sent /\ phase' = "response"
    /\ UNCHANGED <<local, ack, floor, sent, journal, stable>>

\* Crash/lost response: the server commit survives, local ack does not advance.
LoseResponse ==
    /\ ~stable /\ phase # "idle"
    /\ phase' = "idle"
    /\ UNCHANGED <<local, remote, ack, floor, sent, journal, stable>>

Acknowledge ==
    /\ phase = "response"
    /\ LET revision == IF AckLatest THEN local ELSE sent
           nextAck == IF ack > revision THEN ack ELSE revision
       IN /\ ack' = nextAck
          /\ floor' = nextAck
          /\ journal' = {r \in journal : r > revision}
    /\ phase' = "idle"
    /\ UNCHANGED <<local, remote, sent, stable>>

\* Abstract successful double freshness check in already_published. Artifact
\* and identity checks are preconditions, not modeled as a distributed lock.
RecoverAck ==
    /\ phase = "idle" /\ remote = local /\ ack < local
    /\ ack' = remote /\ floor' = remote /\ journal' = {}
    /\ UNCHANGED <<local, remote, sent, phase, stable>>

Stabilize ==
    /\ ~stable /\ stable' = TRUE
    /\ UNCHANGED <<local, remote, ack, floor, sent, phase, journal>>

Next == Edit \/ Capture \/ Commit \/ LoseResponse \/ Acknowledge
        \/ RecoverAck \/ Stabilize
Spec == Init /\ [][Next]_vars
        /\ WF_vars(Capture) /\ WF_vars(Commit) /\ WF_vars(Acknowledge)

TypeOK ==
    /\ local \in 1..MaxRevision /\ remote \in 0..MaxRevision
    /\ ack \in 0..MaxRevision /\ floor \in 0..MaxRevision
    /\ sent \in 0..MaxRevision /\ journal \subseteq 1..MaxRevision
    /\ phase \in {"idle", "upload", "response"} /\ stable \in BOOLEAN
AcknowledgedIsCommitted == ack <= remote /\ remote <= local
JournalRetainsPending == {r \in 1..local : r > ack} \subseteq journal
FloorIsAcknowledged == floor = ack
EventuallyPublished == stable ~> (ack = local)
=============================================================================
