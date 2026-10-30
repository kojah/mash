---------------------------- MODULE LocalUpgrade ----------------------------
EXTENDS FiniteSets
CONSTANTS ValidateFirst, DatabaseLast
OldFiles == {"database", "wal", "attachments", "views"}
NewFiles == {"database", "attachments"}
VARIABLES phase, marker, validated, retired, installed
vars == <<phase, marker, validated, retired, installed>>
Init == /\ phase = "idle" /\ marker = FALSE /\ validated = FALSE
        /\ retired = {} /\ installed = {}
Begin == /\ phase = "idle" /\ marker' = TRUE /\ phase' = "export"
         /\ UNCHANGED <<validated, retired, installed>>
Export == /\ phase = "export"
          /\ phase' = IF ValidateFirst THEN "import" ELSE "retire"
          /\ UNCHANGED <<marker, validated, retired, installed>>
Import == /\ phase = "import" /\ validated' = TRUE /\ phase' = "retire"
          /\ UNCHANGED <<marker, retired, installed>>
Reject == /\ phase = "import" /\ phase' = "rejected" /\ marker' = FALSE
          /\ UNCHANGED <<validated, retired, installed>>
Retire == /\ phase = "retire" /\ retired # OldFiles
          /\ \E file \in OldFiles \ retired : retired' = retired \cup {file}
          /\ UNCHANGED <<phase, marker, validated, installed>>
PrepareInstall == /\ phase = "retire" /\ retired = OldFiles
                  /\ phase' = "install"
                  /\ UNCHANGED <<marker, validated, retired, installed>>
Install == /\ phase = "install"
           /\ \E file \in NewFiles \ installed :
                /\ (~DatabaseLast \/ file # "database" \/ "attachments" \in installed)
                /\ installed' = installed \cup {file}
           /\ UNCHANGED <<phase, marker, validated, retired>>
Finish == /\ phase = "install" /\ installed = NewFiles
          /\ phase' = "done" /\ marker' = FALSE
          /\ UNCHANGED <<validated, retired, installed>>
\* A failed filesystem operation or process kill may leave every completed
\* rename/link intact. Retrying is refused while marker remains; manual
\* recovery is deliberately outside this safety-only model.
Stop == /\ phase \in {"export", "import", "retire", "install"}
        /\ phase' = "stopped"
        /\ UNCHANGED <<marker, validated, retired, installed>>
Next == Begin \/ Export \/ Import \/ Reject \/ Retire \/ PrepareInstall \/ Install \/ Finish \/ Stop
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase \in {"idle", "export", "import", "rejected", "retire", "install", "done", "stopped"}
          /\ marker \in BOOLEAN /\ validated \in BOOLEAN
          /\ retired \subseteq OldFiles /\ installed \subseteq NewFiles
RetireRequiresValidation == retired # {} => validated
DatabaseHasAttachments == "database" \in installed => "attachments" \in installed
IncompleteCutoverBlocked == retired # {} /\ phase # "done" => marker
RejectedImportKeepsActive == phase = "rejected" => retired = {} /\ installed = {}
=============================================================================
