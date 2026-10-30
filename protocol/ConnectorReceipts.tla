------------------------- MODULE ConnectorReceipts -------------------------
EXTENDS Naturals, FiniteSets

\* One source, two batch IDs, two possible payloads, one canonical object.
\* Each request fingerprints its expected revision AND payload.
CONSTANT CheckFingerprint
Ids == {1, 2}
Payloads == {1, 2}
Empty == [expected |-> 0, payload |-> 0, revision |-> 0]
VARIABLES revision, object, receipts, applied, request, phase, response,
          replyMatches, stable
vars == <<revision, object, receipts, applied, request, phase, response,
          replyMatches, stable>>

Init ==
    /\ revision = 0 /\ object = 0 /\ receipts = [id \in Ids |-> Empty]
    /\ applied = {} /\ request = [id |-> 1, expected |-> 0, payload |-> 1]
    /\ phase = "idle" /\ response = 0 /\ replyMatches = TRUE
    /\ stable = FALSE

Submit ==
    /\ phase = "idle"
    /\ \E id \in Ids, expected \in 0..2, payload \in Payloads :
        request' = [id |-> id, expected |-> expected, payload |-> payload]
    /\ phase' = "transaction"
    /\ UNCHANGED <<revision, object, receipts, applied, response,
                   replyMatches, stable>>

SameBatch == receipts[request.id].expected = request.expected
             /\ receipts[request.id].payload = request.payload

\* Receipt lookup precedes the expected_revision guard: a retry of an old
\* committed batch returns the old receipt even after subsequent commits.
Duplicate ==
    /\ phase = "transaction" /\ receipts[request.id] # Empty
    /\ (SameBatch \/ ~CheckFingerprint)
    /\ response' = receipts[request.id].revision
    /\ replyMatches' = SameBatch /\ phase' = "response"
    /\ UNCHANGED <<revision, object, receipts, applied, request, stable>>

Commit ==
    /\ phase = "transaction" /\ receipts[request.id] = Empty
    /\ request.expected = revision
    /\ revision' = revision + 1 /\ object' = request.payload
    /\ receipts' = [receipts EXCEPT ![request.id] =
        [expected |-> request.expected, payload |-> request.payload,
         revision |-> revision + 1]]
    /\ applied' = applied \cup {request.id}
    /\ response' = revision + 1 /\ replyMatches' = TRUE
    /\ phase' = "response"
    /\ UNCHANGED <<request, stable>>

Reject ==
    /\ phase = "transaction"
    /\ IF receipts[request.id] # Empty
       THEN CheckFingerprint /\ ~SameBatch
       ELSE request.expected # revision
    /\ phase' = "idle"
    /\ UNCHANGED <<revision, object, receipts, applied, request, response,
                   replyMatches, stable>>

\* Failure before commit rolls back; after commit it loses only the response.
Crash ==
    /\ ~stable /\ phase # "idle" /\ phase' = "idle"
    /\ UNCHANGED <<revision, object, receipts, applied, request, response,
                   replyMatches, stable>>

Deliver ==
    /\ phase = "response" /\ phase' = "idle"
    /\ UNCHANGED <<revision, object, receipts, applied, request, response,
                   replyMatches, stable>>

Stabilize ==
    /\ ~stable /\ stable' = TRUE
    /\ UNCHANGED <<revision, object, receipts, applied, request, phase,
                   response, replyMatches>>

Resolve == Duplicate \/ Commit \/ Reject
Next == Submit \/ Resolve \/ Crash \/ Deliver \/ Stabilize
Spec == Init /\ [][Next]_vars /\ WF_vars(Resolve) /\ WF_vars(Deliver)

TypeOK ==
    /\ revision \in 0..2 /\ object \in 0..2 /\ applied \subseteq Ids
    /\ receipts \in [Ids -> [expected : 0..2, payload : 0..2, revision : 0..2]]
    /\ request \in [id : Ids, expected : 0..2, payload : Payloads]
    /\ phase \in {"idle", "transaction", "response"}
    /\ response \in 0..2 /\ replyMatches \in BOOLEAN /\ stable \in BOOLEAN
OneCommitPerBatch == revision = Cardinality(applied)
ReceiptAtomicity == applied = {id \in Ids : receipts[id] # Empty}
ReceiptOrder == {receipts[id].revision : id \in applied} = 1..revision
ObjectMatchesLastCommit == revision > 0 =>
    \E id \in applied : receipts[id].revision = revision
                       /\ receipts[id].payload = object
ReplyMatchesRequest == replyMatches
ReplyIsDurable == phase = "response" => response <= revision /\ response > 0
EventuallyResolves == (stable /\ phase # "idle") ~> (phase = "idle")
=============================================================================
