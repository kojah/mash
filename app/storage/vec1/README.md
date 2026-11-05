# Bundled Vec1 for the experimental `vec1` feature

`vec1.c` is unchanged SQLite Vec1 0.8 from SQLite mirror revision
`edb49992bb532644f696f8e488a51860acf9439a`, `ext/vec1/vec1.c`.
SHA256: `b341c15fe10e41a9b96981ff3a9c217ea0774b4e04932b5acf3dac1320aeceb1`.
The upstream public-domain notice is retained in the file.

Source: https://github.com/sqlite/sqlite/blob/edb49992bb532644f696f8e488a51860acf9439a/ext/vec1/vec1.c
Documentation: https://www.sqlite.org/vec1.html

`register.c` installs the static extension using SQLite's native auto-extension
mechanism. Application Rust continues to forbid unsafe code; no external shared
library or runtime extension-loading permission is needed. Portable scalar C is
used for this first trial. SIMD/platform tuning is separate acceptance work.

Vec1 0.8 fails a populated → empty → populated table replacement sequence in
one transaction when the intermediate table has never initialized its vector
shape. Mash inserts and deletes a two-dimensional seed when building an empty
projection, before publishing readiness. The seed never enters `vec1_points`
or committed query results; the generation/empty-rebuild regression covers it.
