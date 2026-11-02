------------------------------ MODULE Coherence ------------------------------
\* Source identity and ownership for account-scoped connectors (Slack and
\* friends), per docs/source-model.md D1/D2:
\*
\*   D1  device identity includes the node; account identity does not
\*   D2  one owner per account source, first writer wins
\*
\* DeviceScope = TRUE models the account mistake: qualifying an account source
\* by node, so every node believes it owns the same Slack workspace and both
\* ingest it.
\*
\* FirstWriterWins = FALSE models a claim table that overwrites: the last node
\* to start polling silently takes over while the first keeps running.
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Sources,  \* logical sources, e.g. <<"slack","T0123","U0ALICE">>
          Nodes,    \* writer nodes
          DeviceScope,     \* identity key includes the node
          FirstWriterWins  \* a claimed source rejects other claimants

VARIABLES claims,   \* <<key, node>> pairs held in the group
          writers   \* <<source, node>> pairs that wrote silver

vars == <<claims, writers>>

TypeOK ==
  /\ \A p \in claims : p[2] \in Nodes
  /\ \A p \in writers : p[2] \in Nodes

Init == claims = {} /\ writers = {}

Key(s, n) == IF DeviceScope THEN <<s, n>> ELSE s

Claim(s, n) ==
  /\ s \in Sources
  /\ n \in Nodes
  /\ <<Key(s, n), n>> \notin claims
  \* With first-writer-wins a key held by any node rejects the new claim.
  /\ (~FirstWriterWins \/ \A m \in Nodes : <<Key(s, n), m>> \notin claims)
  /\ claims' = claims \cup {<<Key(s, n), n>>}
  /\ UNCHANGED writers

\* A node writes only while it holds the claim for its key.
Write(s, n) ==
  /\ s \in Sources
  /\ n \in Nodes
  /\ <<Key(s, n), n>> \in claims
  /\ writers' = writers \cup {<<s, n>>}
  /\ UNCHANGED claims

Next ==
  \/ \E s \in Sources, n \in Nodes : Claim(s, n) \/ Write(s, n)
  \/ UNCHANGED vars

\* One logical source is ingested by one node only.
NoDoubleIngest ==
  \A s \in Sources, a, b \in Nodes :
     <<s, a>> \in writers /\ <<s, b>> \in writers => a = b

\* A device source may legitimately live on several machines; an account
\* source may not. This is the invariant the connectors must satisfy.
AccountSourcesAreSingleOwner ==
  IF DeviceScope THEN TRUE ELSE NoDoubleIngest

====
