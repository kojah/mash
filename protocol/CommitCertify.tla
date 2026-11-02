------------------------------ MODULE CommitCertify -----------------------------
\* Certification runs after the silver commit and outside the admission permit,
\* so a slow certification for an older revision can finish after a newer one.
\*
\* Guard = TRUE drops a certification whose input is older than the last one
\* written, which is what Store::certify does with NodeViews.input.
\* StampFromInput = TRUE stamps only the revision the views were built from; the
\* shipped code does this. FALSE is the historical shape that stamped whatever
\* head certification happened to read, so a slow certification claimed a newer
\* revision while serving older content.
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Revisions, Guard, StampFromInput

VARIABLES silver, pending, content, input, stamp

vars == <<silver, pending, content, input, stamp>>

TypeOK ==
  /\ silver \in Revisions \union {0}
  /\ pending \in SUBSET Revisions
  /\ content \in SUBSET Revisions
  /\ input \in Revisions \union {0}
  /\ stamp \in Revisions \union {0}

ChatOf(r) == IF r = 0 THEN {} ELSE {r}

Init ==
  /\ silver = 0
  /\ pending = {}
  /\ content = {}
  /\ input = 0
  /\ stamp = 0

Commit(r) ==
  /\ r \in Revisions
  /\ r > silver
  /\ silver' = r
  /\ UNCHANGED <<pending, content, input, stamp>>

CertifyStart(r) ==
  /\ r \in Revisions
  /\ r <= silver
  /\ r \notin pending
  /\ pending' = pending \union {r}
  /\ UNCHANGED <<silver, content, input, stamp>>

CertifyFinish(r) ==
  /\ r \in pending
  /\ pending' = pending \ {r}
  /\ IF Guard /\ input > r
     THEN UNCHANGED <<content, input, stamp>>
     ELSE /\ content' = ChatOf(r)
          /\ input' = r
          /\ stamp' = IF StampFromInput THEN r ELSE silver
  /\ UNCHANGED silver

Next ==
  \/ \E r \in Revisions : Commit(r) \/ CertifyStart(r) \/ CertifyFinish(r)
  \/ UNCHANGED vars

\* Views must never claim the current head while holding another revision's chats.
NoStaleContent ==
  stamp = silver => content = ChatOf(silver)

====
