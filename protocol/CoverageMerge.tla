-------------------------- MODULE CoverageMerge --------------------------
\* Two certifications for overlapping chats can finish in either order.
\* The later capture still has an old marker for chat a while it builds b.
EXTENDS Naturals, FiniteSets
CONSTANTS ReplaceStale, PreserveLatestAtWrite
VARIABLES pending, writing, input, chats
vars == <<pending, writing, input, chats>>
Chats == {"a", "b"}
Expected == [a |-> 2, b |-> 1]
Captured(r) == IF r = 2 THEN [a |-> 2, b |-> 0]
                          ELSE [a |-> 1, b |-> 1]
Merge(preferred, other) ==
    [c \in Chats |->
        IF preferred[c] = Expected[c] THEN preferred[c]
        ELSE IF other[c] = Expected[c] /\ (ReplaceStale \/ preferred[c] = 0)
             THEN other[c] ELSE 0]
Init == /\ pending = {2, 3} /\ writing = {}
        /\ input = 1 /\ chats = [a |-> 1, b |-> 0]
\* Pre-write invalidation under the same views lock is a separate durable write.
\* Both uploads captured a=1 before either finished; missing-only insertion
\* keeps that stale a even if stored coverage now proves a=2.
Invalidate(r) ==
    /\ r \in pending \ writing
    /\ writing' = writing \cup {r}
    /\ chats' = IF PreserveLatestAtWrite THEN chats
                ELSE [a |-> 1, b |-> chats.b]
    /\ input' = IF PreserveLatestAtWrite THEN input ELSE 1
    /\ UNCHANGED pending
Certify(r) ==
    /\ r \in writing
    /\ pending' = pending \ {r}
    /\ writing' = writing \ {r}
    /\ chats' = IF input > r THEN Merge(chats, Captured(r))
                             ELSE Merge(Captured(r), chats)
    /\ input' = IF input > r THEN input ELSE r
Next == (\E r \in {2, 3} : Invalidate(r) \/ Certify(r)) \/ UNCHANGED vars
TypeOK == /\ pending \subseteq {2, 3} /\ input \in 1..3
          /\ writing \subseteq pending
          /\ chats \in [Chats -> 0..2]
CompletedHasCoverage == pending = {} => chats = Expected
=============================================================================
