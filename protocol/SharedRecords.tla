--------------------------- MODULE SharedRecords ---------------------------
EXTENDS Naturals, TLC
CONSTANT KeepPriorHeaders
VARIABLES publication, revision, text, erased
vars == <<publication, revision, text, erased>>

Init == /\ publication = 0 /\ revision = 0 /\ text = FALSE /\ erased = FALSE

\* Each action is one admitted shared SQLite transaction. Revision three is
\* the content-free forget header. A newer envelope can contain an old frozen
\* record: envelope ordering alone is insufficient to protect its state.
Apply(envelope, incoming, snapshot) ==
    LET prior == IF snapshot /\ ~KeepPriorHeaders THEN 0 ELSE revision IN
    /\ envelope > publication
    /\ incoming >= prior
    /\ (prior = 3 => incoming = 3)
    /\ publication' = envelope
    /\ revision' = incoming
    /\ text' = (incoming < 3)
    /\ erased' = (erased \/ incoming = 3)

\* An interrupted transaction, failed validation, or lost response followed
\* by an identical retry leaves the committed record state unchanged.
AbortOrRetry == UNCHANGED vars
Next == (\E envelope \in 1..4, incoming \in 1..3, snapshot \in BOOLEAN:
            Apply(envelope, incoming, snapshot)) \/ AbortOrRetry
Spec == Init /\ [][Next]_vars
TypeOK == /\ publication \in 0..4 /\ revision \in 0..3
          /\ text \in BOOLEAN /\ erased \in BOOLEAN
ForgottenStaysErased == erased => ~text
=============================================================================
