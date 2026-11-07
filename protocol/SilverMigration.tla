-------------------------- MODULE SilverMigration --------------------------
EXTENDS Naturals, TLC
CONSTANT Admission
VARIABLES version, tables, lock, writer, migration, history, certificate
vars == <<version, tables, lock, writer, migration, history, certificate>>

Init == /\ version = 62
        /\ tables = FALSE
        /\ lock = "free"
        /\ writer = FALSE
        /\ migration = "idle"
        /\ history = "retained"
        /\ certificate = "unchanged"

\* An old binary checks the version after admission. Its SQLite commit may
\* precede the end of its operation: the lock covers subsequent views work.
OldWriter == /\ lock = "free"
             /\ version = 62
             /\ writer' = TRUE
             /\ lock' = "writer"
             /\ UNCHANGED <<version, tables, migration, history, certificate>>
FinishWriter == /\ writer
                /\ writer' = FALSE
                /\ lock' = "free"
                /\ UNCHANGED <<version, tables, migration, history, certificate>>

Begin == /\ migration = "idle"
         /\ version = 62
         /\ (~Admission \/ lock = "free")
         /\ migration' = "staged"
         /\ lock' = IF Admission THEN "migration" ELSE lock
         /\ UNCHANGED <<version, tables, writer, history, certificate>>
\* Transactional DDL and user_version become visible at one SQLite commit.
Commit == /\ migration = "staged"
          /\ version' = 63
          /\ tables' = TRUE
          /\ migration' = "idle"
          /\ lock' = IF Admission THEN "free" ELSE lock
          /\ UNCHANGED <<writer, history, certificate>>
Crash == /\ migration = "staged"
         /\ migration' = "idle"
         /\ lock' = IF Admission THEN "free" ELSE lock
         /\ UNCHANGED <<version, tables, writer, history, certificate>>

Next == OldWriter \/ FinishWriter \/ Begin \/ Commit \/ Crash
Spec == Init /\ [][Next]_vars
TypeOK == /\ version \in {62,63}
          /\ tables \in BOOLEAN
          /\ writer \in BOOLEAN
          /\ lock \in {"free","writer","migration"}
          /\ migration \in {"idle","staged"}
AtomicSchema == tables = (version = 63)
HistoryPreserved == history = "retained" /\ certificate = "unchanged"
WriterExcluded == writer => (version = 62 /\ migration = "idle")
=============================================================================
