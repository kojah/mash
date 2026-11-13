------------------------------ MODULE LocalGold ------------------------------
\* Durable boundaries of direct local silver -> Qdrant replay. LocalWriter
\* separately checks retention of the process owner. This model assumes no
\* request remains in flight across Crash; see the explicit limitation in README.
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS ClearBeforeInvalidate, CertifyBeforeFinish, DenseWithoutVectors
Items == {"chat-a", "chat-b"}
\* `certified` claims the word index: every document's text. `dense` claims
\* that every document also has its vector. A write whose embedding failed
\* stores the text alone, so a completed pass can certify words without dense.
VARIABLES silver, settings, phase, captured, recipe, documents, vectors,
          remaining, certified, dense, stamp, goldRecipe
vars == <<silver, settings, phase, captured, recipe, documents, vectors,
          remaining, certified, dense, stamp, goldRecipe>>

Projection(revision, build) ==
    [item \in Items |-> IF revision = 2 /\ item = "chat-a"
                       THEN 0 ELSE revision * 2 + build]

Init == /\ silver = 1
        /\ settings = 0
        /\ phase = "idle"
        /\ captured = 1
        /\ recipe = 0
        /\ documents = Projection(1, 0)
        /\ vectors = [item \in Items |-> TRUE]
        /\ remaining = {}
        /\ certified = TRUE
        /\ dense = TRUE
        /\ stamp = 1
        /\ goldRecipe = 0

\* Canonical transactions commit before a refresh acquires its snapshot.
Commit == /\ phase = "idle"
          /\ silver = 1
          /\ silver' = 2
          /\ UNCHANGED <<settings, phase, captured, recipe, documents, vectors,
                         remaining, certified, dense, stamp, goldRecipe>>
Activate == /\ phase = "idle"
            /\ settings = 0
            /\ settings' = 1
            /\ UNCHANGED <<silver, phase, captured, recipe, documents, vectors,
                           remaining, certified, dense, stamp, goldRecipe>>
Capture == /\ phase = "idle"
           /\ captured' = silver
           /\ recipe' = settings
           /\ remaining' = Items
           /\ phase' = IF ClearBeforeInvalidate THEN "clear-first" ELSE "invalidate"
           /\ UNCHANGED <<silver, settings, documents, vectors, certified, dense,
                          stamp, goldRecipe>>

ClearFirst(item) == /\ phase = "clear-first"
              /\ documents' = [documents EXCEPT ![item] = 0]
              /\ vectors' = [vectors EXCEPT ![item] = FALSE]
              /\ phase' = "invalidate"
              /\ UNCHANGED <<silver, settings, captured, recipe, remaining,
                             certified, dense, stamp, goldRecipe>>
Invalidate == /\ phase = "invalidate"
              /\ certified' = FALSE
              /\ dense' = FALSE
              /\ phase' = "writing"
              /\ UNCHANGED <<silver, settings, captured, recipe, documents, vectors,
                             remaining, stamp, goldRecipe>>
Delete(item) == /\ phase = "writing"
               /\ item \in remaining
               /\ documents' = [documents EXCEPT ![item] = 0]
               /\ vectors' = [vectors EXCEPT ![item] = FALSE]
               /\ UNCHANGED <<silver, settings, phase, captured, recipe, remaining,
                              certified, dense, stamp, goldRecipe>>
\* `embedded` is FALSE when inference failed for this document's batch.
Write(item, embedded) ==
               /\ phase = "writing"
               /\ item \in remaining
               /\ documents' = [documents EXCEPT ![item] = Projection(captured, recipe)[item]]
               /\ vectors' = [vectors EXCEPT ![item] = embedded]
               /\ remaining' = remaining \ {item}
               /\ UNCHANGED <<silver, settings, phase, captured, recipe,
                              certified, dense, stamp, goldRecipe>>
\* A removed projection needs no vector.
Embedded == \A item \in Items :
                Projection(captured, recipe)[item] = 0 \/ vectors[item]
Certify == /\ phase = "writing"
           /\ (remaining = {} \/ CertifyBeforeFinish)
           /\ silver = captured
           /\ settings = recipe
           /\ certified' = TRUE
           /\ dense' = (Embedded \/ DenseWithoutVectors)
           /\ stamp' = captured
           /\ goldRecipe' = recipe
           /\ phase' = "idle"
           /\ UNCHANGED <<silver, settings, captured, recipe, documents, vectors,
                          remaining>>
\* An interruption preserves every completed durable write. Retry captures the
\* committed silver again and invalidates before reconstructing all points.
Crash == /\ phase # "idle"
         /\ phase' = "idle"
         /\ UNCHANGED <<silver, settings, captured, recipe, documents, vectors,
                        remaining, certified, dense, stamp, goldRecipe>>
Next == Commit \/ Activate \/ Capture \/ Invalidate
        \/ (\E item \in Items : ClearFirst(item) \/ Delete(item)
                \/ (\E embedded \in BOOLEAN : Write(item, embedded)))
        \/ Certify \/ Crash
Spec == Init /\ [][Next]_vars
TypeOK == /\ silver \in 1..2 /\ settings \in 0..1
          /\ captured \in 1..2 /\ recipe \in 0..1
          /\ stamp \in 1..2 /\ goldRecipe \in 0..1
          /\ documents \in [Items -> 0..5]
          /\ vectors \in [Items -> BOOLEAN]
          /\ remaining \subseteq Items
          /\ certified \in BOOLEAN /\ dense \in BOOLEAN
          /\ phase \in {"idle", "clear-first", "invalidate", "writing"}
CurrentMatchesSilver ==
    certified /\ stamp = silver /\ goldRecipe = settings
    => documents = Projection(silver, settings)
\* Meaning search additionally needs every remaining document's vector.
DenseHasVectors ==
    dense /\ stamp = silver /\ goldRecipe = settings
    => /\ certified
       /\ \A item \in Items : documents[item] = 0 \/ vectors[item]
=============================================================================
