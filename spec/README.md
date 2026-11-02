# Publication protocol model

This directory model-checks the two invariants that concurrent publication
broke in production. It is deliberately small: two variables of interest, no
timing, no storage.

## What is modelled

`Publication.tla` models one node committing baseline batches while several
operations are in flight, plus the service-wide backfill catalog.

| Constant | Meaning | Production value |
|---|---|---|
| `Lanes` | operations a node may have in flight | `MASH_BACKFILL_LANES`, default 4 |
| `Rebase` | the head is read inside the commit lock | `true` after `rebase.silver` |
| `Scoped` | identity includes group and node | `true` after `scope.backfill` |
| `Entries` | backfill attempts registered in the catalog | one per (group, node) |

## What is checked

- `OrderedRevisions` — two committed batches for one node never share a
  revision.
- `ContiguousRevisions` — committed revisions form `1..N`.
- `UniqueBackfillIdentity` — one backfill identity names one destination.

## What is deliberately not modelled

SQLite lock timeouts, retry/QoS, wire credit and views certification latency.
The `database is locked` failure we hit was a 5 s timeout against a longer
critical section; a timed model would have to encode the implementation's
durations, and the answer ("two writers, one lock") is not the property that
broke.

## Running

```
./run.sh
```

`run.sh` downloads `tla2tools.jar` if needed and checks four configurations:

| config | expectation | demonstrates |
|---|---|---|
| `ok.cfg` | clean | the fixed protocol |
| `sequential.cfg` | clean | `Lanes = 1` hides the race (why the suite passed) |
| `rebase-bug.cfg` | violates `OrderedRevisions` | stale head read across lanes |
| `scope-bug.cfg` | violates `UniqueBackfillIdentity` | shared `"auto"` identity |

## Coherence (connectors)

`Coherence.tla` encodes the two rules account-scoped connectors must satisfy
before Slack onboarding (`docs/source-model.md` D1/D2):

- device identity includes the node; account identity does not
- one owner per account source, first writer wins

| config | expectation | demonstrates |
|---|---|---|
| `coherence-ok.cfg` | clean | account sources are group-wide and first-writer-wins |
| `coherence-device-scope.cfg` | violates `NoDoubleIngest` | node-qualified account identity: both machines ingest one workspace |
| `coherence-overwrite.cfg` | violates `NoDoubleIngest` | an overwriting claim: the second node takes over while the first keeps polling |

The two bug configurations are kept, not deleted: they are the regression tests
for the spec itself. If someone models the protocol at a level where the bug
disappears, these configs stop failing and `run.sh` fails loudly.
