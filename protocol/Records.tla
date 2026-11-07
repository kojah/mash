------------------------------ MODULE Records ------------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS ReceiptAtomic, RevisionGuard
VARIABLES revision, receipts, executions, staged, forgotten, text
vars == <<revision, receipts, executions, staged, forgotten, text>>
Ops == {"one", "two"}
Init == /\ revision = 0
        /\ receipts = {}
        /\ executions = [op \in Ops |-> 0]
        /\ staged = {}
        /\ forgotten = FALSE
        /\ text = TRUE

\* Both clients read revision zero; their later writes must compare inside
\* the committing SQLite transaction, not when preparing their requests.
Prepare(op) == /\ op \notin staged
               /\ staged' = staged \cup {op}
               /\ UNCHANGED <<revision, receipts, executions, forgotten, text>>
Commit(op) == /\ op \in staged
              /\ op \notin receipts
              /\ ~forgotten
              /\ (~RevisionGuard \/ revision = 0)
              /\ executions[op] < 2
              /\ revision' = revision + 1
              /\ executions' = [executions EXCEPT ![op] = @ + 1]
              /\ receipts' = IF ReceiptAtomic THEN receipts \cup {op} ELSE receipts
              /\ staged' = staged \ {op}
              /\ UNCHANGED <<forgotten, text>>
\* Response loss after commit is represented by preparing the same op again.
Retry(op) == /\ op \in staged /\ op \in receipts
             /\ staged' = staged \ {op}
             /\ UNCHANGED <<revision, receipts, executions, forgotten, text>>
Crash(op) == /\ op \in staged
             /\ staged' = staged \ {op}
             /\ UNCHANGED <<revision, receipts, executions, forgotten, text>>
Forget == /\ ~forgotten
          /\ forgotten' = TRUE
          /\ text' = FALSE
          /\ UNCHANGED <<revision, receipts, executions, staged>>
Next == (\E op \in Ops: Prepare(op) \/ Commit(op) \/ Retry(op) \/ Crash(op)) \/ Forget
Spec == Init /\ [][Next]_vars
TypeOK == /\ revision \in 0..4
          /\ receipts \subseteq Ops
          /\ executions \in [Ops -> 0..2]
          /\ staged \subseteq Ops
          /\ forgotten \in BOOLEAN /\ text \in BOOLEAN
CompleteReceipt == \A op \in Ops: executions[op] > 0 => op \in receipts
SingleSuccessor == executions["one"] + executions["two"] <= 1
AtMostOnce == \A op \in Ops: executions[op] <= 1
ForgottenHasNoText == forgotten => ~text
=============================================================================
