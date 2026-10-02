# Crate and application boundaries

The handwritten Cargo workspace contains `mash` and `mash-query`. Both are
default members, so the ordinary formatting, Clippy, nextest and doctest
commands cover the query crate as well as the application. The root package
is an implicit member; `generated/postgres` is excluded and remains a generated
path dependency. Cargo's [workspace documentation](https://doc.rust-lang.org/cargo/reference/workspaces.html)
describes package selection and default members. Current workspace membership
was also checked with `cargo metadata`, rather than inferred from the manifest.

`mash-query` owns the shared pure query contract: input/language validation,
stopword classification, literal SQLite FTS construction, and exact reference
and quoted-phrase matching. It depends on `url` and `whatlang`, already present
in the application dependency graph. URL parsing uses the same `url::Url` type
previously reexported by Reqwest. It has no storage, network client, inference,
configuration, CLI or MCP dependency, and forbids unsafe Rust.

The application retains its error envelope and exit-code policy. Its six-line
`search_query` module reexports pure operations and maps the query crate's typed
validation errors into usage errors. There is one implementation of the matching
rules. The existing core, backend, ranking and result-collection callers share
it; neither CLI nor MCP supplies a separate query implementation.

Run the eight query contract tests independently during query work:

```bash
env CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif -p mash-query
```

Use the persistent target/temp directories from `AGENTS.md` on Xenia. The
Cranelift aliases configure both handwritten packages. Integrated application,
real-model and backend validation remains necessary when changing query behavior.
The [extraction control](../benchmarks/2026-10-03-query-crate.md) records source
equivalence, isolated test iteration and integrated verification.

The application still owns core operations, history ingestion, model execution,
SQLite and shared storage, observability and the CLI/MCP/worker entry points.
Those boundaries use concrete private modules where their types and lifecycle
remain coupled. This extraction is a reusable pure boundary, not completion of
the broader architecture work or proof that every module belongs in a crate.
