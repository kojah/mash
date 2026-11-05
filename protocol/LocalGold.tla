------------------------------ MODULE LocalGold ------------------------------
\* Durable boundaries of direct local silver -> Qdrant replay. LocalWriter
\* separately checks retention of the process owner. This model assumes no
\* request remains in flight across Crash; see the explicit limitation in README.
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS ClearBeforeInvalidate, CertifyBeforeFinish
Items == {"native", "connector"}
VARIABLES silver, settings, phase, captured, recipe, documents, remaining,
          certified, stamp, goldRecipe
vars == <<silver, settings, phase, captured, recipe, documents, remaining,
          certified, stamp, goldRecipe>>

Projection(revision, build) ==
    [item \in Items |-> IF revision = 2 /\ item = "native"
                       THEN 0 ELSE revision * 2 + build]

Init == /\ silver = 1
        /\ settings = 0
        /\ phase = "idle"
        /\ captured = 1
        /\ recipe = 0
        /\ documents = Projection(1, 0)
        /\ remaining = {}
        /\ certified = TRUE
        /\ stamp = 1
        /\ goldRecipe = 0

\* Canonical transactions commit before a refresh acquires its snapshot.
Commit == /\ phase = "idle"
          /\ silver = 1
          /\ silver' = 2
          /\ UNCHANGED <<settings, phase, captured, recipe, documents,
                         remaining, certified, stamp, goldRecipe>>
Activate == /\ phase = "idle"
            /\ settings = 0
            /\ settings' = 1
            /\ UNCHANGED <<silver, phase, captured, recipe, documents,
                           remaining, certified, stamp, goldRecipe>>
Capture == /\ phase = "idle"
           /\ captured' = silver
           /\ recipe' = settings
           /\ remaining' = Items
           /\ phase' = IF ClearBeforeInvalidate THEN "clear-first" ELSE "invalidate"
           /\ UNCHANGED <<silver, settings, documents, certified, stamp, goldRecipe>>

ClearFirst(item) == /\ phase = "clear-first"
              /\ documents' = [documents EXCEPT ![item] = 0]
              /\ phase' = "invalidate"
              /\ UNCHANGED <<silver, settings, captured, recipe, remaining,
                             certified, stamp, goldRecipe>>
Invalidate == /\ phase = "invalidate"
              /\ certified' = FALSE
              /\ phase' = "writing"
              /\ UNCHANGED <<silver, settings, captured, recipe, documents,
                             remaining, stamp, goldRecipe>>
Delete(item) == /\ phase = "writing"
               /\ item \in remaining
               /\ documents' = [documents EXCEPT ![item] = 0]
               /\ UNCHANGED <<silver, settings, phase, captured, recipe, remaining,
                              certified, stamp, goldRecipe>>
Write(item) == /\ phase = "writing"
               /\ item \in remaining
               /\ documents' = [documents EXCEPT ![item] = Projection(captured, recipe)[item]]
               /\ remaining' = remaining \ {item}
               /\ UNCHANGED <<silver, settings, phase, captured, recipe,
                              certified, stamp, goldRecipe>>
Certify == /\ phase = "writing"
           /\ (remaining = {} \/ CertifyBeforeFinish)
           /\ silver = captured
           /\ settings = recipe
           /\ certified' = TRUE
           /\ stamp' = captured
           /\ goldRecipe' = recipe
           /\ phase' = "idle"
           /\ UNCHANGED <<silver, settings, captured, recipe, documents, remaining>>
\* An interruption preserves every completed durable write. Retry captures the
\* committed silver again and invalidates before reconstructing all points.
Crash == /\ phase # "idle"
         /\ phase' = "idle"
         /\ UNCHANGED <<silver, settings, captured, recipe, documents,
                        remaining, certified, stamp, goldRecipe>>
Next == Commit \/ Activate \/ Capture \/ Invalidate
        \/ (\E item \in Items : ClearFirst(item) \/ Delete(item) \/ Write(item))
        \/ Certify \/ Crash
Spec == Init /\ [][Next]_vars
TypeOK == /\ silver \in 1..2 /\ settings \in 0..1
          /\ captured \in 1..2 /\ recipe \in 0..1
          /\ stamp \in 1..2 /\ goldRecipe \in 0..1
          /\ documents \in [Items -> 0..5]
          /\ remaining \subseteq Items
          /\ certified \in BOOLEAN
          /\ phase \in {"idle", "clear-first", "invalidate", "writing"}
CurrentMatchesSilver ==
    certified /\ stamp = silver /\ goldRecipe = settings
    => documents = Projection(silver, settings)
=============================================================================
