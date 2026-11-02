------------------------------ MODULE Publication ------------------------------
\* Two invariants that concurrency broke in production, and the fixes:
\*
\*   OrderedRevisions        - two batches committed for one node must never
\*                             receive the same revision
\*   UniqueBackfillIdentity  - a backfill identity names one destination only
\*
\* Rebase = FALSE models reading the head while preparing (outside the commit
\* lock). With two lanes both read the same head and compute the same next
\* revision. Rebase = TRUE models reading the head inside the commit
\* transaction, which is what silver does.
\*
\* Scoped = FALSE models the literal "auto" identity shared by every
\* destination; the transfer catalog is service-wide, so two groups claim one
\* row. Scoped = TRUE qualifies the identity with group and node.
\*
\* Deliberately not modelled: SQLite busy timeouts, QoS/retry, and wire credit.
\* Those are implementation-resource properties, not protocol ordering.
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Batches,  \* batches in one publication
          Lanes,    \* operations a node may have in flight
          Rebase,   \* head is read inside the commit lock
          Scoped,   \* identity includes the destination
          Entries   \* backfill attempts, one model value per destination

VARIABLES head, base, rev, inflight, registered

vars == <<head, base, rev, inflight, registered>>

TypeOK ==
  /\ head \in 0..Cardinality(Batches)
  /\ base \in [Batches -> 0..Cardinality(Batches)]
  /\ rev \in [Batches -> 0..Cardinality(Batches)]
  /\ inflight \in SUBSET Batches
  /\ registered \in SUBSET Entries

Init ==
  /\ head = 0
  /\ base = [b \in Batches |-> 0]
  /\ rev = [b \in Batches |-> 0]
  /\ inflight = {}
  /\ registered = {}

\* Prepare one batch: snapshot the head the client will claim to base on.
Start(b) ==
  /\ b \in Batches
  /\ b \notin inflight
  /\ rev[b] = 0
  /\ Cardinality(inflight) < Lanes
  /\ base' = [base EXCEPT ![b] = head]
  /\ inflight' = inflight \cup {b}
  /\ UNCHANGED <<head, rev, registered>>

\* Commit one batch: assign the next revision, serialized by the node lock.
Commit(b) ==
  /\ b \in inflight
  /\ LET assigned == IF Rebase THEN head + 1 ELSE base[b] + 1
     IN /\ rev' = [rev EXCEPT ![b] = assigned]
        /\ head' = IF assigned > head THEN assigned ELSE head
        /\ inflight' = inflight \ {b}
        /\ UNCHANGED <<base, registered>>

\* Register a backfill identity in the service-wide catalog.
Register(e) ==
  /\ e \in Entries
  /\ e \notin registered
  /\ registered' = registered \cup {e}
  /\ UNCHANGED <<head, base, rev, inflight>>

\* The literal identity every automatic baseline uses. Scoped identities are
\* the destination itself; unscoped ones all collapse onto one string.
Key(e) == IF Scoped THEN e ELSE "auto"

Next ==
  \/ \E b \in Batches : Start(b) \/ Commit(b)
  \/ \E e \in Entries : Register(e)
  \/ UNCHANGED vars  \* stuttering, so a finished run is not a deadlock

\* One node, so committed revisions must be distinct and form 1..N.
OrderedRevisions ==
  \A a, b \in Batches :
     a # b /\ rev[a] # 0 /\ rev[b] # 0 => rev[a] # rev[b]

ContiguousRevisions ==
  \A b \in Batches : rev[b] # 0 => rev[b] \in 1..Cardinality(Batches)

UniqueBackfillIdentity ==
  \A a, b \in registered : a # b => Key(a) # Key(b)

====
