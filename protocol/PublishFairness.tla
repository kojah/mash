--------------------------- MODULE PublishFairness -----------------------------
\* Several publishers share one admission capacity.
\*
\* Fifo = TRUE drains the head of the queue, so a later arrival cannot start
\* while an earlier one waits. Fifo = FALSE lets any queued publisher start:
\* a steady stream of arrivals can then starve one forever.
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS Publishers, Capacity, Fifo

VARIABLES arrived, queue, active, done

vars == <<arrived, queue, active, done>>

TypeOK ==
  /\ arrived \in Seq(Publishers)
  /\ queue \in Seq(Publishers)
  /\ active \in SUBSET Publishers
  /\ done \in SUBSET Publishers

Init ==
  /\ arrived = <<>>
  /\ queue = <<>>
  /\ active = {}
  /\ done = {}

Enqueue(p) ==
  /\ p \in Publishers
  /\ p \notin done
  /\ p \notin active
  /\ p \notin {queue[i] : i \in 1..Len(queue)}
  /\ arrived' = Append(arrived, p)
  /\ queue' = Append(queue, p)
  /\ UNCHANGED <<active, done>>

Start(p) ==
  /\ Cardinality(active) < Capacity
  /\ p \in {queue[i] : i \in 1..Len(queue)}
  /\ p \notin active
  /\ (Fifo => p = Head(queue))
  /\ queue' = IF Fifo
              THEN Tail(queue)
              ELSE SelectSeq(queue, LAMBDA x: x # p)
  /\ active' = active \cup {p}
  /\ UNCHANGED <<arrived, done>>

Finish(p) ==
  /\ p \in active
  /\ active' = active \ {p}
  /\ done' = done \union {p}
  /\ UNCHANGED <<arrived, queue>>

Next ==
  \/ \E p \in Publishers : Enqueue(p) \/ Start(p) \/ Finish(p)
  \/ UNCHANGED vars

\* A publisher that arrived before another must not be overtaken by it.
NoOvertake ==
  \A i, j \in 1..Len(arrived) :
    i < j /\ arrived[i] \notin done => arrived[j] \notin done

====
