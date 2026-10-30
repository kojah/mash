---------------------------- MODULE SharedViews ----------------------------
EXTENDS Naturals

\* Native view lifecycle for ONE owning API process and its per-node mutex.
\* Separate actions represent separate durable SQLite/Qdrant operations.
\* ClearBeforeInvalidate=TRUE reproduces the current replay ordering.
CONSTANT ClearBeforeInvalidate
Objects == {1, 2}
EmptyDocs == [id \in Objects |-> <<0, 0>>]
VARIABLES revision, silver, settings, docs, mark, recipe, phase, mode, stable
vars == <<revision, silver, settings, docs, mark, recipe, phase, mode, stable>>

Desired == [id \in Objects |-> IF silver[id] = 0 THEN <<0, 0>>
                               ELSE <<silver[id], settings>>]
Current == mark = revision /\ recipe = settings

Init ==
    /\ revision = 1 /\ silver = [id \in Objects |-> 1] /\ settings = 1
    /\ docs = [id \in Objects |-> <<1, 1>>]
    /\ mark = 1 /\ recipe = 1 /\ phase = "idle"
    /\ mode = "replay" /\ stable = FALSE

\* Commit a replacement/deletion while holding the node writer lock. Old
\* metadata need not be cleared atomically: its revision no longer matches.
CommitSilver ==
    /\ ~stable /\ phase = "idle" /\ revision < 2
    /\ revision' = revision + 1
    /\ \E retained \in SUBSET Objects :
        silver' = [id \in Objects |-> IF id \in retained THEN revision + 1 ELSE 0]
    /\ phase' = "invalidate" /\ mode' = "upload"
    /\ UNCHANGED <<settings, docs, mark, recipe, stable>>

\* Settings switch at a quiescent owner restart. No rolling multi-owner
\* activation or distributed writer fencing is claimed by this model.
ChangeSettings ==
    /\ ~stable /\ phase = "idle" /\ settings = 1 /\ settings' = 2
    /\ UNCHANGED <<revision, silver, docs, mark, recipe, phase, mode, stable>>

StartReplay ==
    /\ phase = "idle" /\ (~stable \/ ~Current)
    /\ phase' = IF ClearBeforeInvalidate THEN "clear" ELSE "invalidate"
    /\ mode' = "replay"
    /\ UNCHANGED <<revision, silver, settings, docs, mark, recipe, stable>>

Invalidate ==
    /\ phase = "invalidate" /\ mark' = 0
    /\ phase' = IF mode = "replay" /\ ~ClearBeforeInvalidate
                 THEN "clear" ELSE "write"
    /\ UNCHANGED <<revision, silver, settings, docs, recipe, mode, stable>>

Clear ==
    /\ phase = "clear" /\ docs' = EmptyDocs
    /\ phase' = IF ClearBeforeInvalidate THEN "invalidate" ELSE "write"
    /\ UNCHANGED <<revision, silver, settings, mark, recipe, mode, stable>>

\* Each point write/delete can complete separately. Certification is later.
WritePoint ==
    /\ phase = "write"
    /\ \E id \in Objects :
        /\ docs[id] # Desired[id]
        /\ docs' = [docs EXCEPT ![id] = Desired[id]]
    /\ UNCHANGED <<revision, silver, settings, mark, recipe, phase, mode, stable>>

FinishDocuments ==
    /\ phase = "write" /\ docs = Desired /\ phase' = "certify"
    /\ UNCHANGED <<revision, silver, settings, docs, mark, recipe, mode, stable>>

Certify ==
    /\ phase = "certify"
    /\ mark' = revision /\ recipe' = settings /\ phase' = "idle"
    /\ UNCHANGED <<revision, silver, settings, docs, mode, stable>>

Crash ==
    /\ ~stable /\ phase # "idle" /\ phase' = "idle"
    /\ UNCHANGED <<revision, silver, settings, docs, mark, recipe, mode, stable>>

Stabilize ==
    /\ ~stable /\ stable' = TRUE
    /\ UNCHANGED <<revision, silver, settings, docs, mark, recipe, phase, mode>>

Repair == Invalidate \/ Clear \/ WritePoint \/ FinishDocuments \/ Certify
Next == CommitSilver \/ ChangeSettings \/ StartReplay \/ Repair \/ Crash \/ Stabilize
Spec == Init /\ [][Next]_vars /\ WF_vars(StartReplay) /\ WF_vars(Repair)

TypeOK ==
    /\ revision \in 1..2 /\ silver \in [Objects -> 0..2] /\ settings \in 1..2
    /\ docs \in [Objects -> ((0..2) \X (0..2))]
    /\ mark \in 0..2 /\ recipe \in 1..2
    /\ phase \in {"idle", "invalidate", "clear", "write", "certify"}
    /\ mode \in {"upload", "replay"} /\ stable \in BOOLEAN
NeverFalselyCurrent == Current => docs = Desired
EventuallyCurrent == stable ~> Current
=============================================================================
