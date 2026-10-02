# Pure query crate extraction

`mash-c0d.3.7`, Xenia, 2026-10-03.

Query validation, literal SQLite compilation and exact reference/phrase matching
now live in `mash-query`, independently of the application test binary. Eight
existing contract tests moved with their implementation; none were ignored,
deleted or weakened. Language tests read the new typed error's display text
instead of the application error envelope. The application retains its usage
error/exit-code mapping and the same validation messages.

The compiler/reference/phrase portion and its tests compare byte-for-byte to
the previous source after normalizing public visibility, `must_use` annotations
and the direct URL import. Installed Reqwest source confirms `reqwest::Url`
reexports `url::Url`; Cargo retains the existing URL version 2.5.8. Language and
length/common-word decisions retain their existing rules, returning typed crate
errors instead of constructing application errors. This source audit is not a
formal proof for every possible input; the moved tests and integrated searches
remain required.

Workspace metadata confirms exactly two handwritten members/default members.
The generated PostgreSQL crate is an excluded path dependency. An initial
explicit `.` member caused the installed Cargo to include that dependency;
using the implicit root member and checking metadata corrected the scope.
An initial query-only build exposed the former Reqwest URL reference; the pure
crate now uses the existing URL parser directly without a network dependency.
No third-party version was updated.

## Isolated development control

Use the persistent integrated target, installed nightly Cranelift, incremental
compilation enabled and an empty compiler wrapper. Run all eight query tests,
then change only a failure-message argument in one assertion and rerun. Restore
the exact original bytes and rerun. No owned main-crate compilation overlapped.
`/usr/bin/time` records complete Cargo/nextest process work:

| Control | Elapsed, seconds | User / system CPU, seconds | Peak RSS, KiB |
| --- | ---: | ---: | ---: |
| Warm unchanged query crate | .78 | .42 / .38 | 92,496 |
| Assertion-message edit | 1.01 | .69 / .70 | 219,676 |
| Exact restored source | .98 | .67 / .61 | 216,324 |

All three runs pass the same eight tests. This is one small warm edit, not
latency percentiles, a cold dependency build or a matched old/new main-crate
compile comparison. Peak counters include Cargo/compiler/test subprocess work;
they are not application memory. Other host load was uncontrolled. The control
demonstrates a roughly one-second isolated query edit cycle; it establishes no
whole-project build or search-latency speedup.

Original/restored source bytes, metadata, source equivalence and timing/log
artifacts are retained outside Git under `/home/james/scratch/mash-artifacts`,
with `query-crate-` and `query-workspace-` prefixes. The temporary assertion
message is absent from the committed source.

## Integrated verification

The final workspace Cranelift suite passed 597 tests in 9.078 seconds, with
115 opt-in cases excluded. Three installed-model controls passed in 5.564
seconds: the labeled synthetic relevance matrix, thread discovery with scoped
message search, and selection of the fourth thread anchor with ranked evidence.
These use the explicitly supplied existing local models and download no assets.
They are integration controls, not independent production relevance evaluation
or end-user latency measurements. Strict all-target Clippy passed in 25.80 seconds.
Formatting and whitespace checks passed. Workspace doctests passed (one
application example, no query examples). The help/version follow-up is
tracked as `mash-c0d.8.21`.

`cargo run-clif -- --version` selected the correct application binary, but
exposed an existing CLI behavior: informational Clap output becomes a JSON
usage error with exit code 2. Workspace binary selection worked; application
help/version handling needs a separate focused fix. No deployment, fresh
PostgreSQL/Redis/Qdrant service test or cross-platform run is claimed here.
