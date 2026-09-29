# Shared index implementation plan

- [ ] Select the client access boundary with the user.
- [ ] Finalize search/view scope, record ownership, and forget semantics.
- [ ] Define and test the new strict local/shared configuration format.
- [ ] Add transactional publication state and deletion records to SQLite.
- [ ] Implement one concrete shared store with idempotent bounded ingestion,
      revision ordering, and authenticated publisher isolation.
- [ ] Implement shared lexical/semantic retrieval and stored message context.
- [ ] Connect index and forget to publication; expose pending work in status.
- [ ] Connect search and view to the selected shared scope.
- [ ] Test lost acknowledgements, offline retries on no-change indexing,
      out-of-order edits/deletions, owner isolation, incompatible embeddings,
      and context-only message handling.
- [ ] Remove SSH federation and its configuration, worker flags, and tests.
      Add explicit rejection tests for the removed flags and old config shape.
- [ ] Update product instructions, documentation, and configuration examples.
- [ ] Run nextest, Clippy, formatting, and doctests with the persistent build
      directories. Commit only task changes and push main.

Production hosting and deployment remain separate pending decisions.
