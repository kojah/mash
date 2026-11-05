------------------------------ MODULE LocalVec1 ------------------------------
EXTENDS Naturals
CONSTANTS Invalidate, BuildBeforeCertify
VARIABLES source, indexed, dirty, phase, staged, stagedDirty
vars == <<source, indexed, dirty, phase, staged, stagedDirty>>

Init == /\ source = 1 /\ indexed = 1 /\ dirty = FALSE
        /\ phase = "idle" /\ staged = 1 /\ stagedDirty = FALSE

\* A committed embedding mutation invalidates the projection in the same
\* SQLite transaction. Only one SQLite writer is admitted at a time.
Change == /\ phase = "idle" /\ source = 1
          /\ source' = 2 /\ dirty' = IF Invalidate THEN TRUE ELSE dirty
          /\ UNCHANGED <<indexed, phase, staged, stagedDirty>>
Begin == /\ phase = "idle" /\ dirty
         /\ phase' = "building" /\ staged' = 0 /\ stagedDirty' = TRUE
         /\ UNCHANGED <<source, indexed, dirty>>
Build == /\ phase = "building" /\ staged' = source /\ phase' = "built"
         /\ UNCHANGED <<source, indexed, dirty, stagedDirty>>
Certify == /\ phase \in {"building", "built"}
           /\ (~BuildBeforeCertify \/ phase = "built")
           /\ stagedDirty' = FALSE /\ phase' = "certified"
           /\ UNCHANGED <<source, indexed, dirty, staged>>
\* A caller may commit after a failed build: the old dirty marker remains.
\* Publishing staged shadow writes and the marker is one views-file commit.
Commit == /\ phase \in {"building", "built", "certified"}
          /\ indexed' = staged /\ dirty' = stagedDirty /\ phase' = "idle"
          /\ UNCHANGED <<source, staged, stagedDirty>>
Abort == /\ phase # "idle" /\ phase' = "idle"
         /\ UNCHANGED <<source, indexed, dirty, staged, stagedDirty>>
Next == Change \/ Begin \/ Build \/ Certify \/ Commit \/ Abort
TypeOK == /\ source \in {1,2} /\ indexed \in {0,1,2}
          /\ staged \in {0,1,2} /\ dirty \in BOOLEAN /\ stagedDirty \in BOOLEAN
          /\ phase \in {"idle", "building", "built", "certified"}
CurrentMatchesInput == ~dirty => indexed = source
=============================================================================
