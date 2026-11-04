-------------------------- MODULE CoverageMerge --------------------------
\* Two certifications for overlapping chats can finish in either order.
\* The later capture still has an old marker for chat a while it builds b.
EXTENDS Naturals, FiniteSets
CONSTANT ReplaceStale
VARIABLES pending, input, chats
vars == <<pending, input, chats>>
Chats == {"a", "b"}
Expected == [a |-> 2, b |-> 1]
Captured(r) == IF r = 2 THEN [a |-> 2, b |-> 0]
                          ELSE [a |-> 1, b |-> 1]
Merge(preferred, other) ==
    [c \in Chats |->
        IF preferred[c] = Expected[c] THEN preferred[c]
        ELSE IF other[c] = Expected[c] /\ (ReplaceStale \/ preferred[c] = 0)
             THEN other[c] ELSE 0]
Init == /\ pending = {2, 3} /\ input = 1 /\ chats = [a |-> 1, b |-> 0]
Certify(r) ==
    /\ r \in pending
    /\ pending' = pending \ {r}
    /\ chats' = IF input > r THEN Merge(chats, Captured(r))
                             ELSE Merge(Captured(r), chats)
    /\ input' = IF input > r THEN input ELSE r
Next == (\E r \in {2, 3} : Certify(r)) \/ UNCHANGED vars
TypeOK == /\ pending \subseteq {2, 3} /\ input \in 1..3
          /\ chats \in [Chats -> 0..2]
CompletedHasCoverage == pending = {} => chats = Expected
=============================================================================
